//! Fixed sandbox-native Rust runtime scripts; callers own validation and grants.

/// Initialize private selection state without copying or making installations writable.
pub const INITIALIZE: &str = "set -eu\nmkdir -p /tmp/rustup-home\ncp /rust/rustup-home/settings.toml /tmp/rustup-home/settings.toml\nln -s /rust/rustup-home/toolchains /tmp/rustup-home/toolchains\n";

/// Closed runtime executable set, including optional installed companions.
pub const TOOLS: &[RustTool] = &[
    RustTool::Cargo,
    RustTool::Rustc,
    RustTool::Rustdoc,
    RustTool::Rustfmt,
    RustTool::CargoFmt,
    RustTool::CargoClippy,
    RustTool::ClippyDriver,
    RustTool::RustAnalyzer,
];

/// A trusted executable name that can be interpolated into a fixed runtime script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustTool {
    /// Cargo package/build driver.
    Cargo,
    /// Rust compiler.
    Rustc,
    /// Rust documentation compiler.
    Rustdoc,
    /// Rust formatter.
    Rustfmt,
    /// Cargo formatting companion.
    CargoFmt,
    /// Cargo Clippy companion.
    CargoClippy,
    /// Clippy compiler driver.
    ClippyDriver,
    /// Rust analyzer.
    RustAnalyzer,
}

impl RustTool {
    /// The fixed executable basename.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Rustc => "rustc",
            Self::Rustdoc => "rustdoc",
            Self::Rustfmt => "rustfmt",
            Self::CargoFmt => "cargo-fmt",
            Self::CargoClippy => "cargo-clippy",
            Self::ClippyDriver => "clippy-driver",
            Self::RustAnalyzer => "rust-analyzer",
        }
    }
}

/// Render a selection-aware wrapper for a trusted tool.
pub fn wrapper(tool: RustTool) -> String {
    let working_directory = if tool == RustTool::Cargo {
        "if [[ -n \"${KVIST_CARGO_WORKDIR-}\" ]]; then cd \"$KVIST_CARGO_WORKDIR\"; fi\n"
    } else {
        ""
    };
    let source_paths = if matches!(
        tool,
        RustTool::Rustc | RustTool::Rustdoc | RustTool::Rustfmt | RustTool::ClippyDriver
    ) {
        "if [[ -n \"${KVIST_COMPONENT_BUILD_DIR-}\" ]]; then args=(); for arg in \"$@\"; do if [[ \"$arg\" == \"$KVIST_COMPONENT_BUILD_DIR/src/\"* || \"$arg\" == \"$KVIST_COMPONENT_BUILD_DIR/tests/\"* ]]; then arg=\"/workspace/component/${arg#\"$KVIST_COMPONENT_BUILD_DIR/\"}\"; elif [[ \"$PWD\" == \"$KVIST_COMPONENT_BUILD_DIR\" && ( \"$arg\" == src/* || \"$arg\" == tests/* ) ]]; then arg=\"/workspace/component/$arg\"; fi; args+=(\"$arg\"); done; set -- \"${args[@]}\"; fi\n"
    } else {
        ""
    };
    let arguments = if tool == RustTool::Cargo {
        " --offline --locked --config 'source.crates-io.replace-with=\"vendored-sources\"' --config 'source.vendored-sources.directory=\"/rust/vendor\"'"
    } else {
        ""
    };
    let tool = tool.name();
    format!(
        "#!/usr/bin/bash\nset -eu\n{working_directory}if [[ \"${{1-}}\" == +* ]]; then selected=\"${{1:1}}\"; shift; else active=\"$(/usr/bin/rustup show active-toolchain)\"; selected=\"${{active%% *}}\"; fi\n{source_paths}exec /usr/bin/rustup run -- \"$selected\" {tool}{arguments} \"$@\"\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_uses_private_settings_readonly_registrations_and_fixed_selection() {
        assert!(
            INITIALIZE
                .contains("cp /rust/rustup-home/settings.toml /tmp/rustup-home/settings.toml")
        );
        assert!(
            INITIALIZE.contains("ln -s /rust/rustup-home/toolchains /tmp/rustup-home/toolchains")
        );
        for tool in TOOLS {
            let script = wrapper(*tool);
            assert!(script.contains("/usr/bin/rustup show active-toolchain"));
            assert!(script.contains("run -- \"$selected\""));
            assert!(!script.contains("--install"));
            assert!(!script.contains("/home/"));
        }
        assert!(wrapper(RustTool::Cargo).contains("--offline --locked"));
        assert!(wrapper(RustTool::Cargo).contains("/rust/vendor"));
    }
}
