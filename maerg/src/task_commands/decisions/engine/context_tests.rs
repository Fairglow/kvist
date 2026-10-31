use super::*;

#[test]
fn creating_consumed_catalog_or_discussion_invalidates_the_prepared_plan() {
    for (binding, suffix) in [
        ("decision-options", "options"),
        ("decision-discussion", "discussion"),
    ] {
        let fixture = Fixture::new();
        let plan = fixture.plan();
        assert_eq!(plan.input.bindings[binding], "absent");
        let path = fixture.component().join(DIRECTORY).join(format!(
            "{}.{}.json",
            capture_id(&plan.input).unwrap(),
            suffix
        ));
        fs::write(&path, "{\"new\":\"advisory context\"}\n").unwrap();
        assert!(fixture.apply(&plan).is_err());
        assert_eq!(
            fs::read_to_string(fixture.component().join("TODOS.yaml")).unwrap(),
            plan.input.files["TODOS.yaml"]
        );
        fs::remove_file(path).unwrap();
        fixture.apply(&plan).unwrap();
    }
}

#[test]
fn current_alias_changes_keep_workspace_identity_but_invalidate_approval() {
    for (binding, suffix) in [
        ("decision-options", "options"),
        ("decision-discussion", "discussion"),
    ] {
        let fixture = Fixture::new();
        let before = fixture.input();
        let id = capture_id(&before).unwrap();
        let directory = fixture.component().join(DIRECTORY);
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!("{id}.{suffix}.json"));
        let original = "{\"advisory\":\"original exact context\"}\n";
        fs::write(&path, original).unwrap();
        let captured = fixture.input();
        assert_eq!(capture_id(&captured).unwrap(), id);
        assert_eq!(captured.bindings[binding], digest(original.as_bytes()));
        let plan = fixture.plan();
        fs::write(&path, "{\"advisory\":\"external edited context\"}\n").unwrap();
        assert!(fixture.apply(&plan).is_err());
        assert_eq!(
            fs::read_to_string(fixture.component().join("TODOS.yaml")).unwrap(),
            plan.input.files["TODOS.yaml"]
        );
        fs::write(path, original).unwrap();
        fixture.apply(&plan).unwrap();
    }
}

fn attention_files(path: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fs::read_dir(path)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                PathBuf::from(entry.file_name()),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}

fn pending_task_run_preserves_attention(exact: bool) {
    let fixture = Fixture::new();
    let attention = fixture.component().join(".kvist/attention");
    fs::create_dir_all(&attention).unwrap();
    fs::write(
        attention.join("retained.md"),
        "# Retained task attention\n\nOriginal evidence.\n",
    )
    .unwrap();
    let before = attention_files(&attention);
    let plan = fixture.plan();
    assert!(
        plan.input
            .bindings
            .contains_key("local/.kvist/attention/retained.md")
    );
    assert!(
        transaction::apply(
            fixture.project.path(),
            Path::new("."),
            &plan.id,
            &plan.approval,
            Some(1),
        )
        .is_err()
    );
    assert!(ensure_no_pending(&fixture.component()).is_err());
    assert!(
        run_task_or_item(
            fixture.project.path(),
            Path::new("."),
            exact.then_some(plan.input.task.id.as_str()),
            false,
        )
        .is_err()
    );
    assert!(
        attention_files(&attention) == before,
        "fenced task preflight must not publish diagnostics into bound attention evidence"
    );
    fixture.apply(&plan).unwrap();
    ensure_no_pending(&fixture.component()).unwrap();
}

#[test]
fn fenced_bare_task_run_preserves_attention_and_exact_decision_recovery() {
    pending_task_run_preserves_attention(false);
}

#[test]
fn fenced_exact_task_run_preserves_attention_and_exact_decision_recovery() {
    pending_task_run_preserves_attention(true);
}

#[test]
fn numbered_approval_recovers_identical_interrupted_apply_without_replanning() {
    let fixture = Fixture::new();
    let plan = fixture.plan();
    let context = context_for(fixture.project.path(), Path::new(".")).unwrap();
    let responses = super::super::shortcuts::plan(&context, &plan).unwrap();
    assert_eq!(responses[0].number, 1);
    assert!(
        transaction::apply(
            fixture.project.path(),
            Path::new("."),
            &plan.id,
            &plan.approval,
            Some(1)
        )
        .is_err()
    );
    let output = super::super::super::execute(
        fixture.project.path(),
        Path::new("."),
        Some(Action::Do { number: 1 }),
    )
    .unwrap();
    assert_eq!(output.plan_id, Some(plan.id));
    ensure_no_pending(&fixture.component()).unwrap();
}
