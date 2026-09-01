use std::process::Command;

#[test]
fn completions_subcommand_generates_script() {
    let output = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args(["completions", "bash"])
        .output()
        .expect("run completions command");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 completions script");
    assert!(stdout.contains("complete -F _kvist kvist") || stdout.contains("kvist"));
}
