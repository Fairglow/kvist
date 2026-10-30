use super::*;
use std::process::Command;
use tempfile::TempDir;

#[path = "context_tests.rs"]
mod context;
#[path = "document_tests.rs"]
mod documents;

struct Fixture {
    project: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let project = tempfile::Builder::new()
            .prefix(".decision-engine-test-")
            .tempdir_in(std::env::current_dir().unwrap())
            .unwrap();
        crate::init::initialize(project.path()).unwrap();
        let component = project.path().join("src");
        let path = component.join("TODOS.yaml");
        let mut queue = parse(&fs::read_to_string(&path).unwrap()).unwrap();
        let timestamp = Timestamp::now().unwrap();
        let ids = [
            "scope-write-tests",
            "scope-implement-code",
            "scope-security-audit",
            "scope-compliance-review",
        ];
        for (index, kind) in [
            TaskKind::Test,
            TaskKind::Implementation,
            TaskKind::SecurityAudit,
            TaskKind::ComplianceReview,
        ]
        .into_iter()
        .enumerate()
        {
            queue.tasks.push(Task {
                id: ids[index].to_owned(),
                title: format!("Authored {}", ids[index]),
                description: "Perform the explicitly planned local lifecycle work".to_owned(),
                context: "An isolated tracked test component".to_owned(),
                purpose: "Preserve human-directed scope".to_owned(),
                expected_outcome: "The planned local behavior is verified".to_owned(),
                kind,
                status: TaskStatus::Pending,
                depends_on: if index == 0 {
                    Vec::new()
                } else {
                    vec![ids[index - 1].to_owned()]
                },
                requirements: vec!["REQUIREMENTS.md#REQ-TEST".to_owned()],
                timestamps: crate::task_queue::TaskTimestamps {
                    created_at: timestamp.clone(),
                    updated_at: timestamp.clone(),
                    completed_at: None,
                    recovered: false,
                },
                blocked_reason: None,
                recovery_state: None,
                acceptance_id: None,
                disposition: None,
            });
        }
        queue.tasks[0].status = TaskStatus::AwaitingDecision;
        queue.tasks[0].blocked_reason = Some("Choose the revised scope".to_owned());
        fs::write(&path, serialize(&queue).unwrap()).unwrap();
        fs::create_dir_all(component.join("src")).unwrap();
        fs::write(component.join("src/lib.rs"), "pub fn retained() {}\n").unwrap();
        for args in [vec!["init", "--quiet"], vec!["add", "."]] {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(project.path())
                    .status()
                    .unwrap()
                    .success()
            );
        }
        Self { project }
    }

    fn input(&self) -> Capture {
        capture(self.project.path(), Path::new("."), None).unwrap()
    }

    fn draft(&self) -> Draft {
        let mut documents = BTreeMap::new();
        documents.insert(
            "DESIGN.md".to_owned(),
            format!(
                "{}\nA deliberate local decision.\n",
                self.input().files["DESIGN.md"]
            ),
        );
        Draft {
            schema_version: 1,
            rationale: "Use the selected conservative direction".to_owned(),
            documents,
            document_edits: Vec::new(),
            tasks: Vec::new(),
            disposition: Disposition::Resolve,
        }
    }

    fn plan(&self) -> Plan {
        self.prepare(self.draft()).unwrap()
    }

    fn prepare(&self, draft: Draft) -> Result<Plan> {
        prepare(
            self.project.path(),
            self.input(),
            draft,
            Selection {
                option: None,
                guidance: "Human-selected change".to_owned(),
            },
            IntentReview {
                report: "Explicit manual-plan review exception".to_owned(),
                exception: true,
            },
        )
    }

    fn apply(&self, plan: &Plan) -> Result<Plan> {
        apply(
            self.project.path(),
            Path::new("."),
            &plan.id,
            &plan.approval,
        )
    }

    fn component(&self) -> PathBuf {
        self.project.path().join("src")
    }
}

