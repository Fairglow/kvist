use clap::Parser;
use kvist::cli::{Cli, Command, TaskCommand};

#[test]
fn task_controls_are_an_explicit_opt_in_in_both_output_modes() {
    for prefix in [vec!["kvist"], vec!["kvist", "--json"]] {
        let mut args = prefix.clone();
        args.extend([
            "task",
            "run",
            "component",
            "fixture-task",
            "--allow-control",
        ]);
        let cli = Cli::try_parse_from(args).expect("explicit control flag");
        assert!(matches!(
            cli.command,
            Some(Command::Task {
                command: TaskCommand::Run {
                    allow_control: true,
                    ..
                }
            })
        ));
        let mut args = prefix;
        args.extend(["task", "run", "component", "fixture-task"]);
        let cli = Cli::try_parse_from(args).expect("default read-only observation");
        assert!(matches!(
            cli.command,
            Some(Command::Task {
                command: TaskCommand::Run {
                    allow_control: false,
                    ..
                }
            })
        ));
    }
}
