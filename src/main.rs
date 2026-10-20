use std::process::ExitCode;

fn main() -> ExitCode {
    match flowgen::cli::main() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("flowgen: error: {e:#}");
            ExitCode::from(2)
        }
    }
}
