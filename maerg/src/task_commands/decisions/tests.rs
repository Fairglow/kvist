use super::*;

#[test]
fn printed_decision_component_words_are_unambiguous_paths() {
    for name in [
        "show", "options", "discuss", "plan", "apply", "help", "-child",
    ] {
        let (value, rest) = sav::split_raw_command(&component_argument(Path::new(name))).unwrap();
        assert_eq!(value, format!("./{name}"));
        assert!(rest.is_empty());
    }
    assert_eq!(
        sav::split_raw_command(&component_argument(Path::new("maerg")))
            .unwrap()
            .0,
        "maerg"
    );
}

#[test]
fn catalogs_support_more_than_three_choices_and_reject_ambiguous_ids() {
    let mut catalog = Alternatives {
        summary: "Choose an investigated approach.".into(),
        options: (0..8)
            .map(|id| Alternative {
                id: format!("option-{id}"),
                title: "Approach".into(),
                consequences: "Preserve guarantees; schedule verification.".into(),
                recommendation: "Advisory; compare evidence.".into(),
            })
            .collect(),
    };
    assert!(validate_alternatives(&catalog).is_ok());
    catalog.options[1].id = catalog.options[0].id.clone();
    assert!(validate_alternatives(&catalog).is_err());
    catalog.options[1].id = "../uncontrolled".into();
    assert!(validate_alternatives(&catalog).is_err());
}

#[test]
fn manual_drafts_cannot_mint_queue_lifecycle_fields() {
    assert!(strict_parse::<Draft>(r#"{"schema_version":1,"rationale":"retry","documents":{},"tasks":[],"disposition":"resolve","status":"completed"}"#).is_err());
    assert!(strict_parse::<TaskDefinition>(r#"{"id":"task","title":"title","description":"description","context":"context","purpose":"purpose","expected_outcome":"outcome","kind":"test","depends_on":[],"requirements":[],"timestamps":{}}"#).is_err());
    assert!(strict_parse::<Draft>(r#"{"schema_version":1,"rationale":"retry","documents":{},"tasks":[],"disposition":"cancelled"}"#).is_err());
    assert!(strict_parse::<Draft>(r#"{"schema_version":1,"rationale":"retry","documents":{},"tasks":[],"disposition":"resolve"}"#).is_ok());
}

#[test]
fn concurrent_support_does_not_drop_history_or_change_controlled_files() {
    let temporary = tempfile::tempdir().unwrap();
    let component = temporary.path().join("component");
    fs::create_dir(&component).unwrap();
    let queue = component.join("TODOS.yaml");
    fs::write(&queue, "unchanged controlled queue").unwrap();
    let context = TaskContext {
        project_dir: temporary.path().to_path_buf(),
        component_path: PathBuf::from("component"),
        component_dir: component.clone(),
    };
    let conversation = Conversation {
        schema_version: 1,
        capture: "frozen".into(),
        task_id: "paused".into(),
        exchanges: vec![Exchange {
            question: "Why?".into(),
            answer: "Advisory reason.".into(),
        }],
    };
    save_support(&context, "discussion", &conversation, "# Discussion", None).unwrap();
    assert!(
        save_support(
            &context,
            "discussion",
            &conversation,
            "# Stale answer",
            None
        )
        .is_err()
    );
    assert_eq!(
        fs::read_to_string(component.join(DIRECTORY).join("discussion.md")).unwrap(),
        "# Discussion"
    );
    assert_eq!(
        fs::read_to_string(queue).unwrap(),
        "unchanged controlled queue"
    );
}

#[test]
fn bounded_decision_input_rejects_links_and_invalid_utf8() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("draft.json");
    fs::write(&path, [0xff]).unwrap();
    assert!(bounded_read(&path).is_err());
    fs::write(&path, "{}").unwrap();
    let linked = temporary.path().join("linked");
    fs::hard_link(&path, &linked).unwrap();
    assert!(bounded_read(&linked).is_err());
    #[cfg(unix)]
    {
        let symlink = temporary.path().join("symlink");
        std::os::unix::fs::symlink(&path, &symlink).unwrap();
        assert!(bounded_read(&symlink).is_err());
    }
}
