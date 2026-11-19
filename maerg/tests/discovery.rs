use std::fs;

use maerg::{
    discovery::{
        ArtifactStatus, ComponentArtifact, ComponentStatus, InvalidArtifactKind,
        MAX_COMPONENT_DEPTH, discover, discover_with_limits,
    },
    init::initialize,
    project_state::inspect,
};
use tempfile::TempDir;

fn create_component(path: &std::path::Path, artifacts: &[ComponentArtifact]) {
    fs::create_dir_all(path).expect("create component directory");
    for artifact in artifacts {
        fs::write(path.join(artifact.filename()), "fixture").expect("create component artifact");
    }
}

fn initialized_component_root() -> (TempDir, std::path::PathBuf) {
    let project = TempDir::new().expect("create temporary project");
    initialize(project.path()).expect("initialize project");
    let component_root = project.path().join("src");
    (project, component_root)
}

#[test]
fn discovers_complete_nested_components_in_lexical_order() {
    let (_project, component_root) = initialized_component_root();
    create_component(
        &component_root.join("zebra"),
        &[
            ComponentArtifact::Requirements,
            ComponentArtifact::Contract,
            ComponentArtifact::Design,
            ComponentArtifact::TaskQueue,
            ComponentArtifact::ImplementationRecord,
        ],
    );
    create_component(
        &component_root.join("alpha"),
        &[
            ComponentArtifact::Requirements,
            ComponentArtifact::Contract,
            ComponentArtifact::Design,
            ComponentArtifact::TaskQueue,
            ComponentArtifact::ImplementationRecord,
        ],
    );
    fs::create_dir(component_root.join("ordinary-source-directory"))
        .expect("create non-component directory");

    let discovery = discover(&component_root).expect("discover components");

    assert_eq!(
        discovery
            .components
            .iter()
            .map(|component| component.relative_path.as_path())
            .collect::<Vec<_>>(),
        [
            std::path::Path::new("."),
            std::path::Path::new("alpha"),
            std::path::Path::new("zebra")
        ]
    );
    assert!(
        discovery
            .components
            .iter()
            .all(|component| component.status() == ComponentStatus::Complete)
    );
}

#[test]
fn identifies_incomplete_components_without_treating_ordinary_directories_as_components() {
    let (_project, component_root) = initialized_component_root();
    create_component(
        &component_root.join("network"),
        &[ComponentArtifact::Requirements],
    );
    fs::create_dir(component_root.join("ordinary-source-directory"))
        .expect("create non-component directory");

    let discovery = discover(&component_root).expect("discover components");
    let network = discovery
        .components
        .iter()
        .find(|component| component.relative_path == std::path::Path::new("network"))
        .expect("network component");

    assert_eq!(
        network.status(),
        ComponentStatus::Incomplete {
            missing: vec![
                ComponentArtifact::Contract,
                ComponentArtifact::Design,
                ComponentArtifact::TaskQueue,
                ComponentArtifact::ImplementationRecord
            ]
        }
    );
    assert_eq!(discovery.components.len(), 2);
}

#[test]
fn identifies_malformed_artifact_layouts() {
    let (_project, component_root) = initialized_component_root();
    let broken = component_root.join("broken");
    create_component(&broken, &[ComponentArtifact::Requirements]);
    fs::create_dir(broken.join(ComponentArtifact::TaskQueue.filename()))
        .expect("create invalid task queue directory");

    let discovery = discover(&component_root).expect("discover components");
    let component = discovery
        .components
        .iter()
        .find(|component| component.relative_path == std::path::Path::new("broken"))
        .expect("broken component");

    assert_eq!(
        component.status(),
        ComponentStatus::Invalid {
            invalid: vec![ComponentArtifact::TaskQueue]
        }
    );
    assert_eq!(
        component.artifact_status(ComponentArtifact::TaskQueue),
        ArtifactStatus::Invalid(InvalidArtifactKind::Directory)
    );
}

#[test]
fn rejects_component_trees_deeper_than_the_fixed_bound() {
    let project = TempDir::new().expect("create temporary project");
    let component_root = project.path().join("src");
    let mut deepest = component_root.clone();
    fs::create_dir_all(&deepest).expect("create component root");

    for depth in 0..=MAX_COMPONENT_DEPTH {
        deepest.push(format!("level-{depth}"));
        fs::create_dir(&deepest).expect("create nested directory");
    }

    let error = discover(&component_root).expect_err("depth limit must be reported");

    assert!(error.to_string().contains("maximum depth"));
}

#[test]
fn ignores_known_non_component_directories() {
    let (_project, component_root) = initialized_component_root();
    create_component(
        &component_root.join("target/generated"),
        &[ComponentArtifact::Requirements],
    );
    create_component(
        &component_root.join(".git/hooks"),
        &[ComponentArtifact::Requirements],
    );

    let discovery = discover(&component_root).expect("discover components");

    assert_eq!(discovery.components.len(), 1);
}

