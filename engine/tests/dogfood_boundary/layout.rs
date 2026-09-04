use std::fs;

use crate::support::repository_root;

const COMPONENT_ARTIFACTS: [&str; 5] = [
    "REQUIREMENTS.md",
    "CONTRACT.md",
    "DESIGN.md",
    "TODOS.yaml",
    "IMPL.md",
];

#[test]
fn workspace_is_owned_by_the_repository_root() {
    let root = repository_root();
    let required = [
        "Cargo.toml",
        "Cargo.lock",
        "VISION.md",
        "ARCHITECTURE.md",
        "ROOT_CONTRACT.md",
        "kvist.toml",
    ];
    let missing = required
        .iter()
        .filter(|path| !root.join(path).exists())
        .copied()
        .collect::<Vec<_>>();

    assert!(
        missing.is_empty(),
        "repository root must own the workspace manifest, lockfile, and root intent; missing: {missing:?}"
    );
    let manifest: toml::Value =
        toml::from_str(&fs::read_to_string(root.join("Cargo.toml")).expect("read root manifest"))
            .expect("parse root Cargo.toml");
    let members = manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("members"))
        .and_then(toml::Value::as_array)
        .expect("root Cargo.toml workspace.members")
        .iter()
        .map(|member| member.as_str().expect("workspace member must be a string"))
        .collect::<Vec<_>>();
    assert!(
        members.contains(&"engine")
            && members.contains(&"agent_runtime")
            && members.contains(&"sandbox_runner"),
        "root Cargo.toml must declare engine, agent_runtime, and sandbox_runner components; members: {members:?}"
    );
}

#[test]
fn top_level_components_are_complete() {
    let root = repository_root();
    for component in ["engine", "agent_runtime", "sandbox_runner"] {
        let dir = root.join(component);
        let missing = COMPONENT_ARTIFACTS
            .iter()
            .chain(["Cargo.toml", "src", "tests"].iter())
            .filter(|path| !dir.join(path).exists())
            .copied()
            .collect::<Vec<_>>();
        assert!(
            missing.is_empty(),
            "{} must be a complete buildable component; missing: {missing:?}",
            dir.display()
        );
    }
}

#[test]
fn retired_nested_paths_have_no_aliases() {
    let root = repository_root();
    for retired_nested in [
        "engine/agent_runtime",
        "engine/sandbox_runner",
        "engine/src/agent_runtime",
        "engine/src/sandbox_runner",
        "engine/Cargo.lock",
    ] {
        assert!(
            !root.join(retired_nested).exists(),
            "retired nested path `{retired_nested}` must not remain as an alias"
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
