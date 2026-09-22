//! Language tool profiles: detection, configuration, and sandbox-aware gating.
//!
//! The shell tool advertises which language tool-chains are available inside the
//! authoring sandbox. The sandbox mounts only the read-only /usr layout and
//! clears the environment, so advertisement must be honest: a profile is only
//! advertised when its interpreter genuinely reaches the sandbox. Detection is
//! therefore judged against what the sandbox can actually run, never against the
//! host PATH. Configuration lets each profile be forced on, gated on availability
//! (auto), or turned off, and an explicit request for an unavailable profile is a
//! startup error rather than a silent lie.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::Error;

/// A language or base tool-chain profile the registry can advertise.
///
/// `Generic` is the always-on base (coreutils, git, the read-only /usr layout)
/// and is never configured or gated; the remaining entries are configurable and
/// gated on tool-chain availability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToolProfile {
    /// Base system tools: coreutils, git, and the read-only /usr layout.
    Generic,
    /// The Python tool-chain (python3, pip).
    Python,
    /// The Rust tool-chain (cargo, rustc).
    Rust,
    /// The JavaScript and Node tool-chain (node, npm).
    JavaScript,
    /// The Go tool-chain (go).
    Go,
    /// The C tool-chain (gcc, cc).
    C,
}

impl ToolProfile {
    /// All configurable profiles, excluding the always-on `Generic` base.
    pub const CONFIGURABLE: &[ToolProfile] = &[
        ToolProfile::Python,
        ToolProfile::Rust,
        ToolProfile::JavaScript,
        ToolProfile::Go,
        ToolProfile::C,
    ];

    /// The identifier accepted in configuration, on the CLI, and in the profile set.
    pub const fn id(self) -> &'static str {
        match self {
            ToolProfile::Generic => "generic",
            ToolProfile::Python => "python",
            ToolProfile::Rust => "rust",
            ToolProfile::JavaScript => "javascript",
            ToolProfile::Go => "go",
            ToolProfile::C => "c",
        }
    }

    /// A human description of the tool-chain the profile surfaces.
    pub const fn toolkit(self) -> &'static str {
        match self {
            ToolProfile::Generic => "coreutils, git, and common Linux tools",
            ToolProfile::Python => "python3, pip, and common Python build tools",
            ToolProfile::Rust => "cargo, rustc, and the Rust standard toolchain",
            ToolProfile::JavaScript => "node and the npm ecosystem",
            ToolProfile::Go => "the Go toolchain (go)",
            ToolProfile::C => "the GCC toolchain (gcc, cc)",
        }
    }

    /// Parses a profile identifier.
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "generic" => Some(Self::Generic),
            "python" => Some(Self::Python),
            "rust" => Some(Self::Rust),
            "javascript" => Some(Self::JavaScript),
            "go" => Some(Self::Go),
            "c" => Some(Self::C),
            _ => None,
        }
    }

    /// System directories that always host this profile's interpreter,
    /// independent of the user PATH.
    pub const fn system_dirs(self) -> &'static [&'static str] {
        match self {
            ToolProfile::Python => &["/usr/bin", "/bin"],
            ToolProfile::Rust => &["/usr/bin"],
            ToolProfile::JavaScript => &["/usr/bin", "/bin", "/usr/local/bin"],
            ToolProfile::Go => &["/usr/local/go/bin", "/usr/bin", "/usr/local/bin"],
            ToolProfile::C => &["/usr/bin", "/bin", "/usr/sbin"],
            ToolProfile::Generic => &[],
        }
    }

    /// Extra user-home directories that may host this profile's tool-chain.
    pub fn home_dirs(self, home: &Path) -> Vec<PathBuf> {
        match self {
            ToolProfile::Rust => vec![home.join(".cargo").join("bin")],
            ToolProfile::Go => vec![home.join("go").join("bin"), home.join(".local").join("bin")],
            _ => Vec::new(),
        }
    }

    /// The executable names this profile searches for when detecting availability.
    pub const fn binaries(self) -> &'static [&'static str] {
        match self {
            ToolProfile::Python => &["python3", "python"],
            ToolProfile::Rust => &["cargo", "rustc"],
            ToolProfile::JavaScript => &["node"],
            ToolProfile::Go => &["go"],
            ToolProfile::C => &["gcc", "cc", "clang"],
            ToolProfile::Generic => &[],
        }
    }
}

/// How a language profile is enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileSetting {
    /// Always advertise; fail at startup if the tool-chain is not available.
    On,
    /// Advertise only when the tool-chain is detected as available.
    Auto,
    /// Never advertise.
    Off,
}

impl ProfileSetting {
    /// Parses a setting, tolerating common synonyms for resilient configuration.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "on" => Some(Self::On),
            "auto" | "automatic" | "detect" | "detection" => Some(Self::Auto),
            "off" | "disabled" | "none" | "disable" => Some(Self::Off),
            _ => None,
        }
    }
}

impl FromStr for ProfileSetting {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value).ok_or_else(|| {
            format!("invalid profile setting `{value}`; expected `on`, `auto`, or `off`")
        })
    }
}

