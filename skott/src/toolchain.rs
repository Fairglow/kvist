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
//! Workspace resolution additionally uses [`rust_environment`] to expose one
//! concrete host-installed Rust toolchain read-only, with explicitly offline
//! dependencies and private scratch. The generic probe never treats rustup
//! proxies as installed compilers.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::Error;

/// Concrete read-only Rust resources; no maerg provisioning authority.
#[path = "rust_environment.rs"]
pub mod rust_environment;

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
            ToolProfile::C => &["/usr/bin", "/bin"],
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
pub const MOUNTED_SYSTEM_DIRS: &[&str] = &["/usr", "/lib", "/lib64", "/bin"];

/// Fixed System search directories shared by generic and prepared environments.
pub(crate) const SANDBOX_SYSTEM_PATH: &str = "/usr/local/bin:/usr/bin:/bin";

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
            if galla::validation::is_administrative_path(&path) || !path.is_file() {
                continue;
            }
            let Ok(canonical) = std::fs::canonicalize(&path) else {
                continue;
            };
            if galla::validation::is_administrative_path(&canonical) {
                continue;
            }
            if canonical.file_name().is_some_and(|file| file == "rustup") {
                continue;
            }
            if nix::unistd::access(&canonical, nix::unistd::AccessFlags::X_OK).is_err() {
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
    LANGUAGE_MANIFESTS
        .iter()
        .filter(|(name, _)| entries.contains(name))
        .map(|(_, profile)| *profile)
        .collect()
}

const LANGUAGE_MANIFESTS: [(&str, ToolProfile); 9] = [
    ("Cargo.toml", ToolProfile::Rust),
    ("pyproject.toml", ToolProfile::Python),
    ("setup.py", ToolProfile::Python),
    ("requirements.txt", ToolProfile::Python),
    ("package.json", ToolProfile::JavaScript),
    ("go.mod", ToolProfile::Go),
    ("CMakeLists.txt", ToolProfile::C),
    ("configure", ToolProfile::C),
    ("configure.ac", ToolProfile::C),
];

/// A minimal, hand-rolled matcher for a subset of `.gitignore` semantics, used
/// only by the advisory language detection so that the project's own declaration
/// of what it does *not* use is respected.
///
/// It is intentionally small: it parses lines, honors last-match-wins (including
/// `!` negation), and matches a single top-level entry name against anchored,
/// basename, and directory-only patterns. It never consults git state and does
/// not model `git check-ignore --cached` (a file that is ignored *and* tracked
/// with `git add -f` is still part of the project despite the pattern). That is
/// a deliberate trade-off for an offline, dependency-free advisory: the
/// `.gitignore` is the project's authoritative statement of "not part of me."
#[derive(Default)]
pub struct GitIgnore {
    /// Each rule is `(negated, pattern)`, in file order. The last matching rule
    /// determines whether an entry is ignored, matching git semantics.
    rules: Vec<(bool, String)>,
}

impl GitIgnore {
    /// Parse a `.gitignore` document from its text. Empty or whitespace-only
    /// content yields a matcher that ignores nothing.
    pub fn parse(content: &str) -> Self {
        let mut rules = Vec::new();
        for raw in content.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (negated, pattern) = match line.strip_prefix('!') {
                Some(rest) => (true, rest.trim().to_owned()),
                None => (false, line.to_owned()),
            };
            rules.push((negated, pattern));
        }
        Self { rules }
    }

    /// Whether a single top-level entry (`name`) — a file or a directory — is
    /// ignored by this file.
    pub fn matches(&self, name: &str) -> bool {
        let mut ignored = false;
        for (negated, pattern) in &self.rules {
            if pattern_matches(pattern, name) {
                ignored = !*negated;
            }
        }
        ignored
    }
}

/// Match one top-level entry name against a single `.gitignore` pattern.
fn pattern_matches(pattern: &str, name: &str) -> bool {
    // A trailing slash marks a directory-only pattern; for our top-level entry
    // filter, dropping the slash is sufficient because it also drops the
    // directory entry itself from the listing.
    let pattern = pattern.strip_suffix('/').unwrap_or(pattern);
    // A leading slash anchors the pattern to the project root; a slash elsewhere
    // anchors it to the .gitignore's directory, which for a root .gitignore is
    // likewise the project root. In both cases a top-level entry is compared by
    // basename, so the distinction collapses here.
    let anchored = pattern.starts_with('/');
    let pattern = pattern.strip_prefix('/').unwrap_or(pattern);
    let core = pattern.strip_prefix("**/").unwrap_or(pattern);
    if anchored {
        core == name
    } else {
        core == name || name.ends_with(&format!("/{}", core))
    }
}

/// Detects which configurable profiles a project *uses*: its root manifests,
/// excluding any entry the project's `.gitignore` declares as not part of
/// itself.
///
/// This is the advisory counterpart to [`resolve_profiles`]. Unlike the
/// capability gate (which is manifest-free, gitignore-blind, and judges the
/// sandbox against what its interpreters actually reach), this intentionally
/// honors `.gitignore`. Dev-only tooling — an installed editor CLI, vendored
/// build output, and the like — is read from the filesystem but is not what the
/// project uses, so it must not define the project's language. When no
/// `.gitignore` is present, or it cannot be read, detection falls back to the
/// raw manifest scan so a missing ignore file never hides a real language.
/// Only the fixed manifest names are inspected. Ignore files must be regular,
/// non-link files of at most 64 KiB; special files never block startup or quit.
pub fn detect_project_languages(dir: &Path) -> BTreeSet<ToolProfile> {
    let gitignore = match read_project_ignore(&dir.join(".gitignore")) {
        Ok(ignore) => ignore,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => GitIgnore::default(),
        Err(error) => {
            tracing::warn!(%error, "project ignore unavailable; using raw manifest detection");
            GitIgnore::default()
        }
    };
    LANGUAGE_MANIFESTS
        .iter()
        .filter_map(|(name, profile)| {
            if gitignore.matches(name) {
                return None;
            }
            match std::fs::symlink_metadata(dir.join(name)) {
                Ok(_) => Some(*profile),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => {
                    tracing::debug!(%error, manifest = name, "advisory manifest unavailable");
                    None
                }
            }
        })
        .collect()
}