#[test]
fn typed_plan_freezes_exact_outputs_and_repeat_apply_is_idempotent() {
    let fixture = Fixture::new();
    let before = fixture.input();
    let plan = fixture.plan();
    assert_eq!(fixture.input().files, before.files);
    assert!(
        fixture
            .component()
            .join(DIRECTORY)
            .join(format!("{}.md", plan.id))
            .is_file()
    );
    assert!(
        fixture
            .apply(&Plan {
                approval: "wrong".to_owned(),
                ..plan.clone()
            })
            .is_err()
    );
    fixture.apply(&plan).unwrap();
    let after = fs::read(fixture.component().join("TODOS.yaml")).unwrap();
    let queue = parse(std::str::from_utf8(&after).unwrap()).unwrap();
    assert_eq!(queue.tasks[0].status, TaskStatus::Pending);
    assert!(queue.tasks[0].blocked_reason.is_none());
    assert_eq!(
        fs::read_to_string(fixture.component().join("src/lib.rs")).unwrap(),
        "pub fn retained() {}\n"
    );
    fixture.apply(&plan).unwrap();
    assert_eq!(
        fs::read(fixture.component().join("TODOS.yaml")).unwrap(),
        after
    );
}

#[test]
fn source_and_intent_drift_and_tampered_plans_refuse_without_writes() {
    let fixture = Fixture::new();
    let plan = fixture.plan();
    let path = fixture.component().join("src/lib.rs");
    fs::write(&path, "external source edit\n").unwrap();
    assert!(fixture.apply(&plan).is_err());
    fs::write(&path, "pub fn retained() {}\n").unwrap();
    let path = fixture
        .component()
        .join(DIRECTORY)
        .join(format!("{}.json", plan.id));
    let mut tampered = plan.clone();
    tampered.rationale = "Edited after approval".to_owned();
    fs::write(path, serde_json::to_string(&tampered).unwrap()).unwrap();
    assert!(load_plan(fixture.project.path(), Path::new("."), &plan.id).is_err());
    assert_eq!(fixture.input().task.status, TaskStatus::AwaitingDecision);
}

#[test]
fn retirement_requires_full_closure_and_retains_full_records_and_reasons() {
    let fixture = Fixture::new();
    let input = fixture.input();
    let mut draft = fixture.draft();
    draft.disposition = Disposition::Withdraw;
    draft.tasks = vec![TaskEdit::Retire {
        id: input.task.id.clone(),
        reason: "Retire original scope".to_owned(),
    }];
    assert!(fixture.prepare(draft.clone()).is_err());
    draft.tasks = input
        .queue
        .tasks
        .iter()
        .map(|task| TaskEdit::Retire {
            id: task.id.clone(),
            reason: format!("Explicit retirement of {}", task.id),
        })
        .collect();
    let plan = fixture.prepare(draft).unwrap();
    assert_eq!(plan.retired, input.queue.tasks);
    for task in &plan.retired {
        assert_eq!(
            plan.retirement_reasons[&task.id],
            format!("Explicit retirement of {}", task.id)
        );
    }
    fixture.apply(&plan).unwrap();
    let queue = read_queue(&fixture.component()).unwrap();
    assert!(queue.tasks.is_empty());
}

#[test]
fn interrupted_apply_fences_writers_and_recovers_only_known_before_or_after() {
    let fixture = Fixture::new();
    let plan = fixture.plan();
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
    assert!(ensure_no_pending(&fixture.component()).is_err());
    assert!(ensure_component_attempts_recovered(&fixture.component()).is_err());
    assert!(guard_repair_evidence(&fixture.component(), None).is_err());
    assert!(capture(fixture.project.path(), Path::new("."), None).is_err());
    assert!(context_for(fixture.project.path(), Path::new(".")).is_ok());
    let path = fixture.component().join("DESIGN.md");
    let own = fs::read_to_string(&path).unwrap();
    fs::write(&path, "external intermediate edit").unwrap();
    assert!(fixture.apply(&plan).is_err());
    fs::write(&path, own).unwrap();
    fixture.apply(&plan).unwrap();
    ensure_no_pending(&fixture.component()).unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        plan.outputs["DESIGN.md"]
    );
}

