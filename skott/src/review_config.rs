//! Advisory review configuration.
//!
//! Review is default-on for all components. A project may explicitly opt out
//! by setting `required = false` in the `[review]` section of `kvist.toml`.
//! When review is required but no agent is configured, an explicit exception
//! must be recorded; absence is never treated as a pass.

use serde::{Deserialize, Serialize};

/// Default: review is required.
pub const DEFAULT_REVIEW_REQUIRED: bool = true;

/// Review configuration loaded from `kvist.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewConfig {
    /// Whether advisory document review is required before target acceptance.
    /// Defaults to `true` when the `[review]` section or field is absent.
    #[serde(default = "default_required")]
    pub required: bool,
}

fn default_required() -> bool {
    DEFAULT_REVIEW_REQUIRED
}

impl Default for ReviewConfig {
    fn default() -> Self {
        Self {
            required: DEFAULT_REVIEW_REQUIRED,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_config_defaults_to_required() {
        let config = ReviewConfig::default();
        assert!(config.required);
    }

    #[test]
    fn review_config_serialization() {
        let config = ReviewConfig { required: false };
        let serialized = toml::to_string(&config).unwrap();
        assert!(serialized.contains("required = false"));

        let parsed: ReviewConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(parsed, config);
    }

    #[test]
    fn review_config_unknown_field_rejected() {
        let input = "required = true\nunknown = 42";
        let result: Result<ReviewConfig, _> = toml::from_str(input);
        assert!(result.is_err());
    }
}
