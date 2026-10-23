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

const SHARED_MODELS: &str = r#"schema_version = 1
[profiles.coder.backend]
kind = "native"
provider = "ollama"
model = "native-coder"
[profiles.fallback.backend]
kind = "native"
provider = "llama-server"
model = "native-fallback"
[profiles.external.backend]
kind = "external-agent"
provider = "copilot"
executable = "/not/executed"
model = "remote-model"
"#;

fn shared_binding(registry: &str) -> String {
    format!(
        "[agent.registry]\npath = \"{registry}\"\n\
         [agent.roles.architect]\nprofile = \"coder\"\n\
         [agent.roles.developer]\nprofile = \"fallback\"\n\
         [agent.roles.security-reviewer]\nprofile = \"coder\"\n"
    )
}

fn legacy_binding(name: &str, model: &str) -> String {
    format!(
        "[agent.profiles.{name}]\nprovider = \"custom\"\nmodel = \"{model}\"\n\
         command = \"/not/executed {{prompt}}\"\n"
    )
}

fn list_fixture() -> tempfile::TempDir {
    let directory = tempfile::tempdir_in(".").unwrap();
    fs::write(
        directory.path().join("kvist.toml"),
        "schema_version = 1\ncomponent_root = \"src\"\n",
    )
    .unwrap();
    directory
}

fn agent_list(root: &std::path::Path) -> String {
    let root = root.canonicalize().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args(["agent", "list"])
        .current_dir(&root)
        .env("HOME", &root)
        .env("XDG_CONFIG_HOME", root.join("user-config"))
        .env("KVIST_CONFIG_PATH", root.join("user.toml"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn shared_agent_list_renders_one_authoritative_catalogue_and_scoped_role_bindings() {
    for global_shared in [false, true] {
        let directory = list_fixture();
        let root = directory.path().canonicalize().unwrap();
        let config_path = root.join("kvist.toml");
        let registry_path = root.join("providers.toml");
        fs::write(&registry_path, SHARED_MODELS).unwrap();
        let config = "schema_version = 1\ncomponent_root = \"src\"\n".to_owned()
            + &shared_binding("providers.toml");
        fs::write(&config_path, &config).unwrap();
        if global_shared {
            fs::write(
                root.join("user-providers.toml"),
                SHARED_MODELS.replace("native-coder", "global-model")
                    + "\n[profiles.global-unrelated.backend]\nkind = \"native\"\n\
                       provider = \"ollama\"\nmodel = \"global-model\"\n",
            )
            .unwrap();
            fs::write(
                root.join("user.toml"),
                shared_binding("user-providers.toml"),
            )
            .unwrap();
        } else {
            fs::write(
                root.join("user.toml"),
                legacy_binding("global-unrelated", "global-model"),
            )
            .unwrap();
        }
        let listing = agent_list(&root);
        for id in ["coder", "fallback", "external"] {
            assert_eq!(listing.matches(&format!("• {id}:")).count(), 1, "{listing}");
        }
        assert!(
            listing.contains(&format!(
                "Shared profiles referenced by {}",
                config_path.display()
            )),
            "{listing}"
        );
        assert!(
            listing.contains(&registry_path.display().to_string()),
            "{listing}"
        );
        assert!(listing.contains("architect -> coder"), "{listing}");
        assert!(listing.contains("developer -> fallback"), "{listing}");
        assert!(listing.contains("security-reviewer -> coder"), "{listing}");
        assert!(
            listing.contains("external-agent copilot /not/executed remote-model"),
            "{listing}"
        );
        for absent in [
            "no models configured",
            "Configured Models in Project",
            "Global User Profiles",
            "global-unrelated",
            "global-model",
        ] {
            assert!(!listing.contains(absent), "{absent}: {listing}");
        }
        assert_eq!(fs::read_to_string(config_path).unwrap(), config);
        assert_eq!(fs::read_to_string(registry_path).unwrap(), SHARED_MODELS);
    }
}

#[test]
fn agent_list_uses_the_selected_shared_user_source_without_legacy_empty_warning() {
    let directory = list_fixture();
    let root = directory.path().canonicalize().unwrap();
    fs::write(root.join("user-providers.toml"), SHARED_MODELS).unwrap();
    let user_path = root.join("user.toml");
    fs::write(&user_path, shared_binding("user-providers.toml")).unwrap();
    let listing = agent_list(&root);
    assert!(
        listing.contains(&format!(
            "Shared profiles referenced by {}",
            user_path.display()
        )),
        "{listing}"
    );
    assert!(listing.contains("developer -> fallback"), "{listing}");
    assert!(!listing.contains("no models configured"), "{listing}");
    assert_eq!(listing.matches("• coder:").count(), 1, "{listing}");
}

#[test]
fn agent_list_uses_the_selected_project_local_shared_source_instead_of_global_models() {
    let directory = list_fixture();
    let root = directory.path().canonicalize().unwrap();
    fs::write(root.join("providers.toml"), SHARED_MODELS).unwrap();
    fs::create_dir(root.join(".kvist")).unwrap();
    let local_path = root.join(".kvist/config.toml");
    fs::write(&local_path, shared_binding("../providers.toml")).unwrap();
    fs::write(
        root.join("user.toml"),
        legacy_binding("global-unrelated", "global-model"),
    )
    .unwrap();
    let listing = agent_list(&root);
    assert!(
        listing.contains(&format!(
            "Shared profiles referenced by {}",
            local_path.display()
        )),
        "{listing}"
    );
    assert!(listing.contains("developer -> fallback"), "{listing}");
    assert!(!listing.contains("no models configured"), "{listing}");
    assert!(!listing.contains("global-unrelated"), "{listing}");
    assert_eq!(listing.matches("• coder:").count(), 1, "{listing}");
}

#[test]
fn shared_agent_list_with_no_enabled_profiles_does_not_fall_through_to_legacy_models() {
    let directory = list_fixture();
    let root = directory.path();
    fs::write(root.join("providers.toml"), "schema_version = 1\n").unwrap();
    fs::write(
        root.join("kvist.toml"),
        "schema_version = 1\ncomponent_root = \"src\"\n\
         [agent.registry]\npath = \"providers.toml\"\n",
    )
    .unwrap();
    fs::write(
        root.join("user.toml"),
        legacy_binding("global-unrelated", "global-model"),
    )
    .unwrap();
    let listing = agent_list(root);
    assert!(listing.contains("no enabled shared profiles"), "{listing}");
    assert!(!listing.contains("no models configured"), "{listing}");
    assert!(!listing.contains("global-unrelated"), "{listing}");
}

#[test]
fn legacy_agent_list_preserves_explicit_project_and_user_profile_scopes() {
    let directory = list_fixture();
    let root = directory.path().canonicalize().unwrap();
    fs::write(
        root.join("kvist.toml"),
        "schema_version = 1\ncomponent_root = \"src\"\n".to_owned()
            + &legacy_binding("project-local", "project-model"),
    )
    .unwrap();
    let user_path = root.join("user.toml");
    fs::write(
        &user_path,
        legacy_binding("global-unrelated", "global-model"),
    )
    .unwrap();
    let listing = agent_list(&root);
    assert!(
        listing.contains("Configured Models in Project:"),
        "{listing}"
    );
    assert!(listing.contains("• project-local"), "{listing}");
    assert!(
        listing.contains(&format!("Global User Profiles ({}):", user_path.display())),
        "{listing}"
    );
    assert!(listing.contains("• global-unrelated"), "{listing}");
    assert!(
        !listing.contains("Shared profiles referenced by"),
        "{listing}"
    );
}
