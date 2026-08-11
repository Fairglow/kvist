use std::process::ExitCode;

fn main() -> ExitCode {
    match kvist::run() {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            let exit_code = error.exit_code();
            if let Err(report_error) = error.print() {
                eprintln!("error: failed to report command failure: {report_error}");
                return ExitCode::FAILURE;
            }

            ExitCode::from(exit_code)
        }
    }
}
