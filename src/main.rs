mod input;
mod portal;

use std::process::ExitCode;

fn main() -> ExitCode {
    let (commands, receiver) = calloop::channel::channel();
    let connection = match portal::serve(commands) {
        Ok(connection) => connection,
        Err(error) => {
            eprintln!("hyprland-remote-portal: cannot serve the portal: {error}");
            return ExitCode::FAILURE;
        }
    };
    let result = input::run(receiver);
    drop(connection);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("hyprland-remote-portal: {error}");
            ExitCode::FAILURE
        }
    }
}
