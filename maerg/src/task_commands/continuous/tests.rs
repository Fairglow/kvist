use super::*;

#[test]
#[cfg(target_os = "linux")]
fn authenticated_inventory_ledger_can_exceed_source_file_bound() {
    let project = tempfile::tempdir().unwrap();
    let context = TaskContext {
        project_dir: project.path().to_path_buf(),
        component_dir: project.path().to_path_buf(),
        component_path: PathBuf::from("."),
    };
    let mut directory = project.path().join("src");
    for _ in 0..8 {
        directory.push("p".repeat(240));
    }
    fs::create_dir_all(&directory).unwrap();
    for index in 0..500 {
        fs::write(
            directory.join(format!("{index:04}-{}", "f".repeat(220))),
            "x",
        )
        .unwrap();
    }
    fs::write(project.path().join("TODOS.yaml"), "fixture").unwrap();
    fs::write(project.path().join("IMPL.md"), "observations").unwrap();
    let mut ledger = Ledger {
        version: LEDGER_VERSION,
        component: PathBuf::from("."),
        policy: TaskRunConfig::default(),
        policy_identity: String::from("fixture"),
        definition_digest: String::from("fixture"),
        phase: Phase::Paused,
        active_item: None,
        completed_items: BTreeSet::new(),
        baseline: checkpoint::capture(&context).unwrap(),
        expected_head: None,
        expected_effects: String::from("fixture"),
        pending: None,
        authentication_tag: String::new(),
    };
    save(&context, &mut ledger, b"fixture-secret").unwrap();
    assert!(
        fs::metadata(project.path().join(LEDGER_PATH))
            .unwrap()
            .len()
            > MAX_SCOPE_FILE_BYTES
    );
    let restored = load(&context, b"fixture-secret").unwrap().unwrap();
    assert_eq!(restored.baseline, ledger.baseline);
    assert!(load(&context, b"wrong-secret").is_err());
}

#[test]
fn repository_queue_has_unambiguous_complete_lifecycle_items() {
    let queue = parse(include_str!("../../../TODOS.yaml")).expect("repository queue");
    let groups = item_groups(&queue.tasks).expect("complete contiguous lifecycle items");
    assert!(groups.len() > 10);
}

#[test]
fn definition_binding_excludes_lifecycle_but_not_task_intent() {
    let mut queue = parse(include_str!("../../../TODOS.yaml")).expect("repository queue");
    let original = definition_digest(&queue).unwrap();
    queue.tasks[0].status = TaskStatus::Blocked;
    queue.tasks[0].blocked_reason = Some("operator stop".into());
    assert_eq!(definition_digest(&queue).unwrap(), original);
    queue.tasks[0].expected_outcome.push_str(" changed");
    assert_ne!(definition_digest(&queue).unwrap(), original);
}

#[test]
fn lifecycle_stems_are_exact_not_arbitrary_prefixes() {
    assert_eq!(item_stem("feature-implement-code"), Some("feature".into()));
    assert_eq!(item_stem("implement-code"), Some(String::new()));
    assert_eq!(item_stem("feature-implementation"), None);
    assert_eq!(item_stem("featurewrite-tests"), None);
}

#[test]
fn commit_policy_is_typed_strict_and_defaults_to_items() {
    assert_eq!(
        toml::from_str::<TaskRunConfig>("").unwrap().commit,
        CommitFrequency::Item
    );
    for value in ["item", "component", "manual"] {
        assert!(toml::from_str::<TaskRunConfig>(&format!("commit = {value:?}")).is_ok());
    }
    for invalid in ["commit = \"all\"", "commit = true", "commmit = \"manual\""] {
        assert!(toml::from_str::<TaskRunConfig>(invalid).is_err());
    }
}
