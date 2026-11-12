use crossterm::event::{KeyCode, KeyModifiers};

use super::{
    Overlay, Workspace,
    menus::{Action, Entry, Menu, Page},
};

fn menu(workspace: &Workspace) -> &Menu {
    let Some(Overlay::Menu(menu)) = &workspace.overlay else {
        panic!("expected menu");
    };
    menu
}

#[test]
fn recalled_status_prefills_focus_and_observes_explicit_values_before_deletion() {
    let dir = tempfile::tempdir().unwrap();
    let mut workspace = Workspace::new(dir.path()).unwrap();
    *workspace.focus.lock().unwrap() = Some("maerg".into());
    workspace.history = vec!["status".into()];
    workspace.recall(true);
    assert_eq!(workspace.input.text, "status maerg ");

    workspace.prefill.begin_submission();
    workspace.history_index = None;
    workspace.history = vec!["status beta".into()];
    workspace.recall(true);
    assert_eq!(workspace.input.text, "status beta");
    workspace.input.set("status");
    workspace.input.prefill(&workspace.prefill);
    assert_eq!(workspace.input.text, "status");
}

#[test]
fn escape_dismisses_completion_before_opening_the_menu_and_preserves_input() {
    let dir = tempfile::tempdir().unwrap();
    let mut workspace = Workspace::new(dir.path()).unwrap();
    workspace.input.set("sta");
    workspace.candidates = crate::bark::completion::KvistCompleter::resolve(
        &workspace.tree,
        &workspace.state,
        None,
        "sta",
        3,
    );
    assert!(!workspace.candidates.is_empty());
    workspace.escape().unwrap();
    assert!(workspace.overlay.is_none());
    assert!(workspace.candidates.is_empty());
    workspace.escape().unwrap();
    assert_eq!(menu(&workspace).page, Page::Root);
    workspace.escape().unwrap();
    assert!(workspace.overlay.is_none());
    assert_eq!(workspace.input.text, "sta");
}

#[test]
fn filtered_component_picker_and_pin_preserve_parent_and_command_draft() {
    let dir = tempfile::tempdir().unwrap();
    crate::init::initialize(dir.path()).unwrap();
    let mut workspace = Workspace::new(dir.path()).unwrap();
    workspace.input.set("task next ");
    workspace.open_menu(Page::Root).unwrap();
    workspace.overlay_key(KeyCode::Enter.into()).unwrap();
    assert_eq!(menu(&workspace).page, Page::Components);
    workspace.paste(".");
    let selected = menu(&workspace).visible()[menu(&workspace).selected];
    assert!(matches!(
        menu(&workspace).choices[selected].action,
        Action::Focus(Some(_))
    ));
    workspace
        .overlay_key(crossterm::event::KeyEvent::new(
            KeyCode::Char('p'),
            KeyModifiers::CONTROL,
        ))
        .unwrap();
    assert!(
        menu(&workspace)
            .choices
            .iter()
            .any(|entry| entry.label.contains("[pinned]"))
    );
    assert_eq!(menu(&workspace).query, ".");
    workspace.escape().unwrap();
    assert_eq!(menu(&workspace).page, Page::Root);
    assert_eq!(workspace.input.text, "task next ");
    assert!(
        super::preferences::Preferences::load(dir.path())
            .unwrap()
            .pinned
            .contains(&".".into())
    );
}

#[test]
fn paste_is_routed_to_menu_search_and_never_mutates_hidden_command_input() {
    let dir = tempfile::tempdir().unwrap();
    let mut workspace = Workspace::new(dir.path()).unwrap();
    workspace.input.set("status");
    workspace.open_menu(Page::Root).unwrap();
    workspace.paste("quit\n");
    assert_eq!(workspace.input.text, "status");
    workspace.open_menu(Page::Components).unwrap();
    workspace.paste("some\ncomponent\x1b");
    assert_eq!(menu(&workspace).query, "some component");
    workspace.paste(&"x".repeat(256));
    assert_eq!(menu(&workspace).query, "some component");
    assert!(workspace.notice.contains("256"));
    assert_eq!(workspace.input.text, "status");
}

#[test]
fn quit_menu_uses_the_same_idle_exit_path() {
    let dir = tempfile::tempdir().unwrap();
    let mut workspace = Workspace::new(dir.path()).unwrap();
    workspace.open_menu(Page::Root).unwrap();
    workspace.overlay_key(KeyCode::End.into()).unwrap();
    assert_eq!(
        menu(&workspace).choices[menu(&workspace).selected].label,
        "Quit"
    );
    workspace.overlay_key(KeyCode::Enter.into()).unwrap();
    assert!(workspace.quit);
}

