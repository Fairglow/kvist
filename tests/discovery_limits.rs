use std::{fs, path::Path, process::Command};

use kvist::{
    config::{self, DiscoveryLimits, MAX_DISCOVERY_LIMITS, VcsSelection},
    discovery::{ComponentArtifact, discover_with_limits},
    init::initialize,
    project_state::{ProjectState, inspect},
    tree::render_project,
};
use tempfile::TempDir;

fn limits() -> DiscoveryLimits {
    DiscoveryLimits {
        max_depth: 64,
        max_directories: 100,
        max_components: 100,
        max_entries_per_directory: 100,
        max_relative_path_bytes: 1_000,
    }
}

fn create_component(path: &Path) {
    fs::create_dir_all(path).expect("create component");
    fs::write(
        path.join(ComponentArtifact::Specification.filename()),
        "fixture",
    )
    .expect("write artifact");
}

#[test]
fn discovery_reports_each_resource_limit_specifically() {
    let workspace = TempDir::new().expect("workspace");

    let depth = workspace.path().join("depth");
    fs::create_dir_all(depth.join("one/two")).expect("create nesting");
    assert!(
        discover_with_limits(
            &depth,
            DiscoveryLimits {
                max_depth: 1,
                ..limits()
            }
        )
        .expect_err("depth")
        .to_string()
        .contains("maximum depth of 1")
    );

    let directories = workspace.path().join("directories");
    fs::create_dir_all(directories.join("child")).expect("create child");
    assert!(
        discover_with_limits(
            &directories,
            DiscoveryLimits {
                max_directories: 1,
                ..limits()
            }
        )
        .expect_err("directory limit")
        .to_string()
        .contains("maximum of 1 scanned directories")
    );

    let components = workspace.path().join("components");
    create_component(&components);
    create_component(&components.join("child"));
    assert!(
        discover_with_limits(
            &components,
            DiscoveryLimits {
                max_components: 1,
                ..limits()
            }
        )
        .expect_err("component limit")
        .to_string()
        .contains("maximum of 1 recognized components")
    );

    let entries = workspace.path().join("entries");
    fs::create_dir_all(&entries).expect("create root");
    fs::write(entries.join("one"), "").expect("entry");
    fs::write(entries.join("two"), "").expect("entry");
    assert!(
        discover_with_limits(
            &entries,
            DiscoveryLimits {
                max_entries_per_directory: 1,
                ..limits()
            }
        )
        .expect_err("entry limit")
        .to_string()
        .contains("maximum of 1 entries")
    );

    let paths = workspace.path().join("paths");
    fs::create_dir_all(paths.join("long")).expect("create long path");
    assert!(
        discover_with_limits(
            &paths,
            DiscoveryLimits {
                max_relative_path_bytes: 1,
                ..limits()
            }
        )
        .expect_err("path limit")
        .to_string()
        .contains("maximum relative path length of 1 encoded bytes")
    );
}

#[test]
fn discovery_rejects_components_below_ordinary_directories() {
    let workspace = TempDir::new().expect("workspace");
    let root = workspace.path().join("src");
    create_component(&root.join("ordinary/component"));

    let error = discover_with_limits(&root, limits()).expect_err("hierarchy violation");

    assert!(
        error
            .to_string()
            .contains("below ordinary directory `ordinary`")
    );
}

#[test]
fn discovery_configuration_defaults_and_invalid_ranges_are_enforced() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");

    let default = config::load(project.path()).expect("default config");
    assert_eq!(default.discovery, DiscoveryLimits::default());
    assert_eq!(default.vcs, VcsSelection::Auto);

    for discovery in [
        "[discovery]\nmax_depth = 0\n",
        "[discovery]\nmax_directories = \"many\"\n",
        "[vcs]\nkind = \"unsupported\"\n",
        &format!(
            "[discovery]\nmax_relative_path_bytes = {}\n",
            MAX_DISCOVERY_LIMITS.max_relative_path_bytes + 1
        ),
    ] {
        fs::write(
            project.path().join("kvist.toml"),
            format!("schema_version = 1\ncomponent_root = \"src\"\n{discovery}"),
        )
        .expect("write config");
        let error = config::load(project.path()).expect_err("invalid configuration");
        assert!(
            error
                .to_string()
                .contains("invalid Kvist project configuration")
        );
    }
}

#[test]
fn invalid_discovery_configuration_propagates_to_tree_doctor_and_init() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(
        project.path().join("kvist.toml"),
        format!(
            "schema_version = 1\ncomponent_root = \"src\"\n[discovery]\nmax_depth = {}\n",
            MAX_DISCOVERY_LIMITS.max_depth + 1
        ),
    )
    .expect("write config");

    let tree_error = render_project(project.path()).expect_err("tree rejects invalid config");
    assert!(tree_error.to_string().contains("hard maximum"));
    assert_eq!(
        inspect(project.path()).expect("doctor inspection").state,
        ProjectState::Invalid
    );
    assert!(
        initialize(project.path())
            .expect_err("init rejects invalid configuration")
            .to_string()
            .contains("state is invalid")
    );

    let output = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args(["doctor", project.path().to_str().expect("UTF-8 path")])
        .output()
        .expect("run doctor");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("state: invalid"));
}

#[test]
fn tree_uses_configured_discovery_limits() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    create_component(&project.path().join("src/child"));
    fs::write(
        project.path().join("kvist.toml"),
        "schema_version = 1\ncomponent_root = \"src\"\n[discovery]\nmax_components = 1\n",
    )
    .expect("write config");

    let error = render_project(project.path()).expect_err("configured component limit");
    assert!(
        error
            .to_string()
            .contains("maximum of 1 recognized components")
    );
}