fn read_project_ignore(path: &Path) -> std::io::Result<GitIgnore> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    const MAX_IGNORE_BYTES: u64 = 64 * 1024;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags((nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_NONBLOCK).bits())
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_IGNORE_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            ".gitignore must be a regular non-link file of at most 64 KiB",
        ));
    }
    let mut content = String::new();
    file.take(MAX_IGNORE_BYTES + 1)
        .read_to_string(&mut content)?;
    if content.len() as u64 > MAX_IGNORE_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            ".gitignore grew beyond 64 KiB",
        ));
    }
    Ok(GitIgnore::parse(&content))
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
    fn gitignore_parsing_last_match_wins_and_negates() {
        let ignore = GitIgnore::parse("# comment\n\npackage.json\n!package.json\n");
        // Negated last: package.json is *not* ignored.
        assert!(!ignore.matches("package.json"));
        let ignore = GitIgnore::parse("package.json\n");
        assert!(ignore.matches("package.json"));
    }

    #[test]
    fn gitignore_matches_top_level_entries() {
        let ignore = GitIgnore::parse("node_modules\n");
        assert!(ignore.matches("node_modules"));
        assert!(!ignore.matches("src"));
        // Anchored patterns match the same top-level name.
        let ignore = GitIgnore::parse("/node_modules");
        assert!(ignore.matches("node_modules"));
        // Directory-only patterns drop their trailing slash.
        let ignore = GitIgnore::parse("node_modules/\n");
        assert!(ignore.matches("node_modules"));
    }

    #[test]
    fn detect_project_languages_respects_gitignore() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path();

        // Dev tooling present but ignored: Rust is the only real language.
        std::fs::write(dir.join("package.json"), "{}").unwrap();
        std::fs::write(dir.join("package-lock.json"), "{}").unwrap();
        std::fs::create_dir_all(dir.join("node_modules")).unwrap();
        std::fs::write(dir.join("Cargo.toml"), "[package]\n").unwrap();
        std::fs::write(
            dir.join(".gitignore"),
            "/package.json\n/package-lock.json\n/node_modules\n",
        )
        .unwrap();

        let used = detect_project_languages(dir);
        assert_eq!(used, BTreeSet::from([ToolProfile::Rust]));

        // Removing the gitignore surfaces the dev tooling again.
        std::fs::remove_file(dir.join(".gitignore")).unwrap();
        let used = detect_project_languages(dir);
        assert_eq!(
            used,
            BTreeSet::from([ToolProfile::Rust, ToolProfile::JavaScript])
        );
    }

    #[test]
    fn detect_project_languages_falls_back_without_gitignore() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path();
        std::fs::write(dir.join("package.json"), "{}").unwrap();
        // No .gitignore present: detection falls back to a raw manifest scan and
        // reports the manifest, so a missing ignore file never hides a language.
        let used = detect_project_languages(dir);
        assert_eq!(used, BTreeSet::from([ToolProfile::JavaScript]));
    }

    #[test]
    fn detect_project_languages_handles_missing_directory() {
        let used = detect_project_languages(Path::new("/does/not/exist"));
        assert!(used.is_empty());
    }

    #[test]
    fn advisory_ignore_reads_are_bounded_and_reject_links_and_special_files() {
        let root = tempfile::tempdir().unwrap();
        let ignore = root.path().join(".gitignore");
        let target = root.path().join("ignore-target");
        std::fs::write(root.path().join("Cargo.toml"), "").unwrap();
        std::fs::write(&target, "Cargo.toml\n").unwrap();
        std::os::unix::fs::symlink(&target, &ignore).unwrap();
        assert!(read_project_ignore(&ignore).is_err());
        assert_eq!(
            detect_project_languages(root.path()),
            BTreeSet::from([ToolProfile::Rust])
        );
        std::fs::remove_file(&ignore).unwrap();
        nix::unistd::mkfifo(&ignore, nix::sys::stat::Mode::S_IRUSR).unwrap();
        let started = std::time::Instant::now();
        assert!(read_project_ignore(&ignore).is_err());
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        std::fs::remove_file(&ignore).unwrap();
        std::fs::write(&ignore, vec![b' '; 65536]).unwrap();
        assert!(read_project_ignore(&ignore).is_ok());
        std::fs::write(&ignore, vec![b' '; 65537]).unwrap();
        assert!(read_project_ignore(&ignore).is_err());
        std::fs::write(&ignore, [0xff]).unwrap();
        assert!(read_project_ignore(&ignore).is_err());
        assert_eq!(
            detect_project_languages(root.path()),
            BTreeSet::from([ToolProfile::Rust])
        );
    }

    #[test]
    fn language_available_respects_mounted_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let bin = root.join("usr").join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let interpreter = bin.join("python3");
        std::fs::write(&interpreter, "#!/bin/sh\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&interpreter, std::fs::Permissions::from_mode(0o755)).unwrap();

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
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(language_available(
            ToolProfile::Rust,
            std::slice::from_ref(&bin),
            &[&*root.to_string_lossy()],
        ));
    }
}