#[test]
fn direct_theme_picker_returns_through_the_same_appearance_hierarchy() {
    let dir = tempfile::tempdir().unwrap();
    let mut workspace = Workspace::new(dir.path()).unwrap();
    workspace.open_menu(Page::Theme).unwrap();
    workspace.escape().unwrap();
    assert_eq!(menu(&workspace).page, Page::Appearance);
    workspace.escape().unwrap();
    assert_eq!(menu(&workspace).page, Page::Root);
    assert_eq!(
        menu(&workspace).choices[menu(&workspace).selected].label,
        "Appearance"
    );
    workspace.escape().unwrap();
    assert!(workspace.overlay.is_none());
}

#[test]
fn current_theme_marker_remains_when_highlight_moves() {
    let dir = tempfile::tempdir().unwrap();
    let mut workspace = Workspace::new(dir.path()).unwrap();
    workspace.set_theme("light").unwrap();
    workspace.open_menu(Page::Theme).unwrap();
    workspace.overlay_key(KeyCode::Home.into()).unwrap();
    assert_eq!(menu(&workspace).selected, 0);
    assert!(
        menu(&workspace)
            .choices
            .iter()
            .any(|entry| entry.current && entry.label == "light")
    );
    assert!(!menu(&workspace).choices[0].current);
}

#[test]
fn stale_cancel_never_reports_a_cancellation_or_changes_the_menu_selection() {
    let dir = tempfile::tempdir().unwrap();
    let mut workspace = Workspace::new(dir.path()).unwrap();
    workspace.overlay = Some(Overlay::Menu(Menu::new(
        Page::Root,
        "Workspace",
        vec![
            Entry::new("Cancel command", Action::Cancel),
            Entry::new("Quit", Action::Quit),
        ],
    )));
    let error = workspace.overlay_key(KeyCode::Enter.into()).unwrap_err();
    assert!(error.to_string().contains("already finished"));
    assert_eq!(menu(&workspace).selected, 0);
    assert!(!workspace.quit);
    assert!(!workspace.notice.contains("Cancellation requested"));
}

#[test]
fn failed_focus_keeps_the_component_picker_and_command_draft() {
    let dir = tempfile::tempdir().unwrap();
    let mut workspace = Workspace::new(dir.path()).unwrap();
    workspace.input.set("status");
    workspace.overlay = Some(Overlay::Menu(Menu::new(
        Page::Components,
        "Components",
        vec![Entry::new("Missing", Action::Focus(Some("missing".into())))],
    )));
    assert!(workspace.overlay_key(KeyCode::Enter.into()).is_err());
    assert_eq!(menu(&workspace).page, Page::Components);
    assert_eq!(workspace.input.text, "status");
    assert!(!workspace.quit);
}

#[test]
fn f2_opens_the_root_from_help_details_and_quit_without_changing_the_draft() {
    let dir = tempfile::tempdir().unwrap();
    let mut workspace = Workspace::new(dir.path()).unwrap();
    workspace.input.set("status");
    for overlay in [
        Overlay::Help { scroll: 0 },
        Overlay::Details {
            title: "Details".into(),
            markdown: "# Details".into(),
            scroll: 0,
        },
        Overlay::Quit,
    ] {
        workspace.overlay = Some(overlay);
        workspace.overlay_key(KeyCode::F(2).into()).unwrap();
        assert_eq!(menu(&workspace).page, Page::Root);
        assert_eq!(workspace.input.text, "status");
        assert!(!workspace.quit);
        assert!(workspace.running.is_none());
    }
}

#[test]
fn f2_cancels_staged_confirmation_without_authorizing_or_retaining_its_input() {
    let dir = tempfile::tempdir().unwrap();
    let mut workspace = Workspace::new(dir.path()).unwrap();
    let file = tempfile::NamedTempFile::new().unwrap();
    let path = file.path().to_owned();
    workspace.prompt_file = Some(file);
    workspace.pending_page = Some(Page::Models);
    workspace.overlay = Some(Overlay::Confirm {
        prompt: "Submit authored prompt".into(),
        arguments: vec!["--help".into()],
        scroll: 0,
    });
    workspace.overlay_key(KeyCode::F(2).into()).unwrap();
    assert_eq!(menu(&workspace).page, Page::Root);
    assert!(workspace.pending_page.is_none());
    assert!(workspace.prompt_file.is_none());
    assert!(!path.exists());
    workspace.overlay_key(KeyCode::Char('y').into()).unwrap();
    assert!(workspace.running.is_none());
}

