use super::*;

#[test]
fn native_offline_requests_grant_only_data_and_match_locked_alias_protocol() {
    let root = tempfile::tempdir().unwrap();
    let toolchain = root.path().join("toolchain");
    fs::create_dir_all(toolchain.join("bin")).unwrap();
    fs::write(toolchain.join("bin/cargo"), b"fixture").unwrap();
    for directory in ["runtime", "scratch", "workspace", "registry", "git"] {
        fs::create_dir(root.path().join(directory)).unwrap();
    }
    let mut config = SandboxConfig {
        runner: "/usr/bin/true".into(),
        backend: "/usr/bin/bwrap".into(),
        environment_allowlist: Vec::new(),
        acquisition: crate::config::AcquisitionConfig::default(),
    };
    let backend = BackendIdentity {
        kind: "bubblewrap".into(),
        path: "/usr/bin/bwrap".into(),
        digest: digest_label(b"backend"),
    };
    for (present, command, parallelism) in [false, true].into_iter().flat_map(|present| {
        [
            (OfflineCommand::Test, CargoParallelism::Normal),
            (OfflineCommand::Test, CargoParallelism::Serial),
            (OfflineCommand::Metadata, CargoParallelism::Normal),
        ]
        .map(|(command, parallelism)| (present, command, parallelism))
    }) {
        let encoded = Request {
            config: &config,
            runner_identity: &digest_label(b"runner"),
            backend: &backend,
            policy_identity: &digest_label(b"policy"),
            toolchain_identity: &digest_label(b"toolchain"),
            toolchain_root: &toolchain,
            cargo: &toolchain.join("bin/cargo"),
            runtime: &root.path().join("runtime"),
            scratch: &root.path().join("scratch"),
            workspace: &root.path().join("workspace"),
            registry: present.then(|| root.path().join("registry")).as_deref(),
            git: present.then(|| root.path().join("git")).as_deref(),
            phase: Phase::Verification {
                command,
                lockfile_identity: &digest_label(b"lock"),
                projection: None,
                parallelism,
            },
        }
        .encode()
        .unwrap();
        let request = galla::validation::parse_and_validate(&encoded).unwrap();
        assert_eq!(&request.argv[1..], command.arguments());
        assert_eq!(request.resources.max_processes, 4096);
        for (name, value) in galla::protocol::SERIAL_CARGO_ENV {
            assert_eq!(
                request.environment.get(name).map(String::as_str),
                (parallelism == CargoParallelism::Serial).then_some(value)
            );
        }
        let cache = request.cache.unwrap();
        assert_eq!(cache.cargo_home, HOME);
        assert!(cache.writable.is_none() && cache.approved.is_none());
        assert!(cache.promotion.is_none() && cache.lockfile.is_none());
        assert!(request.grants.iter().all(|grant| {
            grant.access == galla::protocol::Access::ReadOnly
                || grant.destination == CARGO_SCRATCH_DEST
        }));
        assert!(!request.grants.iter().any(|grant| grant.destination == HOME));
        assert_eq!(request.grants.len(), if present { 6 } else { 4 });
        let workspace = request
            .grants
            .iter()
            .find(|grant| grant.destination == CARGO_WORKING_DIRECTORY)
            .unwrap();
        assert_eq!(workspace.identity, digest_label(b"lock"));
    }
    for (origin, source, expected) in [
        (
            "https://registry.example.com/index",
            "registry+https://registry.example.com/index",
            galla::protocol::NativeRegistryProtocol::Git,
        ),
        (
            "https://registry.example.com/index/",
            "sparse+https://registry.example.com/index/",
            galla::protocol::NativeRegistryProtocol::Sparse,
        ),
    ] {
        config.acquisition.additional_sources = vec![crate::config::PackageSource::CargoRegistry {
            name: "alt".into(),
            index_origin: origin.into(),
            download_origin: "https://downloads.example.com/".into(),
        }];
        fs::write(
            root.path().join("workspace/Cargo.lock"),
            format!("version=4\n[[package]]\nname='fixture'\nversion='1.0.0'\nsource='{source}'\n"),
        )
        .unwrap();
        let encoded = Request {
            config: &config,
            runner_identity: &digest_label(b"runner"),
            backend: &backend,
            policy_identity: &digest_label(b"policy"),
            toolchain_identity: &digest_label(b"toolchain"),
            toolchain_root: &toolchain,
            cargo: &toolchain.join("bin/cargo"),
            runtime: &root.path().join("runtime"),
            scratch: &root.path().join("scratch"),
            workspace: &root.path().join("workspace"),
            registry: None,
            git: None,
            phase: Phase::Verification {
                command: OfflineCommand::Metadata,
                lockfile_identity: &digest_label(b"lock"),
                projection: None,
                parallelism: CargoParallelism::Normal,
            },
        }
        .encode()
        .unwrap();
        let request = galla::validation::parse_and_validate(&encoded).unwrap();
        let aliases = request.cache.unwrap().native.unwrap().registries.unwrap();
        assert_eq!(aliases.len(), 1);
        assert_eq!(aliases[0].protocol, expected);
        assert_eq!(aliases[0].index_origin, origin);
    }
    fs::write(root.path().join("workspace/Cargo.lock"), "version=4\n[[package]]\nsource='registry+https://registry.example.com/index/'\n[[package]]\nsource='sparse+https://registry.example.com/index/'\n").unwrap();
    let error = aliases::selected(&config, &root.path().join("workspace")).unwrap_err();
    assert!(error.to_string().contains("both Git and sparse"), "{error}");
}
