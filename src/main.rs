use std::env;
use std::process;
use cli::args::CliArgs;
use cli::args::Command;
use cli::parser::CliParser;
use commands::commit::CommitCommand;
use commands::init::InitCommand;

mod cli;
mod commands;
mod validators;
mod errors;
mod core;

fn main() {
    let args: Vec<String> = env::args().collect();

    match CliParser::parse(args) {
        Ok(cli_args) => handle_command(cli_args),
        Err(e) => exit_with_error(&e.to_string()),
    }
}

fn handle_command(cli_args: CliArgs) {
    match cli_args.command {
        Command::Init { path } => handle_init_command(&path),
        Command::Commit { message } => handle_commit_command(&message),
        Command::Unknown { name } => exit_with_error(&format!("'{}' is not a ash command", name)),
    }
}

fn handle_commit_command(message: &str) {
    match CommitCommand::execute(message) {
        Ok(_) => process::exit(0),
        Err(e) => exit_with_error(&format!("fatal: {}", e)),
    }
}

fn handle_init_command(path: &str) {
    match InitCommand::execute(path) {
        Ok(_) => process::exit(0),
        Err(e) => exit_with_error(&format!("fatal: {}", e)),
    }
}

fn exit_with_error(message: &str) -> ! {
    eprintln!("{}", message);
    if message.contains("Usage") {
        eprintln!("{}", CliParser::format_help());
    }
    process::exit(1);
}   