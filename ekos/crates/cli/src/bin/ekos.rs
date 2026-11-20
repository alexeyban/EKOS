//! The public `ekos` binary: the whole CLI with no out-of-tree extensions (RFC 0149).

use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    match ekos::app::main_with(ekos::extension::Extensions::none()).await {
        Ok(()) => ExitCode::SUCCESS,
        // What returning the error from `main` prints, in ASCII on Windows (`ekos::ascii`).
        Err(e) => {
            eprintln!("Error: {}", ekos::ascii::console(&format!("{e:?}")));
            ExitCode::FAILURE
        }
    }
}
