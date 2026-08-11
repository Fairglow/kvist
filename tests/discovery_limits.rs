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
