use std::fs;

use kvist::config::{self, Role};

#[test]
fn role_profile_is_shared_structured_pinned_and_digest_bound() {
    let directory = tempfile::tempdir_in(".").unwrap();
    let root = directory.path().canonicalize().unwrap();
    let registry = root.join("providers.toml");
    fs::write(&registry, "schema_version = 1\n[profiles.coder.backend]\nkind = \"native\"\nprovider = \"ollama\"\nmodel = \"shared-model\"\n").unwrap();
    fs::write(root.join("kvist.toml"), "schema_version = 1\ncomponent_root = \"src\"\n[agent.registry]\npath = \"providers.toml\"\n[agent.roles.developer]\nprofile = \"coder\"\n").unwrap();
    let loaded = config::load(&root).unwrap();
    let resolved = loaded.agent.resolve_role_profile(Role::Developer).unwrap();
    assert_eq!(resolved.native().unwrap().model, "shared-model");
    assert_eq!(
        loaded.agent.developer.resolved_profile.as_ref(),
        Some(resolved)
    );
    let digest = loaded.agent.source.digest.clone();
    fs::write(
        &registry,
        fs::read_to_string(&registry)
            .unwrap()
            .replace("shared-model", "changed"),
    )
    .unwrap();
    assert_eq!(
        loaded
            .agent
            .resolve_named_profile("coder")
            .unwrap()
            .native()
            .unwrap()
            .model,
        "shared-model"
    );
    assert_ne!(config::load(&root).unwrap().agent.source.digest, digest);
}
