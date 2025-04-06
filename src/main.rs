// src/main.rs
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
use commands::reset::ResetCommand;
use commands::status::StatusCommand;
use commands::branch::BranchCommand;
use commands::merge::MergeCommand;

// --- Imports pentru Logging ---
use env_logger::Builder;
use log::{LevelFilter, info}; // Eliminăm 'error' de aici, îl vom folosi doar unde e necesar
use std::fs::OpenOptions;
// --- Sfârșit Imports Logging ---

mod cli;
mod commands;
mod validators;
mod errors;
mod core;

fn main() {
    // --- Inițializare Logging ---
    let mut builder = Builder::from_default_env();
    if let Ok(log_path) = env::var("ASH_LOG_FILE") {
        match OpenOptions::new().create(true).append(true).open(&log_path) {
            Ok(file) => {
                builder.target(env_logger::Target::Pipe(Box::new(file)));
                eprintln!("[Logging detailed output to: {}]", log_path);
            }
            Err(e) => {
                eprintln!("Warning: Could not open or create log file '{}': {}. Logging to stderr.", log_path, e);
                builder.target(env_logger::Target::Stderr);
            }
        }
    } else {
        builder.target(env_logger::Target::Stderr);
    }
    builder.filter_level(LevelFilter::Info);
    // builder.filter_module("AsheraFlow", LevelFilter::Trace);
    builder.init();
    info!("AsheraFlow application starting...");
    // --- Sfârșit Inițializare Logging ---

    let args: Vec<String> = env::args().collect();

    match CliParser::parse(args) {
        Ok(cli_args) => handle_command(cli_args),
        Err(e) => {
             // Logarea erorii de parsare poate fi utilă aici
             log::error!("CLI parsing failed: {}", e); // Folosim log::error direct
             exit_with_error(&e.to_string())
        },
    }
}

fn handle_command(cli_args: CliArgs) {
    info!("Handling command: {:?}", cli_args.command);
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
        Command::Merge { branch, message, abort, continue_merge } => {
                if abort {
                    info!("Merge abort requested.");
                    println!("Merge abort functionality not fully implemented yet.");
                    process::exit(1);
                } else if continue_merge {
                    info!("Merge continue requested.");
                    println!("Merge continue functionality not fully implemented yet.");
                    process::exit(1);
                } else {
                    info!("Handling normal merge for branch '{}'", branch);
                    handle_merge_command(&branch, message.as_deref())
                }
            },
        Command::Reset { revision, paths, soft, mixed, hard } => {
            handle_reset_command(&revision, &paths, soft, mixed, hard)
        },
        Command::Unknown { name } => {
             // Logarea poate fi utilă
             log::error!("Unknown command received: {}", name);
             exit_with_error(&format!("'{}' is not a ash command", name))
        },
    }
}


fn handle_commit_command(message: &str) {
     info!("Executing CommitCommand with message: '{}'", message);
    match CommitCommand::execute(message) {
        Ok(_) => process::exit(0),
        Err(e) => {
             // Nu mai logăm aici, lăsăm exit_with_error să afișeze mesajul simplu
             exit_with_error(&e.to_string())
        },
    }
}

fn handle_init_command(path: &str) {
    info!("Executing InitCommand for path: '{}'", path);
    match InitCommand::execute(path) {
        Ok(_) => process::exit(0),
        Err(e) => {
             // Nu mai logăm aici
             exit_with_error(&e.to_string())
        },
    }
}

fn handle_add_command(paths: &[String]) {
     info!("Executing AddCommand for paths: {:?}", paths);
    match AddCommand::execute(paths) {
        Ok(_) => process::exit(0),
        Err(e) => {
             // Nu mai logăm aici
             exit_with_error(&e.to_string())
        },
    }
}

fn handle_status_command(porcelain: bool, color: &str) {
     info!("Executing StatusCommand (porcelain: {}, color: {})", porcelain, color);
    std::env::set_var("ASH_COLOR", color);
    match StatusCommand::execute(porcelain) {
        Ok(_) => process::exit(0),
        Err(e) => {
             // Nu mai logăm aici
             exit_with_error(&e.to_string())
        },
    }
}

