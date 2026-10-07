use std::process::Command;

fn run_kvist(arguments: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args(arguments)
        .output()
        .expect("run kvist command")
}

/// No topic renders the guided tour with the closed topic list.
#[test]
fn help_without_topic_renders_tour_listing_topics() {
    let output = run_kvist(&["help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 help output");
    for expected in [
        "kvist help concepts",
        "kvist help lifecycle",
        "kvist help task-states",
        "kvist --help",
        "GUIDE.md",
        "docs/command-set.md",
        "component accept",
    ] {
        assert!(stdout.contains(expected), "tour missing `{expected}`");
    }
}

/// The concepts topic explains the four core concepts and artifact set.
#[test]
fn help_concepts_explains_core_concepts() {
    let output = run_kvist(&["help", "concepts"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 help output");
    for expected in [
        "project",
        "component",
        "intent",
        "tasks",
        "ROOT_CONTRACT.md",
        "REQUIREMENTS.md",
        "CONTRACT.md",
        "DESIGN.md",
        "TODOS.yaml",
        "IMPL.md",
        "kvist component accept",
        "kvist task run",
        "GUIDE.md",
    ] {
        assert!(stdout.contains(expected), "concepts missing `{expected}`");
    }
}

/// The lifecycle topic names the exact commands for every stage.
#[test]
fn help_lifecycle_names_exact_stage_commands() {
    let output = run_kvist(&["help", "lifecycle"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 help output");
    for expected in [
        "kvist component new",
        "kvist component validate",
        "kvist component accept",
        "kvist task run",
        "kvist task finalize",
        "STALE",
        "kvist status",
        "kvist help task-states",
        "GUIDE.md",
    ] {
        assert!(stdout.contains(expected), "lifecycle missing `{expected}`");
    }
}

/// The task-states topic explains every state, the legal transitions, and the
/// command that achieves each.
#[test]
fn help_task_states_explains_states_transitions_and_commands() {
    let output = run_kvist(&["help", "task-states"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 help output");
    for expected in [
        "pending",
        "in-progress",
        "blocked",
        "awaiting-decision",
        "completed",
        "Legal transitions",
        "kvist task run",
        "kvist task transition",
        "kvist task finalize",
        "kvist task approve-policy",
        "blocked_reason",
        "GUIDE.md",
    ] {
        assert!(
            stdout.contains(expected),
            "task-states missing `{expected}`"
        );
    }
    // Every non-terminal state lists its legal targets.
    for line in [
        "pending            -> in-progress | blocked",
        "in-progress        -> pending | blocked | completed | awaiting-decision",
        "blocked            -> pending | in-progress",
        "awaiting-decision  -> pending | in-progress",
        "completed          -> (none; terminal)",
    ] {
        assert!(
            stdout.contains(line),
            "task-states missing transition line `{line}`"
        );
    }
}

/// Unknown topics fail with the closed set spelled out.
#[test]
fn help_unknown_topic_reports_the_closed_set() {
    let output = run_kvist(&["help", "bogus"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 stderr");
    for expected in ["concepts", "lifecycle", "task-states"] {
        assert!(
            stderr.contains(expected),
            "error missing topic `{expected}`"
        );
    }
}
