use std::collections::HashMap;
use std::env;
use std::process;
use cli::args::CliArgs;
use cli::args::Command;
use cli::parser::CliParser;
use commands::checkout::CheckoutCommand;
use commands::commit::CommitCommand;
use commands::diff::DiffCommand;
use commands::init::InitCommand;
use commands::add::AddCommand;
use commands::log::LogCommand;
use commands::status::StatusCommand;
use commands::branch::BranchCommand;

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
        Command::Add { paths } => handle_add_command(&paths),
        Command::Status { porcelain, color } => handle_status_command(porcelain, &color),
        Command::Diff { paths, cached } => handle_diff_command(&paths, cached),
        Command::Branch { name, start_point, verbose, delete, force } => 
            handle_branch_command(&name, start_point.as_deref(), verbose, delete, force),
        Command::Checkout { target } => handle_checkout_command(&target),
        Command::Log { revisions, abbrev, format, patch, decorate } => 
            handle_log_command(&revisions, abbrev, &format, patch, &decorate),
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

fn handle_add_command(paths: &[String]) {
    match AddCommand::execute(paths) {
        Ok(_) => process::exit(0),
        Err(e) => exit_with_error(&format!("fatal: {}", e)),
    }
}

fn handle_status_command(porcelain: bool, color: &str) {
    // Set color mode environment variable
    std::env::set_var("ASH_COLOR", color);
    
    match StatusCommand::execute(porcelain) {
        Ok(_) => process::exit(0),
        Err(e) => exit_with_error(&format!("fatal: {}", e)),
    }
}

fn handle_diff_command(paths: &[String], cached: bool) {
    match DiffCommand::execute(paths, cached) {
        Ok(_) => process::exit(0),
        Err(e) => exit_with_error(&format!("fatal: {}", e)),
    }
}

fn handle_branch_command(name: &str, start_point: Option<&str>, verbose: bool, delete: bool, force: bool) {
    // Set environment variables to pass flag information
    if verbose {
        std::env::set_var("ASH_BRANCH_VERBOSE", "1");
    }
    if delete {
        std::env::set_var("ASH_BRANCH_DELETE", "1");
    }
    if force {
        std::env::set_var("ASH_BRANCH_FORCE", "1");
    }
    
    match BranchCommand::execute(name, start_point) {
        Ok(_) => process::exit(0),
        Err(e) => exit_with_error(&format!("fatal: {}", e)),
    }
}


fn handle_log_command(revisions: &[String], abbrev: bool, format: &str, patch: bool, decorate: &str) {
    // Convert options to HashMap for easier handling
    let mut options = HashMap::new();
    options.insert("abbrev".to_string(), abbrev.to_string());
    options.insert("format".to_string(), format.to_string());
    options.insert("patch".to_string(), patch.to_string());
    options.insert("decorate".to_string(), decorate.to_string());
    
    match LogCommand::execute(revisions, &options) {
        Ok(_) => process::exit(0),
        Err(e) => exit_with_error(&format!("fatal: {}", e)),
    }
}

fn handle_checkout_command(target: &str) {
    match CheckoutCommand::execute(target) {
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