fn handle_diff_command(paths: &[String], cached: bool) {
     info!("Executing DiffCommand (cached: {}, paths: {:?})", cached, paths);
    match DiffCommand::execute(paths, cached) {
        Ok(_) => process::exit(0),
        Err(e) => {
             // Nu mai logăm aici
             exit_with_error(&e.to_string())
        },
    }
}

fn handle_branch_command(name: &str, start_point: Option<&str>, verbose: bool, delete: bool, force: bool) {
    info!(
        "Executing BranchCommand (name: '{}', start: {:?}, verbose: {}, delete: {}, force: {})",
        name, start_point, verbose, delete, force
    );
    if verbose { std::env::set_var("ASH_BRANCH_VERBOSE", "1"); }
    if delete { std::env::set_var("ASH_BRANCH_DELETE", "1"); }
    if force { std::env::set_var("ASH_BRANCH_FORCE", "1"); }
    match BranchCommand::execute(name, start_point) {
        Ok(_) => process::exit(0),
        Err(e) => {
             // Nu mai logăm aici
             exit_with_error(&e.to_string())
        },
    }
}


fn handle_log_command(revisions: &[String], abbrev: bool, format: &str, patch: bool, decorate: &str) {
     info!("Executing LogCommand (revisions: {:?}, options: abbrev={}, format={}, patch={}, decorate={})",
           revisions, abbrev, format, patch, decorate);
    let mut options = HashMap::new();
    options.insert("abbrev".to_string(), abbrev.to_string());
    options.insert("format".to_string(), format.to_string());
    options.insert("patch".to_string(), patch.to_string());
    options.insert("decorate".to_string(), decorate.to_string());
    match LogCommand::execute(revisions, &options) {
        Ok(_) => process::exit(0),
        Err(e) => {
             // Nu mai logăm aici
             exit_with_error(&e.to_string())
        },
    }
}

fn handle_checkout_command(target: &str) {
    info!("Executing CheckoutCommand for target: '{}'", target);
    match CheckoutCommand::execute(target) {
        Ok(_) => process::exit(0),
        Err(e) => {
             // Nu mai logăm aici
             exit_with_error(&e.to_string())
        },
    }
}

fn handle_merge_command(branch: &str, message: Option<&str>) {
     info!("Executing MergeCommand for branch '{}' with message: {:?}", branch, message);
    match MergeCommand::execute(branch, message) {
        Ok(_) => {
             info!("Merge completed successfully.");
            process::exit(0);
        }
        Err(e) => {
            let error_message = e.to_string();
            if error_message == "Already up to date." {
                info!("Merge resulted in 'Already up to date.'");
                println!("{}", error_message); // Afișăm mesajul standard
                process::exit(0);
            }
            else if error_message.contains("fix conflicts") ||
                    error_message.contains("Automatic merge failed") ||
                    error_message.contains("untracked working tree files would be overwritten by merge") ||
                    error_message.contains("Your local changes to the following files would be overwritten by merge")
            {
                 // Logăm eroarea specifică intern, dar afișăm mesajul simplu
                 log::error!("Merge failed: {}", error_message);
                exit_with_error(&error_message);
            }
            else {
                 // Logăm eroarea generală intern
                 log::error!("Merge command failed generally: {}", error_message);
                 // Păstrăm prefixul distinctiv pentru erori generale de merge, dar fără "fatal:"
                 exit_with_error(&format!("merge failed: {}", error_message));
            }
        }
    }
}

fn handle_reset_command(revision: &str, paths: &[String], soft: bool, mixed: bool, hard: bool)  {
    info!("Executing ResetCommand with revision: {:?}, paths: {:?}",revision, paths);
    match  ResetCommand::execute(revision, paths, soft, mixed, hard)  {
        Ok(_) => process::exit(0),
        Err(e) => {
             exit_with_error(&e.to_string())
        },
    }
}

// exit_with_error rămâne la fel
fn exit_with_error(message: &str) -> ! {
    eprintln!("{}", message); // Afișează mesajul simplu pe stderr
    process::exit(1);
}