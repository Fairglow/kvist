use std::path::{Path, PathBuf};

use sav::{
    ReasoningEffort, render_command, render_command_with_reasoning_effort, split_raw_command,
};

#[test]
fn renders_prompt_context_and_target_without_a_shell() {
    let context = vec![
        PathBuf::from("/workspace/REQUIREMENTS.md"),
        PathBuf::from("/workspace/TODOS.yaml"),
    ];

    let (program, arguments) = render_command(
        "agent --message '{prompt}' --files {context_files} --dir '{target_directory}'",
        "Review this contract",
        &context,
        Path::new("/workspace"),
    )
    .expect("render command");

    assert_eq!(program, "agent");
    assert_eq!(
        arguments,
        [
            "--message",
            "Review this contract",
            "--files",
            "/workspace/REQUIREMENTS.md",
            "/workspace/TODOS.yaml",
            "--dir",
            "/workspace",
        ]
    );
}

#[test]
fn removes_option_before_an_empty_context_placeholder() {
    let (program, arguments) = render_command(
        "agent --prompt '{prompt}' --files '{context_files}'",
        "hello",
        &[],
        Path::new("."),
    )
    .expect("render command");

    assert_eq!(program, "agent");
    assert_eq!(arguments, ["--prompt", "hello"]);
}

#[test]
fn rejects_unterminated_quotes() {
    let error = render_command("agent --prompt 'unfinished", "hello", &[], Path::new("."))
        .expect_err("reject malformed template");

    assert!(error.to_string().contains("unterminated"));
}

#[test]
fn renders_all_placeholders_in_a_repeated_context_argument() {
    let context = vec![PathBuf::from("/workspace/REQUIREMENTS.md")];
    let (_, arguments) = render_command(
        "agent '--request={prompt}:{target_directory}:{context_files}'",
        "review",
        &context,
        Path::new("/workspace"),
    )
    .expect("render mixed placeholders");

    assert_eq!(
        arguments,
        ["--request=review:/workspace:/workspace/REQUIREMENTS.md"]
    );
}

#[test]
fn renders_a_prompt_as_a_complete_json_string() {
    let (_, arguments) = render_command(
        "curl --json '{\"content\":{prompt_json}}'",
        "Say \"hello\"\\again\nnext",
        &[],
        Path::new("."),
    )
    .expect("render JSON prompt");

    assert_eq!(
        arguments,
        [
            "--json",
            "{\"content\":\"Say \\\"hello\\\"\\\\again\\nnext\"}"
        ]
    );
}

#[test]
fn reasoning_effort_requires_and_renders_an_explicit_placeholder() {
    let (_, arguments) = render_command_with_reasoning_effort(
        "copilot --reasoning-effort '{reasoning_effort}' --prompt '{prompt}'",
        "review",
        &[],
        Path::new("."),
        Some(ReasoningEffort::High),
    )
    .expect("render reasoning effort");

    assert_eq!(
        arguments,
        ["--reasoning-effort", "high", "--prompt", "review"]
    );

    let (_, arguments) = render_command(
        "copilot --reasoning-effort '{reasoning_effort}' --prompt '{prompt}'",
        "review",
        &[],
        Path::new("."),
    )
    .expect("remove absent reasoning effort");
    assert_eq!(arguments, ["--prompt", "review"]);

    let error = render_command_with_reasoning_effort(
        "copilot --prompt '{prompt}'",
        "review",
        &[],
        Path::new("."),
        Some(ReasoningEffort::High),
    )
    .expect_err("reject ignored reasoning effort");
    assert!(error.to_string().contains("{reasoning_effort}"));
}

#[test]
fn rejects_program_tokens_with_control_bytes() {
    let error = render_command(
        "agent\x00 --prompt '{prompt}'",
        "hello",
        &[],
        Path::new("."),
    )
    .expect_err("NUL in program");
    assert!(error.to_string().contains("control bytes"));
}

#[test]
fn rejects_program_tokens_exceeding_the_bound() {
    let long_program = "a".repeat(4097);
    let error = render_command(
        &format!("{long_program} --prompt '{{prompt}}'"),
        "hello",
        &[],
        Path::new("."),
    )
    .expect_err("oversized program");
    assert!(error.to_string().contains("4096"));
}

#[test]
fn rejects_control_byte_program_in_split_raw_command() {
    let error =
        split_raw_command("agent\x1b --prompt '{prompt}'").expect_err("control byte program");
    assert!(error.to_string().contains("control bytes"));
}