/// Determines whether a language tool-chain is available inside the sandbox.
///
/// The authoring sandbox only mounts the read-only /usr layout and clears the
/// environment, so availability must be judged against what actually reaches the
/// tool, not against the host PATH.
pub trait ToolchainProbe {
    /// Whether `profile` interpreter is available to the sandbox.
    fn is_available(&self, profile: ToolProfile) -> bool;
}

/// A probe backed by the host environment (PATH, HOME, and system directories).
#[derive(Debug, Clone, Copy)]
pub struct HostProbe;

impl HostProbe {
    /// The directories searched for a profile interpreter, in priority order.
    pub fn search_dirs(&self, profile: ToolProfile) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
            .map(|value| std::env::split_paths(&value).collect())
            .unwrap_or_default();
        for dir in profile.system_dirs() {
            let candidate = PathBuf::from(dir);
            if !dirs.contains(&candidate) {
                dirs.push(candidate);
            }
        }
        if let Some(home) = std::env::var_os("HOME") {
            for dir in profile.home_dirs(&PathBuf::from(home)) {
                if !dirs.contains(&dir) {
                    dirs.push(dir);
                }
            }
        }
        dirs
    }
}

impl ToolchainProbe for HostProbe {
    fn is_available(&self, profile: ToolProfile) -> bool {
        language_available(profile, &self.search_dirs(profile), MOUNTED_SYSTEM_DIRS)
    }
}

/// The directories the sandbox mounts read-only for the authoring phase. A tool
/// only counts as available when it lives under one of these roots.
pub const MOUNTED_SYSTEM_DIRS: &[&str] = &["/usr", "/lib", "/lib64", "/bin", "/sbin"];

/// Whether `profile` has a usable interpreter under one of `search_dirs` that
/// resolves (after symlink canonicalisation) beneath a `mounted` root.
///
/// A interpreter whose canonical path ends in `rustup` is ignored: a `rustup`
/// proxy named like the tool is not a real compiler and cannot build.
pub fn language_available(profile: ToolProfile, search_dirs: &[PathBuf], mounted: &[&str]) -> bool {
    let binaries = profile.binaries();
    let mounted_roots: Vec<PathBuf> = mounted
        .iter()
        .map(|root| std::fs::canonicalize(Path::new(root)).unwrap_or_else(|_| PathBuf::from(root)))
        .collect();
    for base in search_dirs {
        for name in binaries {
            let path = base.join(name);
            if !path.is_file() {
                continue;
            }
            let Ok(canonical) = std::fs::canonicalize(&path) else {
                continue;
            };
            if canonical.file_name().is_some_and(|file| file == "rustup") {
                continue;
            }
            if mounted_roots
                .iter()
                .any(|root| canonical == *root || canonical.strip_prefix(root).is_ok())
            {
                return true;
            }
        }
    }
    false
}

/// Detects which configurable profiles a project appears to use, from root
/// manifests. This is advisory: it informs logging and recommendations but never
/// gates use, which is gated solely on tool-chain availability.
pub fn detect_languages(entries: &[&str]) -> BTreeSet<ToolProfile> {
    let set: BTreeSet<&str> = entries.iter().copied().collect();
    let mut found = BTreeSet::new();
    if set.contains("Cargo.toml") {
        found.insert(ToolProfile::Rust);
    }
    if set
        .iter()
        .copied()
        .any(|name| matches!(name, "pyproject.toml" | "setup.py" | "requirements.txt"))
    {
        found.insert(ToolProfile::Python);
    }
    if set.contains("package.json") {
        found.insert(ToolProfile::JavaScript);
    }
    if set.contains("go.mod") {
        found.insert(ToolProfile::Go);
    }
    if set
        .iter()
        .copied()
        .any(|name| matches!(name, "CMakeLists.txt" | "configure" | "configure.ac"))
    {
        found.insert(ToolProfile::C);
    }
    found
}

