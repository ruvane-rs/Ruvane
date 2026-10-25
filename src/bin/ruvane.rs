#![forbid(unsafe_code)]

mod cli;

fn main() -> std::process::ExitCode {
    match cli::run(std::env::args_os().skip(1)) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ruvane: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
