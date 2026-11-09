use crate::{Result, fs::Dir, unavailable};
use serde::Deserialize;
use std::path::Path;

const CONFIG_BYTES: u64 = 1024 * 1024;

/// Project-wide vendor quotas from optional `.kvist/vendor.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VendorLimits {
    /// Maximum locked package count.
    pub max_packages: usize,
    /// Maximum generated source-replacement tables.
    pub max_sources: usize,
    /// Maximum package-tree entries.
    pub max_entries: usize,
    /// Maximum aggregate regular-file bytes.
    pub max_total_bytes: u64,
    /// Maximum individual regular-file bytes.
    pub max_file_bytes: u64,
    /// Maximum aggregate relative-path bytes.
    pub max_path_bytes: usize,
    /// Maximum package-tree directory depth.
    pub max_depth: usize,
    /// Maximum Cargo.lock bytes.
    pub max_lockfile_bytes: u64,
    /// Maximum package/control metadata bytes.
    pub max_metadata_bytes: u64,
    /// Maximum combined acquisition output bytes.
    pub max_output_bytes: usize,
    /// Maximum acquisition wall-clock and idle seconds.
    pub timeout_secs: u64,
}

impl Default for VendorLimits {
    fn default() -> Self {
        Self {
            max_packages: 100_000,
            max_sources: 1024,
            max_entries: 100_000,
            max_total_bytes: 1024 * 1024 * 1024,
            max_file_bytes: 256 * 1024 * 1024,
            max_path_bytes: 32 * 1024 * 1024,
            max_depth: 64,
            max_lockfile_bytes: 16 * 1024 * 1024,
            max_metadata_bytes: 16 * 1024 * 1024,
            max_output_bytes: 1024 * 1024,
            timeout_secs: 900,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    #[serde(default)]
    limits: VendorLimits,
}

impl VendorLimits {
    /// Load bounded, non-link project policy; omissions use defaults.
    pub fn load(project: &Path) -> Result<Self> {
        let project = Dir::open(project)?;
        if !project.exists(".kvist")? {
            return Ok(Self::default());
        }
        let state = project.child(".kvist")?;
        if !state.exists("vendor.toml")? {
            return Ok(Self::default());
        }
        let path = state.display.join("vendor.toml");
        let (bytes, _) = state.read("vendor.toml", CONFIG_BYTES)?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| unavailable(&path, "vendor policy is not UTF-8"))?;
        let config: Config = toml::from_str(text).map_err(|_| {
            unavailable(
                &path,
                "invalid vendor policy; use known [limits] fields and positive integers",
            )
        })?;
        config.limits.validate(&path)?;
        Ok(config.limits)
    }

    pub(crate) fn validate(&self, path: &Path) -> Result<()> {
        if self.max_packages == 0
            || self.max_sources == 0
            || self.max_entries == 0
            || self.max_path_bytes == 0
            || self.max_depth == 0
            || self.max_output_bytes == 0
            || self.max_total_bytes == 0
            || self.max_file_bytes == 0
            || self.max_lockfile_bytes == 0
            || self.max_metadata_bytes == 0
            || self.timeout_secs == 0
        {
            return Err(unavailable(path, "vendor limits must be positive integers"));
        }
        Ok(())
    }
}
