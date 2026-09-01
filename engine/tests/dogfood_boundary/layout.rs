use std::{fs, path::Path};

use crate::support::repository_root;

const COMPONENT_ARTIFACTS: [&str; 5] = [
    "REQUIREMENTS.md",
    "CONTRACT.md",
    "DESIGN.md",
    "TODOS.yaml",
    "IMPL.md",
];

#[test]
fn target_workspace_is_owned_by_the_engine_component() {
    let root = repository_root();
    let engine = root.join("engine");
    let required = [
        "Cargo.toml",
        "Cargo.lock",
        "src",
        "tests",
        "REQUIREMENTS.md",
        "CONTRACT.md",
        "DESIGN.md",
        "TODOS.yaml",
        "IMPL.md",
    ];
    let missing = required
        .iter()
        .filter(|path| !engine.join(path).exists())
        .copied()
        .collect::<Vec<_>>();

    assert!(
        missing.is_empty(),
        "engine/ must own the Rust workspace and root artifacts; missing: {missing:?}"
    );
    let manifest: toml::Value = toml::from_str(
        &fs::read_to_string(engine.join("Cargo.toml")).expect("read engine manifest"),
    )
    .expect("parse engine Cargo.toml");
    let members = manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("members"))
        .and_then(toml::Value::as_array)
        .expect("engine Cargo.toml workspace.members")
        .iter()
        .map(|member| member.as_str().expect("workspace member must be a string"))
        .collect::<Vec<_>>();
    assert!(
        members.contains(&"agent_runtime") && members.contains(&"sandbox_runner"),
        "engine/Cargo.toml must structurally declare both child components; members: {members:?}"
    );
}

#[test]
fn runtime_and_runner_are_complete_child_components() {
    let engine = repository_root().join("engine");
    for child in ["agent_runtime", "sandbox_runner"] {
        let child = engine.join(child);
        let missing = COMPONENT_ARTIFACTS
            .iter()
            .chain(["Cargo.toml", "src", "tests"].iter())
            .filter(|path| !child.join(path).exists())
            .copied()
            .collect::<Vec<_>>();
        assert!(
            missing.is_empty(),
            "{} must be a complete buildable child component; missing: {missing:?}",
            child.display()
        );
    }
}

#[test]
fn retired_workspace_and_component_paths_have_no_aliases() {
    let root = repository_root();
    for retired in ["Cargo.toml", "Cargo.lock", "src", "tests"] {
        assert!(
            !root.join(retired).exists(),
            "retired repository-root path `{retired}` must be removed, not retained as an alias"
        );
    }
    for retired_child in ["engine/src/agent_runtime", "engine/src/sandbox_runner"] {
        assert!(
            !Path::new(&root).join(retired_child).exists(),
            "retired child path `{retired_child}` must not remain as an alias"
        );
    }
    let config: toml::Value =
        toml::from_str(&fs::read_to_string(root.join("kvist.toml")).expect("read kvist.toml"))
            .expect("parse kvist.toml");
    assert_eq!(
        config.get("component_root").and_then(toml::Value::as_str),
        Some("engine"),
        "the structurally parsed component root must be engine/"
    );
}