#[cfg(unix)]
#[test]
fn rejects_link_like_descendants_without_following_them() {
    use std::os::unix::fs::symlink;

    let (_project, component_root) = initialized_component_root();
    symlink(&component_root, component_root.join("cycle")).expect("create cyclic link");

    let error = discover(&component_root).expect_err("link-like descendants must be rejected");

    assert!(error.to_string().contains("link-like component path"));
}

#[cfg(unix)]
#[test]
fn ignores_emacs_lock_files_and_gitignored_paths_during_discovery() {
    use std::os::unix::fs::symlink;

    let (_project, component_root) = initialized_component_root();
    // Simulate an Emacs lock file which is a symlink pointing to an arbitrary string
    symlink(
        "stefan@linux.1234:12345678",
        component_root.join(".#issues.txt"),
    )
    .expect("create emacs lockfile symlink");

    // Also simulate a gitignored file
    fs::write(component_root.join(".gitignore"), "/*.txt\n").expect("write gitignore");
    symlink("nonexistent", component_root.join("ignored.txt")).expect("create ignored symlink");

    let discovery = discover(&component_root).expect("discovery should succeed despite lockfile");
    assert_eq!(discovery.components.len(), 1);
}

#[test]
fn discovers_top_level_peer_components_when_root_is_dot() {
    let workspace = TempDir::new().expect("workspace");
    fs::write(
        workspace.path().join("kvist.toml"),
        "schema_version = 1\ncomponent_root = \".\"\n",
    )
    .expect("write kvist.toml");
    create_component(
        &workspace.path().join("maerg"),
        &[
            ComponentArtifact::Requirements,
            ComponentArtifact::Contract,
            ComponentArtifact::Design,
            ComponentArtifact::TaskQueue,
            ComponentArtifact::ImplementationRecord,
        ],
    );
    create_component(
        &workspace.path().join("sav"),
        &[
            ComponentArtifact::Requirements,
            ComponentArtifact::Contract,
            ComponentArtifact::Design,
            ComponentArtifact::TaskQueue,
            ComponentArtifact::ImplementationRecord,
        ],
    );

    let discovery = maerg::discovery::discover_with_limits(workspace.path(), Default::default())
        .expect("discover peer components");

    assert_eq!(
        discovery
            .components
            .iter()
            .map(|c| c.relative_path.as_path())
            .collect::<Vec<_>>(),
        [std::path::Path::new("maerg"), std::path::Path::new("sav")]
    );
    assert!(
        discovery
            .components
            .iter()
            .all(|c| c.status() == ComponentStatus::Complete)
    );
}

/// Workspace-namespace classification must reuse the already-loaded project
/// configuration instead of re-parsing `kvist.toml` as a raw `toml::Value`.
///
/// `component_root = "./"` is normalized to `.` by `config::load`, so the
/// loaded configuration identifies the project as a workspace root. A raw
/// re-parse, however, compares the literal string `"./"` against `"."` and
/// concludes it is *not* a workspace root. That divergence must disappear:
/// classification must follow the loaded configuration, so the component root
/// is not emitted as an extra root component.
#[test]
fn workspace_namespace_classification_reuses_loaded_configuration() {
    let project = TempDir::new().expect("create temporary project");
    fs::write(
        project.path().join("kvist.toml"),
        "schema_version = 1\ncomponent_root = \"./\"\n",
    )
    .expect("write workspace kvist.toml");
    create_component(
        &project.path().join("alpha"),
        &[
            ComponentArtifact::Requirements,
            ComponentArtifact::Contract,
            ComponentArtifact::Design,
            ComponentArtifact::TaskQueue,
            ComponentArtifact::ImplementationRecord,
        ],
    );

    let discovery =
        discover_with_limits(project.path(), Default::default()).expect("discover components");

    // The loaded configuration normalizes `./` to `.`, a workspace root, so the
    // component root itself must not be emitted as a root component.
    assert!(
        !discovery
            .components
            .iter()
            .any(|component| component.relative_path == std::path::Path::new(".")),
        "classification must reuse the loaded configuration (component_root `./` normalizes to `.`); \
         the component root must not be emitted as a root component: {:?}",
        discovery
            .components
            .iter()
            .map(|component| component.relative_path.as_path())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        discovery
            .components
            .iter()
            .map(|component| component.relative_path.as_path())
            .collect::<Vec<_>>(),
        [std::path::Path::new("alpha")],
        "only the nested component must be discovered"
    );
}