#[test]
fn draft_fields_paths_and_invalid_graphs_are_rejected() {
    let fixture = Fixture::new();
    assert!(serde_json::from_str::<Draft>(r#"{"schema_version":1,"rationale":"x","documents":{},"tasks":[],"disposition":"resolve","status":"completed"}"#).is_err());
    let mut draft = fixture.draft();
    draft
        .documents
        .insert("../CONTRACT.md".to_owned(), "x".to_owned());
    assert!(fixture.prepare(draft).is_err());
    let mut draft = fixture.draft();
    let task = fixture.input().task;
    let definition = TaskDefinition {
        id: "new-test".to_owned(),
        title: "Test a prerequisite".to_owned(),
        description: task.description.clone(),
        context: task.context.clone(),
        purpose: task.purpose.clone(),
        expected_outcome: task.expected_outcome.clone(),
        kind: TaskKind::Test,
        depends_on: vec![task.id],
        requirements: task.requirements,
    };
    draft.tasks.push(TaskEdit::Add { task: definition });
    assert!(fixture.prepare(draft).is_err());
}

#[cfg(unix)]
#[test]
fn symlinks_and_hardlinks_are_rejected_before_capture() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let source = fixture.component().join("src/lib.rs");
    let alias = fixture.component().join("src/alias.rs");
    symlink(&source, &alias).unwrap();
    assert!(capture(fixture.project.path(), Path::new("."), None).is_err());
    fs::remove_file(&alias).unwrap();
    fs::hard_link(&source, &alias).unwrap();
    assert!(capture(fixture.project.path(), Path::new("."), None).is_err());
}

fn definition(task: &Task, id: &str, kind: TaskKind, depends_on: Vec<String>) -> TaskDefinition {
    TaskDefinition {
        id: id.to_owned(),
        title: format!("Authored {id}"),
        description: task.description.clone(),
        context: task.context.clone(),
        purpose: task.purpose.clone(),
        expected_outcome: task.expected_outcome.clone(),
        kind,
        depends_on,
        requirements: task.requirements.clone(),
    }
}

fn write_queue(fixture: &Fixture, queue: &TaskQueue) {
    fs::write(
        fixture.component().join("TODOS.yaml"),
        serialize(queue).unwrap(),
    )
    .unwrap();
}

#[test]
fn additions_form_a_full_lifecycle_before_the_resumed_task() {
    let fixture = Fixture::new();
    let input = fixture.input();
    let mut draft = fixture.draft();
    let kinds = [
        TaskKind::Test,
        TaskKind::Implementation,
        TaskKind::SecurityAudit,
        TaskKind::ComplianceReview,
    ];
    let ids = [
        "follow-test",
        "follow-code",
        "follow-audit",
        "follow-review",
    ];
    for (index, kind) in kinds.into_iter().enumerate() {
        draft.tasks.push(TaskEdit::Add {
            task: definition(
                &input.task,
                ids[index],
                kind,
                if index == 0 {
                    Vec::new()
                } else {
                    vec![ids[index - 1].to_owned()]
                },
            ),
        });
    }
    let plan = fixture.prepare(draft).unwrap();
    let queue = parse(&plan.outputs["TODOS.yaml"]).unwrap();
    assert_eq!(
        queue.tasks[..4]
            .iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>(),
        ids
    );
    for id in ids {
        assert!(
            queue.tasks[4]
                .depends_on
                .iter()
                .any(|dependency| dependency == id)
        );
    }
    for task in &queue.tasks[..4] {
        assert_eq!(task.status, TaskStatus::Pending);
        assert!(task.timestamps.completed_at.is_none());
        assert!(!task.timestamps.recovered);
    }
    fixture.apply(&plan).unwrap();
    assert_eq!(
        next_ready_task_id(&read_queue(&fixture.component()).unwrap().tasks),
        Some("follow-test".to_owned())
    );
}

#[test]
fn lifecycle_and_unknown_dependency_edits_fail_native_validation() {
    let fixture = Fixture::new();
    for (kind, dependencies) in [
        (TaskKind::Implementation, Vec::new()),
        (TaskKind::Test, vec!["unknown-task".to_owned()]),
    ] {
        let mut draft = fixture.draft();
        draft.tasks.push(TaskEdit::Add {
            task: definition(&fixture.input().task, "invalid-task", kind, dependencies),
        });
        assert!(fixture.prepare(draft).is_err());
    }
}

#[test]
fn completed_records_and_index_are_preserved_exactly() {
    let fixture = Fixture::new();
    let mut queue = fixture.input().queue;
    queue.tasks[0].status = TaskStatus::Completed;
    queue.tasks[0].blocked_reason = None;
    queue.tasks[0].timestamps.completed_at = Some(queue.tasks[0].timestamps.updated_at.clone());
    queue.tasks[1].status = TaskStatus::AwaitingDecision;
    queue.tasks[1].blocked_reason = Some("Choose an implementation direction".to_owned());
    let completed = queue.tasks[0].clone();
    write_queue(&fixture, &queue);
    let index = fs::read(fixture.project.path().join(".git/index")).unwrap();
    let mut draft = fixture.draft();
    draft.tasks.push(TaskEdit::Update {
        task: definition(
            &completed,
            &completed.id,
            completed.kind,
            completed.depends_on.clone(),
        ),
    });
    assert!(fixture.prepare(draft).is_err());
    let plan = fixture.plan();
    fixture.apply(&plan).unwrap();
    assert_eq!(
        read_queue(&fixture.component()).unwrap().tasks[0],
        completed
    );
    assert_eq!(
        fs::read(fixture.project.path().join(".git/index")).unwrap(),
        index
    );
}

#[test]
fn ambiguous_pauses_and_unrelated_paused_updates_are_rejected() {
    let fixture = Fixture::new();
    let mut queue = fixture.input().queue;
    queue.tasks[1].status = TaskStatus::Blocked;
    queue.tasks[1].blocked_reason = Some("Independent pause".to_owned());
    write_queue(&fixture, &queue);
    assert!(capture(fixture.project.path(), Path::new("."), None).is_err());
    let input = capture(
        fixture.project.path(),
        Path::new("."),
        Some(&queue.tasks[0].id),
    )
    .unwrap();
    let mut draft = Draft {
        schema_version: 1,
        rationale: "Only resolve the selected pause".to_owned(),
        documents: BTreeMap::new(),
        document_edits: Vec::new(),
        tasks: Vec::new(),
        disposition: Disposition::Resolve,
    };
    draft.tasks.push(TaskEdit::Update {
        task: definition(
            &queue.tasks[1],
            &queue.tasks[1].id,
            queue.tasks[1].kind,
            queue.tasks[1].depends_on.clone(),
        ),
    });
    assert!(
        prepare(
            fixture.project.path(),
            input,
            draft,
            Selection {
                option: None,
                guidance: "No unrelated changes".to_owned()
            },
            IntentReview {
                report: "Deliberate local test exception".to_owned(),
                exception: true
            },
        )
        .is_err()
    );
}

#[test]
fn structurally_valid_local_staleness_is_allowed_but_old_capture_is_not() {
    let fixture = Fixture::new();
    let input = fixture.input();
    let path = fixture.component().join("DESIGN.md");
    fs::write(
        &path,
        format!(
            "{}\nAn unaccepted local revision.\n",
            input.files["DESIGN.md"]
        ),
    )
    .unwrap();
    assert!(capture(fixture.project.path(), Path::new("."), None).is_ok());
    let draft = fixture.draft();
    assert!(
        prepare(
            fixture.project.path(),
            input,
            draft,
            Selection {
                option: None,
                guidance: "Use fresh local intent".to_owned()
            },
            IntentReview {
                report: "Deliberate local exception".to_owned(),
                exception: true
            },
        )
        .is_err()
    );
    let plan = fixture.plan();
    fixture.apply(&plan).unwrap();
    assert_eq!(
        read_queue(&fixture.component())
            .unwrap()
            .component
            .design_revision,
        digest(plan.outputs["DESIGN.md"].as_bytes())
    );
}

#[test]
fn malformed_markdown_and_recovery_placeholders_never_become_accepted_intent() {
    let fixture = Fixture::new();
    let mut draft = fixture.draft();
    draft
        .documents
        .insert("DESIGN.md".to_owned(), "# Invalid document".to_owned());
    assert!(fixture.prepare(draft).is_err());
    let mut draft = fixture.draft();
    draft
        .documents
        .get_mut("DESIGN.md")
        .unwrap()
        .push_str("\n<!-- kvist-recovery-blocker: human review required -->\n");
    assert!(fixture.prepare(draft).is_err());
}

#[test]
fn retired_journal_recognition_requires_an_authenticated_exact_receipt() {
    let fixture = Fixture::new();
    let input = fixture.input();
    let directory = fixture.component().join(".kvist-attempts");
    fs::create_dir(&directory).unwrap();
    let journal = directory.join(format!("{}.jsonl", input.task.id));
    fs::write(&journal, "{\"phase\":\"committed\"}\n").unwrap();
    let input = fixture.input();
    let mut draft = fixture.draft();
    draft.disposition = Disposition::Withdraw;
    draft.tasks = input
        .queue
        .tasks
        .iter()
        .map(|task| TaskEdit::Retire {
            id: task.id.clone(),
            reason: format!("Archive {} with all history", task.id),
        })
        .collect();
    let plan = fixture.prepare(draft).unwrap();
    assert!(!recognizes_retired_journal(&fixture.component(), &journal).unwrap());
    fixture.apply(&plan).unwrap();
    assert!(recognizes_retired_journal(&fixture.component(), &journal).unwrap());
    let other = Fixture::new();
    fs::create_dir_all(other.component().join(DIRECTORY)).unwrap();
    fs::copy(
        fixture
            .component()
            .join(DIRECTORY)
            .join(format!("{}.applied.json", plan.id)),
        other
            .component()
            .join(DIRECTORY)
            .join(format!("{}.applied.json", plan.id)),
    )
    .unwrap();
    fs::create_dir(other.component().join(".kvist-attempts")).unwrap();
    let copied = other
        .component()
        .join(".kvist-attempts")
        .join(format!("{}.jsonl", input.task.id));
    fs::copy(&journal, &copied).unwrap();
    assert!(recognizes_retired_journal(&other.component(), &copied).is_err());
    fs::write(&journal, "{\"phase\":\"externally-edited\"}\n").unwrap();
    assert!(recognizes_retired_journal(&fixture.component(), &journal).is_err());
}

#[test]
fn unknown_attempt_history_and_live_owners_are_not_silently_ignored() {
    let fixture = Fixture::new();
    let path = fixture.component().join(".kvist-attempts");
    fs::create_dir(&path).unwrap();
    fs::write(
        path.join("unknown-task.jsonl"),
        "{\"phase\":\"prepared\"}\n",
    )
    .unwrap();
    assert!(capture(fixture.project.path(), Path::new("."), None).is_err());
    fs::remove_dir_all(path).unwrap();
    let context = context_for(fixture.project.path(), Path::new(".")).unwrap();
    let lock = TaskLock::for_context(&context, "other-operation", &now().unwrap()).unwrap();
    assert!(capture(fixture.project.path(), Path::new("."), None).is_err());
    lock.release().unwrap();
}

#[test]
fn receipt_before_marker_cleanup_is_exactly_idempotently_recoverable() {
    let fixture = Fixture::new();
    let plan = fixture.plan();
    assert!(
        transaction::apply(
            fixture.project.path(),
            Path::new("."),
            &plan.id,
            &plan.approval,
            Some(plan.outputs.len() + 1),
        )
        .is_err()
    );
    let path = fixture
        .component()
        .join(DIRECTORY)
        .join(format!("{}.applied.json", plan.id));
    let receipt = fs::read(&path).unwrap();
    assert!(ensure_no_pending(&fixture.component()).is_err());
    fixture.apply(&plan).unwrap();
    assert_eq!(fs::read(path).unwrap(), receipt);
    ensure_no_pending(&fixture.component()).unwrap();
}

#[test]
fn pending_progress_tampering_and_source_drift_fence_recovery() {
    let fixture = Fixture::new();
    let plan = fixture.plan();
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
    fs::write(
        fixture.component().join("src/lib.rs"),
        "changed during interrupted transaction\n",
    )
    .unwrap();
    assert!(fixture.apply(&plan).is_err());
    fs::write(
        fixture.component().join("src/lib.rs"),
        "pub fn retained() {}\n",
    )
    .unwrap();
    let path = fixture.component().join(DIRECTORY).join("pending.json");
    let mut pending: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    pending["completed"] = serde_json::json!(["TODOS.yaml"]);
    fs::write(path, serde_json::to_vec(&pending).unwrap()).unwrap();
    assert!(fixture.apply(&plan).is_err());
    assert!(ensure_no_pending(&fixture.component()).is_err());
}

fn ledger(fixture: &Fixture, phase: &str, pending: bool) -> Value {
    let context = context_for(fixture.project.path(), Path::new(".")).unwrap();
    let secret = secret(&context).unwrap();
    let mut value = serde_json::json!({
        "version": 1, "component": ".", "policy": {"commit":"manual"},
        "policy_identity": "test-policy", "definition_digest": digest(b"test"),
        "phase":phase, "active_item":null, "completed_items":[],
        "baseline":{"component":".","entries":{}}, "expected_head":null,
        "expected_effects":digest(b"test"), "pending":null, "authentication_tag":""
    });
    if pending {
        value["pending"] = serde_json::json!({
            "commit_id":"checkpoint-test", "expected_head":"head", "branch":"refs/heads/test",
            "after":{"component":".","entries":{}}, "changes":{}
        });
    }
    value["authentication_tag"] = Value::String(authenticated(&value, &secret).unwrap());
    value
}

#[test]
fn native_authenticated_pending_checkpoint_and_running_ledgers_refuse_capture() {
    let fixture = Fixture::new();
    let path = fixture.component().join(".kvist/continuous.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        serde_json::to_vec(&ledger(&fixture, "paused", false)).unwrap(),
    )
    .unwrap();
    assert!(capture(fixture.project.path(), Path::new("."), None).is_ok());
    for (phase, pending) in [("paused", true), ("running", false)] {
        fs::write(
            &path,
            serde_json::to_vec(&ledger(&fixture, phase, pending)).unwrap(),
        )
        .unwrap();
        assert!(capture(fixture.project.path(), Path::new("."), None).is_err());
    }
    let mut value = ledger(&fixture, "paused", false);
    value["policy_identity"] = Value::String("tampered".to_owned());
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(capture(fixture.project.path(), Path::new("."), None).is_err());
}

#[test]
fn bounded_reads_and_read_only_context_refuse_unsafe_component_paths() {
    let fixture = Fixture::new();
    assert!(context_for(fixture.project.path(), Path::new("../outside")).is_err());
    let path = fixture.component().join("src/oversize.rs");
    fs::write(path, vec![b'x'; MAX_FILE as usize + 1]).unwrap();
    assert!(capture(fixture.project.path(), Path::new("."), None).is_err());
    assert!(context_for(fixture.project.path(), Path::new(".")).is_ok());
}

#[test]
fn decision_support_and_model_diagnostics_do_not_make_their_own_capture_stale() {
    let fixture = Fixture::new();
    let input = fixture.input();
    let draft = Draft {
        schema_version: 1,
        rationale: "Retry the same intent after resolving the question".to_owned(),
        documents: BTreeMap::new(),
        document_edits: Vec::new(),
        tasks: Vec::new(),
        disposition: Disposition::Resolve,
    };
    for path in [
        ".kvist/decisions/decision-catalog.options.json",
        ".kvist/decisions/decision-catalog.discussion.json",
        ".kvist/runs/decision.events.jsonl",
        ".kvist/logs/decision.log",
    ] {
        let path = fixture.component().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "New advisory model diagnostics").unwrap();
    }
    let plan = prepare(
        fixture.project.path(),
        input,
        draft,
        Selection {
            option: Some("retry".to_owned()),
            guidance: "Retry this unchanged scope".to_owned(),
        },
        IntentReview {
            report: "Deliberate manual exception".to_owned(),
            exception: true,
        },
    )
    .unwrap();
    assert_eq!(plan.outputs.keys().collect::<Vec<_>>(), vec!["TODOS.yaml"]);
    fixture.apply(&plan).unwrap();
}