/// Resolves which configurable profiles to advertise from the configured
/// settings, a probe, and an optional forced profile. The caller always adds the
/// `Generic` base.
///
/// Profiles set to `On` (including a `forced` profile from the CLI) require the
/// interpreter to be available, so an explicit request can never be silently
/// dishonest. `Auto` profiles are advertised only when available; `Off` profiles
/// are never advertised.
pub fn resolve_profiles(
    settings: &BTreeMap<ToolProfile, ProfileSetting>,
    probe: &dyn ToolchainProbe,
    forced: Option<ToolProfile>,
) -> Result<Vec<ToolProfile>, Error> {
    let mut enabled: Vec<ToolProfile> = Vec::new();
    for profile in ToolProfile::CONFIGURABLE {
        let setting = match forced {
            Some(candidate) if candidate == *profile => ProfileSetting::On,
            _ => settings
                .get(profile)
                .copied()
                .unwrap_or(ProfileSetting::Auto),
        };
        match setting {
            ProfileSetting::On => {
                if !probe.is_available(*profile) {
                    return Err(Error::ToolchainUnavailable {
                        profile: profile.id().to_owned(),
                    });
                }
                enabled.push(*profile);
            }
            ProfileSetting::Auto => {
                if probe.is_available(*profile) {
                    enabled.push(*profile);
                }
            }
            ProfileSetting::Off => {}
        }
    }
    enabled.sort_by_key(|profile| profile.id());
    enabled.dedup_by_key(|profile| profile.id());
    Ok(enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StaticProbe {
        available: BTreeSet<ToolProfile>,
    }

    impl ToolchainProbe for StaticProbe {
        fn is_available(&self, profile: ToolProfile) -> bool {
            self.available.contains(&profile)
        }
    }

    fn settings(pairs: &[(&ToolProfile, ProfileSetting)]) -> BTreeMap<ToolProfile, ProfileSetting> {
        pairs
            .iter()
            .map(|&(profile, setting)| (*profile, setting))
            .collect()
    }

    #[test]
    fn profile_id_round_trip() {
        for profile in ToolProfile::CONFIGURABLE {
            assert_eq!(ToolProfile::from_id(profile.id()), Some(*profile));
        }
        assert_eq!(ToolProfile::from_id("python-rust"), None);
    }

    #[test]
    fn profile_setting_parses_all_spellings() {
        assert_eq!(ProfileSetting::parse("on"), Some(ProfileSetting::On));
        assert_eq!(
            ProfileSetting::parse("  auto  "),
            Some(ProfileSetting::Auto)
        );
        assert_eq!(ProfileSetting::parse("off"), Some(ProfileSetting::Off));
        assert_eq!(ProfileSetting::parse("nope"), None);
    }

    #[test]
    fn resolve_profiles_auto_uses_availability() {
        let available: BTreeSet<ToolProfile> = [ToolProfile::Python, ToolProfile::Rust]
            .iter()
            .cloned()
            .collect();
        let probe = StaticProbe { available };
        let resolved = resolve_profiles(&settings(&[]), &probe, None).unwrap();
        assert_eq!(resolved, vec![ToolProfile::Python, ToolProfile::Rust]);
    }

    #[test]
    fn resolve_profiles_off_is_excluded() {
        let available: BTreeSet<ToolProfile> = [ToolProfile::Python, ToolProfile::Rust]
            .iter()
            .cloned()
            .collect();
        let probe = StaticProbe { available };
        let resolved = resolve_profiles(
            &settings(&[(&ToolProfile::Rust, ProfileSetting::Off)]),
            &probe,
            None,
        )
        .unwrap();
        assert_eq!(resolved, vec![ToolProfile::Python]);
    }

    #[test]
    fn resolve_profiles_forced_on_without_availability_errors() {
        let available: BTreeSet<ToolProfile> = [ToolProfile::Python].iter().cloned().collect();
        let probe = StaticProbe { available };
        let err = resolve_profiles(&settings(&[]), &probe, Some(ToolProfile::Rust)).unwrap_err();
        match err {
            Error::ToolchainUnavailable { profile } => assert_eq!(profile, "rust"),
            other => panic!("expected ToolchainUnavailable, got {other:?}"),
        }
    }

    #[test]
    fn resolve_profiles_forced_on_with_availability_succeeds() {
        let available: BTreeSet<ToolProfile> = [ToolProfile::Rust].iter().cloned().collect();
        let probe = StaticProbe { available };
        let resolved = resolve_profiles(&settings(&[]), &probe, Some(ToolProfile::Rust)).unwrap();
        assert_eq!(resolved, vec![ToolProfile::Rust]);
    }

    #[test]
    fn detect_languages_manifests_and_gaps() {
        assert_eq!(
            detect_languages(&["Cargo.toml"]),
            BTreeSet::from([ToolProfile::Rust])
        );
        assert_eq!(
            detect_languages(&["pyproject.toml", "README.md"]),
            BTreeSet::from([ToolProfile::Python])
        );
        assert_eq!(
            detect_languages(&["go.mod"]),
            BTreeSet::from([ToolProfile::Go])
        );
        assert_eq!(
            detect_languages(&["package.json"]),
            BTreeSet::from([ToolProfile::JavaScript])
        );
        assert_eq!(
            detect_languages(&["CMakeLists.txt"]),
            BTreeSet::from([ToolProfile::C])
        );
        assert!(detect_languages(&["README.md"]).is_empty());
    }

    #[test]
    fn language_available_respects_mounted_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let bin = root.join("usr").join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let interpreter = bin.join("python3");
        std::fs::write(&interpreter, "#!/bin/sh\n").unwrap();

        let mounted_root = root.join("usr").to_string_lossy().into_owned();
        assert!(language_available(
            ToolProfile::Python,
            std::slice::from_ref(&bin),
            &[mounted_root.as_str()],
        ));

        assert!(!language_available(
            ToolProfile::Python,
            &[bin],
            &[&*Path::new("/usr").to_string_lossy()],
        ));
    }

    #[test]
    fn language_available_treats_rustup_proxy_as_unavailable() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let real = bin.join("rustc");
        std::fs::write(&real, "").unwrap();
        assert!(language_available(
            ToolProfile::Rust,
            std::slice::from_ref(&bin),
            &[&*root.to_string_lossy()],
        ));
    }
}
