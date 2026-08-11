use std::fs;

use kvist::{
    discovery::{
        ArtifactStatus, ComponentArtifact, ComponentStatus, InvalidArtifactKind,
        MAX_COMPONENT_DEPTH, discover,
    },
    init::initialize,
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
            ComponentArtifact::Specification,
            ComponentArtifact::TaskQueue,
            ComponentArtifact::Documentation,
        ],
    );
    create_component(
        &component_root.join("alpha"),
        &[
            ComponentArtifact::Specification,
            ComponentArtifact::TaskQueue,
            ComponentArtifact::Documentation,
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
        &[ComponentArtifact::Specification],
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
                ComponentArtifact::TaskQueue,
                ComponentArtifact::Documentation
            ]
        }
    );
    assert_eq!(discovery.components.len(), 2);
}

#[test]
fn identifies_malformed_artifact_layouts() {
    let (_project, component_root) = initialized_component_root();
    let broken = component_root.join("broken");
    create_component(&broken, &[ComponentArtifact::Specification]);
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
        &[ComponentArtifact::Specification],
    );
    create_component(
        &component_root.join(".git/hooks"),
        &[ComponentArtifact::Specification],
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