#[test]
fn root_configuration_and_retained_decision_evidence_drift_refuse_approval() {
    for name in [
        "ROOT_CONTRACT.md",
        "kvist.toml",
        "src/.kvist/authoring/proposals/decision.json",
    ] {
        let fixture = Fixture::new();
        let path = fixture.project.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        if !path.exists() {
            fs::write(&path, "Retained decision evidence").unwrap();
        }
        let original = fs::read_to_string(&path).unwrap();
        let plan = fixture.plan();
        fs::write(&path, format!("{original}\n# External changed input\n")).unwrap();
        assert!(fixture.apply(&plan).is_err());
        assert_eq!(
            fs::read_to_string(fixture.component().join("TODOS.yaml")).unwrap(),
            plan.input.files["TODOS.yaml"]
        );
    }
}

#[test]
fn immediate_parent_contract_binding_crosses_transparent_namespaces() {
    let fixture = Fixture::new();
    let component = Path::new("namespace/child");
    let child = fixture.component().join(component);
    fs::create_dir_all(&child).unwrap();
    for name in [
        "REQUIREMENTS.md",
        "CONTRACT.md",
        "DESIGN.md",
        "TODOS.yaml",
        "IMPL.md",
    ] {
        fs::copy(fixture.component().join(name), child.join(name)).unwrap();
    }
    let context = context_for(fixture.project.path(), component).unwrap();
    let mut queue = read_queue(&child).unwrap();
    queue.component.parent_contract = capture::parent(&context).unwrap();
    assert!(queue.component.parent_contract.is_some());
    fs::write(child.join("TODOS.yaml"), serialize(&queue).unwrap()).unwrap();
    let input = capture(fixture.project.path(), component, None).unwrap();
    let plan = prepare(
        fixture.project.path(),
        input,
        Draft {
            schema_version: 1,
            rationale: "Only local intent is accepted".to_owned(),
            documents: BTreeMap::new(),
            document_edits: Vec::new(),
            tasks: Vec::new(),
            disposition: Disposition::Resolve,
        },
        Selection {
            option: None,
            guidance: "Keep upstream constraints".to_owned(),
        },
        IntentReview {
            report: "Explicit local exception".to_owned(),
            exception: true,
        },
    )
    .unwrap();
    let path = fixture.component().join("CONTRACT.md");
    let original = fs::read_to_string(&path).unwrap();
    fs::write(&path, format!("{original}\nChanged parent guarantee.\n")).unwrap();
    assert!(apply(fixture.project.path(), component, &plan.id, &plan.approval).is_err());
    assert!(capture(fixture.project.path(), component, None).is_err());
    fs::write(path, original).unwrap();
    apply(fixture.project.path(), component, &plan.id, &plan.approval).unwrap();
}

