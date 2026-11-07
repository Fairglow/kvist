//! Fixed sandbox-native Rust runtime scripts; callers own validation and grants.

/// Initialize private selection state without copying or making installations writable.
pub const INITIALIZE: &str = "set -eu\nmkdir -p /tmp/rustup-home\ncp /rust/rustup-home/settings.toml /tmp/rustup-home/settings.toml\nln -sf /rust/rustup-home/toolchains /tmp/rustup-home/toolchains\nln -sf /rust/toolchain /rust/toolchains/default\ncargo_home=\"${CARGO_HOME:-/tmp/cargo-home}\"\nmkdir -p \"$cargo_home\"\nif test -d /rust/cargo-registry; then ln -sfn /rust/cargo-registry \"$cargo_home/registry\"; fi\nif test -d /rust/cargo-git; then ln -sfn /rust/cargo-git \"$cargo_home/git\"; fi\n";

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
    render_wrapper(tool, true)
}

/// Render an offline workspace wrapper that allows writable lock resolution.
pub fn workspace_wrapper(tool: RustTool) -> String {
    render_wrapper(tool, false)
}

fn render_wrapper(tool: RustTool, locked: bool) -> String {
    let rustup = if locked {
        "/usr/bin/rustup"
    } else {
        "/rust/rustup"
    };
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
    // Cargo and its dependency-resolving companion force offline resolution.
    // Protected consumers also lock it. Cargo consumes its own flags before
    // delegating to the external clippy subcommand, so the flags must also be
    // injected here or clippy would resolve against the empty scratch registry.
    let arguments = if matches!(tool, RustTool::Cargo | RustTool::CargoClippy) {
        if locked {
            " --offline --locked"
        } else {
            " --offline"
        }
    } else {
        ""
    };
    // cargo delegates an external subcommand as `cargo-clippy clippy <args>`,
    // passing the subcommand name as the first argument. cargo-clippy only
    // recognizes that name in the leading position, so it must be stripped
    // before the injected flags; with no token left, cargo-clippy treats all
    // remaining arguments as cargo-check arguments, which is the form its
    // flag set accepts.
    let subcommand = if tool == RustTool::CargoClippy {
        "if [[ \"${1-}\" == clippy ]]; then shift; fi\n"
    } else {
        ""
    };
    let source_config = if matches!(tool, RustTool::Cargo | RustTool::CargoClippy) {
        "source_config=(); if [[ -f /rust/runtime/vendor-config.toml ]]; then source_config=(--config /rust/runtime/vendor-config.toml); fi\n"
    } else {
        ""
    };
    let source_arguments = if matches!(tool, RustTool::Cargo | RustTool::CargoClippy) {
        " \"${source_config[@]}\""
    } else {
        ""
    };
    let tool = tool.name();
    format!(
        "#!/usr/bin/bash\nset -eu\n{working_directory}if [[ \"${{1-}}\" == +* ]]; then selected=\"${{1:1}}\"; shift; else active=\"$({rustup} show active-toolchain)\"; selected=\"${{active%% *}}\"; fi\n{subcommand}{source_paths}{source_config}exec {rustup} run -- \"$selected\" {tool}{arguments}{source_arguments} \"$@\"\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_cache_is_default_and_administration_is_private() {
        for tool in [RustTool::Cargo, RustTool::CargoClippy] {
            for script in [wrapper(tool), workspace_wrapper(tool)] {
                assert!(!script.contains("replace-with"));
                assert!(!script.contains("/rust/vendor"));
                assert!(script.contains("/rust/runtime/vendor-config.toml"));
            }
        }
        assert!(INITIALIZE.contains("/rust/cargo-registry"));
        assert!(INITIALIZE.contains("/rust/cargo-git"));
        assert!(INITIALIZE.contains("CARGO_HOME"));
    }

    #[test]
    fn workspace_resolution_is_offline_without_an_injected_lock_requirement() {
        for tool in [RustTool::Cargo, RustTool::CargoClippy] {
            let script = workspace_wrapper(tool);
            assert!(script.contains("--offline"));
            assert!(!script.contains("--locked"), "{script}");
        }
    }

    #[test]
    fn runtime_uses_private_settings_readonly_registrations_and_fixed_selection() {
        assert!(
            INITIALIZE
                .contains("cp /rust/rustup-home/settings.toml /tmp/rustup-home/settings.toml")
        );
        assert!(
            INITIALIZE.contains("ln -sf /rust/rustup-home/toolchains /tmp/rustup-home/toolchains")
        );
        for tool in TOOLS {
            let script = wrapper(*tool);
            assert!(script.contains("/usr/bin/rustup show active-toolchain"));
            assert!(script.contains("run -- \"$selected\""));
            assert!(!script.contains("--install"));
            assert!(!script.contains("/home/"));
        }
        assert!(wrapper(RustTool::Cargo).contains("--offline --locked"));
        assert!(!wrapper(RustTool::Cargo).contains("/rust/vendor"));
        assert!(!wrapper(RustTool::Cargo).contains("/workspace/.kvist/vendored"));
        // The clippy companion must force the same vendored resolution as
        // cargo, because cargo does not forward its own flags to the
        // external clippy subcommand it delegates to.
        assert!(wrapper(RustTool::CargoClippy).contains("--offline --locked"));
        // The delegated subcommand name must be stripped before the injected
        // flags, or cargo-clippy forwards it to `cargo check` as a stray
        // positional argument.
        assert!(
            wrapper(RustTool::CargoClippy).contains("if [[ \"${1-}\" == clippy ]]; then shift; fi")
        );
        assert!(!wrapper(RustTool::CargoClippy).contains("/rust/vendor"));
        for tool in [RustTool::Cargo, RustTool::CargoClippy] {
            let script = wrapper(tool);
            assert!(script.contains("[[ -f /rust/runtime/vendor-config.toml ]]"));
            assert!(script.contains("source_config=(--config /rust/runtime/vendor-config.toml)"));
            assert!(script.contains("\"${source_config[@]}\""));
        }
        assert!(!wrapper(RustTool::Rustc).contains("vendor-config.toml"));
        assert!(
            !wrapper(RustTool::CargoClippy)
                .contains("source.crates-io.replace-with=\"vendored-sources\"")
        );
        assert!(!wrapper(RustTool::CargoClippy).contains("/workspace/.kvist/vendored"));
    }
}