#[cfg(unix)]
#[test]
fn discovery_rejects_unix_directory_links() {
    use std::os::unix::fs::symlink;

    let workspace = TempDir::new().expect("workspace");
    let root = workspace.path().join("src");
    fs::create_dir_all(&root).expect("root");
    symlink(&root, root.join("cycle")).expect("symlink");

    assert!(
        discover_with_limits(&root, limits())
            .expect_err("link-like path")
            .to_string()
            .contains("link-like component path")
    );
}

#[test]
fn agent_configuration_loading_is_supported_and_validated() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");

    // 1. Loading with default agent configurations
    let config = config::load(project.path()).expect("default agent config load");
    assert_eq!(
        config.agent.architect.command_template,
        "claude --non-interactive --dangerously-skip-permissions --message '{prompt}' {context_files}"
    );
    assert_eq!(config.agent.architect.token_limit, None);
    assert_eq!(
        config.agent.developer.command_template,
        "gemini-cli --prompt '{prompt}' --files {context_files}"
    );
    assert_eq!(config.agent.developer.token_limit, None);

    // 2. Custom valid agent configurations
    let custom_toml = r#"schema_version = 1
component_root = "src"
[agent.profiles.architect]
command_template = "custom-architect --prompt '{prompt}' {context_files}"
token_limit = 25000

[agent.profiles.developer]
command_template = "custom-developer --files {context_files} '{prompt}'"
token_limit = 10000
"#;
    fs::write(project.path().join("kvist.toml"), custom_toml).expect("write custom config");
    let config = config::load(project.path()).expect("custom agent config load");
    assert_eq!(
        config.agent.architect.command_template,
        "custom-architect --prompt '{prompt}' {context_files}"
    );
    assert_eq!(config.agent.architect.token_limit, Some(25000));
    assert_eq!(
        config.agent.developer.command_template,
        "custom-developer --files {context_files} '{prompt}'"
    );
    assert_eq!(config.agent.developer.token_limit, Some(10000));

    // 2b. Project-local `.kvist/config.toml` overrides root `kvist.toml`
    // Reset kvist.toml to not contain any agent block
    fs::write(
        project.path().join("kvist.toml"),
        "schema_version = 1\ncomponent_root = \"src\"\n",
    )
    .expect("reset config");
    let local_dir = project.path().join(".kvist");
    fs::create_dir_all(&local_dir).expect("create .kvist dir");
    let local_config_toml = r#"[agent.profiles.architect]
command_template = "local-override-architect '{prompt}'"
token_limit = 500
[agent.profiles.developer]
command_template = "local-override-developer"
"#;
    fs::write(local_dir.join("config.toml"), local_config_toml).expect("write local config");
    let config = config::load(project.path()).expect("local override config load");
    assert_eq!(
        config.agent.architect.command_template,
        "local-override-architect '{prompt}'"
    );
    assert_eq!(config.agent.architect.token_limit, Some(500));
    assert_eq!(
        config.agent.developer.command_template,
        "local-override-developer"
    );
    assert_eq!(config.agent.developer.token_limit, None);

    // 3. Invalid agent configurations (e.g. invalid types)
    for invalid in [
        "[agent]\nprofiles = \"not-a-table\"\n",
        "[agent.profiles]\narchitect = \"not-a-table\"\n",
        "[agent.profiles.architect]\ncommand_template = 12345\n",
        "[agent.profiles.architect]\ntoken_limit = \"not-an-integer\"\n",
        "[agent.profiles.architect]\ntoken_limit = 0\n",
        "[agent.profiles.architect]\ntoken_limit = -5\n",
    ] {
        fs::write(
            project.path().join("kvist.toml"),
            format!("schema_version = 1\ncomponent_root = \"src\"\n{invalid}"),
        )
        .expect("write invalid config");
        match config::load(project.path()) {
            Ok(cfg) => panic!(
                "Expected error for configuration:\n{invalid}\nBut got successful parse: {cfg:?}"
            ),
            Err(error) => {
                assert!(
                    error
                        .to_string()
                        .contains("invalid Kvist project configuration"),
                    "Expected invalid config error but got: {error}"
                );
            }
        }
    }
}

#[test]
fn sandbox_configuration_requires_explicit_deny_network_component_mount_and_environment() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    let base = "schema_version = 1\ncomponent_root = \"src\"\n";
    let valid = r#"[sandbox]
    schema_version = 1
    runner = "/trusted/sandbox-runner"
    network = "deny"
    environment_allowlist = ["PATH"]
    mount = "component"
    "#;
    fs::write(project.path().join("kvist.toml"), format!("{base}{valid}")).expect("write config");
    let loaded = config::load(project.path()).expect("load sandbox");
    assert_eq!(
        loaded.sandbox.expect("sandbox").environment_allowlist,
        vec!["PATH"]
    );

    for invalid in [
        "[sandbox]\nschema_version = 1\nrunner = \"runner\"\nnetwork = \"deny\"\nenvironment_allowlist = []\nmount = \"component\"\n",
        "[sandbox]\nschema_version = 1\nrunner = \"runner\"\nnetwork = \"allow\"\nenvironment_allowlist = []\nmount = \"component\"\n",
        "[sandbox]\nschema_version = 1\nrunner = \"runner\"\nnetwork = \"deny\"\nmount = \"component\"\n",
        "[sandbox]\nschema_version = 1\nrunner = \"runner\"\nnetwork = \"deny\"\nenvironment_allowlist = []\nmount = \"project\"\n",
    ] {
        fs::write(
            project.path().join("kvist.toml"),
            format!("{base}{invalid}"),
        )
        .expect("write invalid config");
        assert!(config::load(project.path()).is_err(), "{invalid}");
    }
}