#[test]
fn preview_and_task_metadata_tampering_are_never_authority() {
    let fixture = Fixture::new();
    let plan = fixture.plan();
    let preview = fixture
        .component()
        .join(DIRECTORY)
        .join(format!("{}.md", plan.id));
    fs::write(
        preview,
        "# An edited human preview\nApprove without seeing the actual changes",
    )
    .unwrap();
    assert!(fixture.apply(&plan).is_err());
    let input = fixture.input();
    let mut definition = serde_json::to_value(definition(
        &input.task,
        "authored-task",
        TaskKind::Test,
        Vec::new(),
    ))
    .unwrap();
    definition["status"] = Value::String("completed".to_owned());
    assert!(
        serde_json::from_value::<TaskEdit>(serde_json::json!({"action":"add","task":definition}))
            .is_err()
    );
}

#[test]
fn withdrawal_never_archives_completed_dependants_or_live_work() {
    let fixture = Fixture::new();
    let mut queue = fixture.input().queue;
    queue.tasks[3].status = TaskStatus::Completed;
    queue.tasks[3].timestamps.completed_at = Some(queue.tasks[3].timestamps.updated_at.clone());
    write_queue(&fixture, &queue);
    let mut draft = fixture.draft();
    draft.disposition = Disposition::Withdraw;
    draft.tasks = queue
        .tasks
        .iter()
        .map(|task| TaskEdit::Retire {
            id: task.id.clone(),
            reason: "Explicitly proposed withdrawal".to_owned(),
        })
        .collect();
    assert!(fixture.prepare(draft).is_err());
    queue.tasks[0].status = TaskStatus::InProgress;
    queue.tasks[0].blocked_reason = None;
    write_queue(&fixture, &queue);
    assert!(
        capture(
            fixture.project.path(),
            Path::new("."),
            Some(&queue.tasks[0].id)
        )
        .is_err()
    );
}