/// A nested subproject that carries its own `kvist.toml` must not be
/// misclassified as a workspace namespace root. Discovery must treat it as a
/// regular component, not suppress it as a second workspace root.
#[test]
fn nested_subproject_is_not_misclassified_as_workspace_root() {
    let project = TempDir::new().expect("create temporary project");
    fs::write(
        project.path().join("kvist.toml"),
        "schema_version = 1\ncomponent_root = \".\"\n",
    )
    .expect("write workspace kvist.toml");
    let subproject = project.path().join("subproject");
    fs::create_dir_all(&subproject).expect("create subproject directory");
    fs::write(
        subproject.join("kvist.toml"),
        "schema_version = 1\ncomponent_root = \".\"\n",
    )
    .expect("write subproject kvist.toml");
    create_component(
        &subproject,
        &[
            ComponentArtifact::Requirements,
            ComponentArtifact::Contract,
            ComponentArtifact::Design,
            ComponentArtifact::TaskQueue,
            ComponentArtifact::ImplementationRecord,
        ],
    );

    let discovery =
        discover_with_limits(project.path(), Default::default()).expect("discover components");

    // The subproject must be discovered as a regular component, not suppressed
    // as a misclassified workspace namespace root.
    let subproject_component = discovery
        .components
        .iter()
        .find(|component| component.relative_path == std::path::Path::new("subproject"))
        .expect("nested subproject must be discovered as a component");
    assert_eq!(
        subproject_component.status(),
        ComponentStatus::Complete,
        "nested subproject must be a complete component, not a misclassified workspace root"
    );
}

/// When `component_root` is the repository root, the artifact projection's
/// second discovery must surface errors instead of silently continuing.
///
/// Today `artifacts_for_project` swallows the discovery error (via `if let
/// Ok`), so `inspect` returns `Ok` with a silently truncated artifact set. The
/// projection must instead fail loudly: a link-like path at the project root
/// must surface as an error rather than being swallowed.
#[cfg(unix)]
#[test]
fn artifact_projection_surfaces_discovery_errors() {
    use std::os::unix::fs::symlink;

    let project = TempDir::new().expect("create temporary project");
    fs::write(
        project.path().join("kvist.toml"),
        "schema_version = 1\ncomponent_root = \".\"\n",
    )
    .expect("write workspace kvist.toml");
    // A link-like path at the project root that discovery must reject.
    symlink(project.path(), project.path().join("cycle")).expect("create cyclic link");

    // The projection must surface the discovery error, not silently continue.
    let inspection = inspect(project.path()).expect_err("discovery error must not be swallowed");
    assert!(
        inspection.to_string().contains("link-like"),
        "the surfaced error must be the link-like path error: {inspection}"
    );
}

/// Workspace-namespace classification must not be coupled to the validity of
/// unrelated configuration sections.
///
/// A project whose `component_root` is `.` is a workspace root even when
/// another section (here `test_policy`) is malformed. Discovery must still
/// classify the root as a workspace root and must not emit the component root
/// as a root component. Full configuration validation is the caller's
/// responsibility; classification reads only `component_root`.
#[test]
fn workspace_classification_is_decoupled_from_unrelated_config_sections() {
    let project = TempDir::new().expect("create temporary project");
    // A valid `component_root = "."` alongside a malformed `test_policy`
    // section (timeout of 0 is rejected by full configuration validation).
    fs::write(
        project.path().join("kvist.toml"),
        "schema_version = 1\ncomponent_root = \".\"\n[test_policy]\nschema_version = 1\nworking_directory = \"component\"\nenvironment_allowlist = []\ntimeout_seconds = 0\nmax_output_bytes = 1\ncommands = []\n",
    )
    .expect("write workspace kvist.toml");
    create_component(
        &project.path().join("alpha"),
        &[
            ComponentArtifact::Requirements,
            ComponentArtifact::Contract,
            ComponentArtifact::Design,
            ComponentArtifact::TaskQueue,
            ComponentArtifact::ImplementationRecord,
        ],
    );

    let discovery = discover_with_limits(project.path(), Default::default())
        .expect("discover components despite malformed unrelated section");

    // The root must still be classified as a workspace root: the component
    // root itself must not be emitted as a root component.
    assert!(
        !discovery
            .components
            .iter()
            .any(|component| component.relative_path == std::path::Path::new(".")),
        "classification must read only component_root and must not be coupled to \
         the validity of unrelated sections; the component root must not be \
         emitted as a root component: {:?}",
        discovery
            .components
            .iter()
            .map(|component| component.relative_path.as_path())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        discovery
            .components
            .iter()
            .map(|component| component.relative_path.as_path())
            .collect::<Vec<_>>(),
        [std::path::Path::new("alpha")],
        "only the nested component must be discovered"
    );
}
