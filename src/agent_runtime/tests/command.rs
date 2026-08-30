use std::path::{Path, PathBuf};

use agent_runtime::render_command;

#[test]
fn renders_prompt_context_and_target_without_a_shell() {
    let context = vec![
        PathBuf::from("/workspace/SPEC.md"),
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
            "/workspace/SPEC.md",
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
    let context = vec![PathBuf::from("/workspace/SPEC.md")];
    let (_, arguments) = render_command(
        "agent '--request={prompt}:{target_directory}:{context_files}'",
        "review",
        &context,
        Path::new("/workspace"),
    )
    .expect("render mixed placeholders");

    assert_eq!(
        arguments,
        ["--request=review:/workspace:/workspace/SPEC.md"]
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