#[test]
fn aggregate_capture_bounds_fail_explicitly_without_omitting_files() {
    let fixture = Fixture::new();
    let bytes = vec![b'x'; MAX_FILE as usize];
    for index in 0..8 {
        fs::write(
            fixture.component().join(format!("src/bounded-{index}.rs")),
            &bytes,
        )
        .unwrap();
    }
    let error = capture(fixture.project.path(), Path::new("."), None).unwrap_err();
    assert!(error.to_string().contains("8 MiB"));
}

#[test]
fn recovery_takes_over_only_the_exact_authenticated_dead_operation_lock() {
    let fixture = Fixture::new();
    let plan = fixture.plan();
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
    let context = context_for(fixture.project.path(), Path::new(".")).unwrap();
    let path = TaskLock::task_lock_path(&context).unwrap();
    let contents = format!(
        "schema_version: 1\nstarted_at: {}\ntask_id: \"decision-apply\"\npid: 2147483647\nnonce: 00000000000000000000000000000000\n",
        now().unwrap(),
    );
    write_new_file_atomically(&path, &contents).unwrap();
    assert!(fixture.apply(&plan).is_err());
    let pending_path = fixture.component().join(DIRECTORY).join("pending.json");
    let mut pending: Value = serde_json::from_slice(&fs::read(&pending_path).unwrap()).unwrap();
    pending["lock_digest"] = Value::String(digest(contents.as_bytes()));
    pending["authentication_tag"] =
        Value::String(authenticated(&pending, &secret(&context).unwrap()).unwrap());
    fs::write(pending_path, serde_json::to_vec(&pending).unwrap()).unwrap();
    let result = fixture.apply(&plan);
    if path.exists() {
        fs::remove_file(&path).unwrap();
    }
    result.unwrap();
    assert!(!path.exists());
    ensure_no_pending(&fixture.component()).unwrap();
}