#[test]
fn active_commands_refuse_persisted_theme_and_shortcut_changes() {
    let dir = tempfile::tempdir().unwrap();
    crate::init::initialize(dir.path()).unwrap();
    let mut workspace = Workspace::new(dir.path()).unwrap();
    workspace.set_theme("dark").unwrap();
    let preference = std::fs::read(dir.path().join(".kvist/theme")).unwrap();
    workspace.start(vec!["--help".into()]).unwrap();
    workspace.open_menu(Page::Theme).unwrap();
    workspace.overlay_key(KeyCode::Home.into()).unwrap();
    workspace.overlay_key(KeyCode::Down.into()).unwrap();
    assert!(workspace.overlay_key(KeyCode::Enter.into()).is_err());
    assert_eq!(workspace.theme.name(), "dark");
    assert_eq!(
        std::fs::read(dir.path().join(".kvist/theme")).unwrap(),
        preference
    );
    workspace.open_menu(Page::Components).unwrap();
    workspace.paste(".");
    assert!(
        workspace
            .overlay_key(crossterm::event::KeyEvent::new(
                KeyCode::Char('p'),
                KeyModifiers::CONTROL
            ))
            .is_err()
    );
    assert!(workspace.preferences.pinned.is_empty());
    assert!(!dir.path().join(".kvist/transcript/workspace.json").exists());
    assert_eq!(menu(&workspace).page, Page::Components);
    workspace.request_quit();
    workspace.overlay_key(KeyCode::Char('y').into()).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while workspace.running.is_some() {
        workspace.pump().unwrap();
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn quitting_active_work_requires_confirmation_and_reaps_the_owned_child() {
    let dir = tempfile::tempdir().unwrap();
    let mut workspace = Workspace::new(dir.path()).unwrap();
    workspace.start(vec!["--help".into()]).unwrap();
    workspace.open_menu(Page::Root).unwrap();
    workspace.request_quit();
    assert!(matches!(workspace.overlay, Some(Overlay::Quit)));
    assert!(!workspace.quit);
    workspace.overlay_key(KeyCode::Esc.into()).unwrap();
    assert_eq!(menu(&workspace).page, Page::Root);
    assert!(workspace.running.is_some());
    workspace.request_quit();
    workspace.overlay_key(KeyCode::Char('y').into()).unwrap();
    assert!(workspace.quit);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while workspace.running.is_some() {
        workspace.pump().unwrap();
        assert!(
            std::time::Instant::now() < deadline,
            "owned command was not reaped"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn task_run_auto_selects_the_next_ready_task_from_the_canonical_queue() {
    // Regression: `task run <component>` (with no task id) used to read
    // `<component>/.kvist/TODOS.yaml`, a path that does not exist, so it always
    // reported "no ready task found" even while `kvist status` correctly showed
    // a ready task. It must read the canonical `<component>/TODOS.yaml` and
    // agree with the shared `next_ready_task_id` definition.
    const READY_QUEUE: &str = r#"schema_version: 1
component:
  requirements_revision: sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  contract_revision: sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
  design_revision: sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc
  parent_contract: null
  revalidation:
    state: current
    checked_at: 2026-08-13T20:19:53Z
    stale_since: null
    causes: []
tasks:
  - id: write-tests
    title: Write tests
    description: Define tests.
    context: Context.
    purpose: Purpose.
    expected_outcome: Outcome.
    kind: test
    status: completed
    depends_on: []
    requirements: []
    timestamps:
      created_at: 2026-08-13T20:19:53Z
      updated_at: 2026-08-13T20:19:53Z
      completed_at: 2026-08-13T20:19:53Z
    blocked_reason: null
  - id: implement-code
    title: Implement
    description: Implement.
    context: Context.
    purpose: Purpose.
    expected_outcome: Outcome.
    kind: implementation
    status: pending
    depends_on:
      - write-tests
    requirements: []
    timestamps:
      created_at: 2026-08-13T20:19:53Z
      updated_at: 2026-08-13T20:19:53Z
      completed_at: null
    blocked_reason: null
"#;
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path()).unwrap();
    let component = dir.path().join("blad");
    std::fs::create_dir_all(&component).unwrap();
    // The queue lives at the component root, matching every other reader
    // (`status`, `kvist task run`, `kvist repair`).
    std::fs::write(component.join("TODOS.yaml"), READY_QUEUE).unwrap();

    // `write-tests` is completed, so `implement-code` (whose only dependency is
    // done) is the next ready task and must be auto-selected.
    assert_eq!(workspace.next_ready_task("blad"), Some("implement-code".to_owned()));

    // A component whose queue has no ready task yields `None` (the caller then
    // reports the actionable "specify a task or make one ready" error).
    let empty = dir.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let empty_queue = READY_QUEUE.replace("status: pending", "status: blocked");
    std::fs::write(empty.join("TODOS.yaml"), empty_queue).unwrap();
    assert_eq!(workspace.next_ready_task("empty"), None);

    // And a component with no queue at all is also `None`, not an error.
    assert_eq!(workspace.next_ready_task("missing"), None);
}