#[test]
fn applied_receipt_is_idempotent_after_later_legitimate_execution() {
    let fixture = Fixture::new();
    let plan = fixture.plan();
    fixture.apply(&plan).unwrap();
    let receipt_path = fixture
        .component()
        .join(DIRECTORY)
        .join(format!("{}.applied.json", plan.id));
    let receipt = fs::read(&receipt_path).unwrap();
    let source_path = fixture.component().join("src/lib.rs");
    fs::write(&source_path, "pub fn subsequent_implementation() {}\n").unwrap();
    let mut queue = read_queue(&fixture.component()).unwrap();
    queue.tasks[0].status = TaskStatus::Completed;
    queue.tasks[0].timestamps.updated_at = now().unwrap();
    queue.tasks[0].timestamps.completed_at = Some(queue.tasks[0].timestamps.updated_at.clone());
    write_queue(&fixture, &queue);
    let later_queue = fs::read(fixture.component().join("TODOS.yaml")).unwrap();
    let returned = fixture.apply(&plan).unwrap();
    assert_eq!(returned.id, plan.id);
    assert_eq!(
        fs::read(fixture.component().join("TODOS.yaml")).unwrap(),
        later_queue
    );
    assert_eq!(
        fs::read_to_string(source_path).unwrap(),
        "pub fn subsequent_implementation() {}\n"
    );
    assert_eq!(fs::read(receipt_path).unwrap(), receipt);
}
