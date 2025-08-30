use std::collections::HashMap;
use std::env;
use std::process;
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
use commands::merge::MergeCommand;
use commands::merge_tool::MergeToolCommand;
use commands::rm::RmCommand;
use commands::reset::ResetCommand;
use commands::sprint::{
    SprintStartCommand, SprintInfoCommand, SprintCommitMapCommand,
    SprintBurndownCommand, SprintVelocityCommand, SprintAdvanceCommand,
    SprintViewCommand,
};
use commands::task::task_create::TaskCreateCommand;
use commands::task::task_complete::TaskCompleteCommand;
use commands::task::task_status::TaskStatusCommand;
use commands::task::task_list::TaskListCommand;
use std::path::Path;
use crate::core::index::index::Index;
use crate::core::refs::Refs;
use crate::errors::error::Error;
use crate::core::repository::repository::Repository;
use crate::core::database::database::Database;
use commands::commit_writer::CommitWriter;
use crate::core::repository::pending_commit::PendingCommitType;
use commands::commit::get_editor_command;
use commands::cherry_pick::CherryPickCommand;
use commands::revert::RevertCommand;

mod cli;
mod commands;
mod validators;
mod errors;
mod core;

/// Entry point for the AsheraFlow CLI application.
/// Parses command-line arguments and dispatches to the appropriate command handler.

/// Constant representing the original HEAD reference used in merge operations.
const ORIG_HEAD: &str = "ORIG_HEAD";

fn main() {
    let args: Vec<String> = env::args().collect();

    // Parse CLI arguments and execute the corresponding command
    match CliParser::parse(args) {
        Ok(cli_args) => {
            match cli_args.command {
                Command::Init { path } => handle_init_command(&path),
                Command::Commit { message, amend, reuse_message, edit } => 
                    handle_commit_command(&message, amend, reuse_message, edit),
                Command::Add { paths } => handle_add_command(&paths),
                Command::Status { porcelain, color } => handle_status_command(porcelain, &color),
                Command::Diff { paths, cached } => handle_diff_command(&paths, cached),
                Command::Branch { name, start_point, verbose, delete, force } => {
                    handle_branch_command(&name, start_point.as_deref(), verbose, delete, force)
                },
                Command::Checkout { target } => handle_checkout_command(&target),
                Command::Log { revisions, abbrev, format, patch, decorate } => {
                    handle_log_command(&revisions, abbrev, &format, patch, &decorate)
                },
                Command::Merge { branch, message, abort, continue_merge, tool } => {
                    if abort {
                        handle_merge_abort_command();
                    } else if continue_merge {
                        match handle_merge_continue_command() {
                            Ok(_) => process::exit(0),
                            Err(e) => exit_with_error(&format!("fatal: {}", e)),
                        }
                    } else if tool.is_some() && branch.is_empty() {
                        handle_merge_tool_command(tool.as_deref());
                    } else {
                        handle_merge_command(&branch, message.as_deref());
                    }
                },
                Command::Rm { files, cached, force, recursive } => {
                    handle_rm_command(&files, cached, force, recursive)
                },
                Command::Reset { files, soft, mixed, hard, force, reuse_message } => {
                    handle_reset_command(&files, soft, mixed, hard, force, reuse_message.as_deref())
                },
                Command::CherryPick { args, r#continue, abort, quit, mainline } => {
                    handle_cherry_pick_command(&args, r#continue, abort, quit, mainline)
                },
                Command::Revert { args, r#continue, abort, quit, mainline } => {
                    handle_revert_command(&args, r#continue, abort, quit, mainline)
                },
                // Sprint management commands
                Command::SprintStart { name, duration } => {
                    handle_sprint_start_command(&name, duration)
                },
                Command::SprintInfo {} => {
                    handle_sprint_info_command()
                },
                Command::SprintCommitMap { sprint_name } => {
                    handle_sprint_commitmap_command(sprint_name.as_deref())
                },
                // Add other sprint command handlers
                Command::SprintBurndown { sprint_name } => {
                    handle_sprint_burndown_command(sprint_name.as_deref())
                },
                Command::SprintVelocity {} => {
                    handle_sprint_velocity_command()
                },
                Command::SprintAdvance { name, start_date, end_date } => {
                    handle_sprint_advance_command(&name, &start_date, &end_date)
                },
                Command::SprintView {} => {
                    handle_sprint_view_command()
                },
                // Task management commands
                Command::TaskCreate { id, description, story_points } => {
                    handle_task_create_command(&id, &description, story_points)
                },
                Command::TaskComplete { id, story_points: _, auto_merge } => {
                    handle_task_complete_command(&id, auto_merge)
                },
                Command::TaskStatus { id } => {
                    handle_task_status_command(&id)
                },
                Command::TaskList { args } => handle_task_list_command(&args),
                Command::Unknown { name } => {
                    println!("Unknown command: {}", name);
                    println!("{}", CliParser::format_help());
                    process::exit(1);
                }
            }
        },
        Err(e) => {
            if e.to_string().contains("Usage:") {
                // Handle the case where no command is given
                println!("{}", e);
            } else {
                println!("Error parsing command: {}", e);
            }
            process::exit(1);
        }
    }
}

/// Handles the 'commit' command, creating a new commit or amending an existing one.
fn handle_commit_command(message: &str, amend: bool, reuse_message: Option<String>, edit: bool) {
    match CommitCommand::execute(message, amend, reuse_message.as_deref(), edit) {
        Ok(_) => process::exit(0),
        Err(e) => exit_with_error(&format!("fatal: {}", e)),
    }
}

/// Handles the 'init' command, initializing a new AsheraFlow repository.
fn handle_init_command(path: &str) {
    match InitCommand::execute(path) {
        Ok(_) => process::exit(0),
        Err(e) => exit_with_error(&format!("fatal: {}", e)),
    }
}

/// Handles the 'add' command, staging files for commit.
fn handle_add_command(paths: &[String]) {
    match AddCommand::execute(paths) {
        Ok(_) => process::exit(0),
        Err(e) => exit_with_error(&format!("fatal: {}", e)),
    }
}

/// Handles the 'status' command, displaying the current state of the working directory and index.
fn handle_status_command(porcelain: bool, color: &str) {
    std::env::set_var("ASH_COLOR", color);
    match StatusCommand::execute(porcelain) {
        Ok(_) => process::exit(0),
        Err(e) => exit_with_error(&format!("fatal: {}", e)),
    }
}

/// Handles the 'diff' command, showing changes between commits, commit and working tree, etc.
fn handle_diff_command(paths: &[String], cached: bool) {
    match DiffCommand::execute(paths, cached) {
        Ok(_) => process::exit(0),
        Err(e) => exit_with_error(&format!("fatal: {}", e)),
    }
}

/// Handles the 'branch' command, managing branches (create, delete, list, etc.).
fn handle_branch_command(name: &str, start_point: Option<&str>, verbose: bool, delete: bool, force: bool) {
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


/// Handles the 'log' command, displaying commit logs with various formatting options.
fn handle_log_command(revisions: &[String], abbrev: bool, format: &str, patch: bool, decorate: &str) {
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

/// Handles the 'checkout' command, switching branches or restoring working tree files.
fn handle_checkout_command(target: &str) {
    match CheckoutCommand::execute(target) {
        Ok(_) => process::exit(0),
        Err(e) => exit_with_error(&format!("fatal: {}", e)),
    }
}

/// Handles the 'merge-tool' command, launching an external merge tool if configured.
fn handle_merge_tool_command(tool: Option<&str>) {
    match MergeToolCommand::execute(tool) {
        Ok(_) => process::exit(0),
        Err(e) => exit_with_error(&format!("fatal: {}", e)),
    }
}

/// Handles the 'merge --continue' operation, resuming a merge, cherry-pick, or revert after conflicts are resolved.
fn handle_merge_continue_command() -> Result<(), Error> {
    println!("Checking for unresolved conflicts...");
    let root_path = Path::new(".");
    let git_path = root_path.join(".ash");
    if !git_path.exists() {
        return Err(Error::Generic("Not an AsheraFlow repository: .ash directory not found".into()));
    }
    let db_path = git_path.join("objects");
    let mut database = Database::new(db_path);
    let index_path = git_path.join("index");
    if !index_path.exists() {
        return Err(Error::Generic("No index file found.".into()));
    }
    let mut index = Index::new(index_path);
    match index.load() {
        Ok(_) => println!("Index loaded successfully"),
        Err(e) => return Err(Error::Generic(format!("Error loading index: {}", e))),
    }
    if index.has_conflict() {
        return Err(Error::Generic(
            "Cannot continue due to unresolved conflicts. Fix conflicts and add the files.".into(),
        ));
    }
    let refs = Refs::new(&git_path);
    let mut commit_writer = CommitWriter::new(
        root_path,
        git_path.clone(),
        &mut database,
        &mut index,
        &refs
    );
    if commit_writer.pending_commit.in_progress(PendingCommitType::Merge) {
        return commit_writer.resume_merge(PendingCommitType::Merge, get_editor_command());
    } else if commit_writer.pending_commit.in_progress(PendingCommitType::CherryPick) {
        return commit_writer.resume_merge(PendingCommitType::CherryPick, get_editor_command());
    } else if commit_writer.pending_commit.in_progress(PendingCommitType::Revert) {
        return commit_writer.resume_merge(PendingCommitType::Revert, get_editor_command());
    } else {
        return Err(Error::Generic(
            "No merge, cherry-pick, or revert in progress. Nothing to continue.".into(),
        ));
    }
}

/// Handles the 'rm' command, removing files from the working tree and/or index.
fn handle_rm_command(files: &[String], cached: bool, force: bool, recursive: bool) {
    match RmCommand::execute(files, cached, force, recursive) {
        Ok(_) => process::exit(0),
        Err(e) => exit_with_error(&format!("fatal: {}", e)),
    }
}

/// Handles the 'reset' command, resetting current HEAD to the specified state.
fn handle_reset_command(files: &[String], soft: bool, mixed: bool, hard: bool, force: bool, reuse_message: Option<&str>) {
    match ResetCommand::execute(files, soft, mixed, hard, force, reuse_message) {
        Ok(_) => process::exit(0),
        Err(e) => exit_with_error(&format!("fatal: {}", e)),
    }
}

/// Handles the 'cherry-pick' command, applying changes from specific commits.
fn handle_cherry_pick_command(commits: &[String], continue_op: bool, abort: bool, quit: bool, mainline: Option<u32>) {
    match CherryPickCommand::execute(commits, continue_op, abort, quit, mainline) {
        Ok(_) => process::exit(0),
        Err(e) => exit_with_error(&format!("fatal: {}", e)),
    }
}

/// Handles the 'revert' command, reverting changes from specific commits.
fn handle_revert_command(commits: &[String], continue_op: bool, abort: bool, quit: bool, mainline: Option<u32>) {
    match RevertCommand::execute(commits, continue_op, abort, quit, mainline) {
        Ok(_) => process::exit(0),
        Err(e) => exit_with_error(&format!("fatal: {}", e)),
    }
}

/// Utility function to print an error message and exit the process with code 1.
fn exit_with_error(message: &str) -> ! {
    eprintln!("{}", message);
    process::exit(1);
}

/// Handles the 'merge' command, merging changes from another branch into the current branch.
fn handle_merge_command(branch: &str, message: Option<&str>) {
    match MergeCommand::execute(branch, message) {
        Ok(_) => process::exit(0),
        Err(e) => {
            if e.to_string().contains("Already up to date") {
                println!("Already up to date.");
                process::exit(0);
            } else if e.to_string().contains("fix conflicts") {
                println!("{}", e);
                println!("Conflicts detected. Fix conflicts and then run 'ash merge --continue'");
                process::exit(1);
            } else {
                exit_with_error(&format!("fatal: {}", e));
            }
        }
    }
}

/// Handles the 'merge --abort' operation, aborting an in-progress merge and resetting to the original state.
fn handle_merge_abort_command() {
    let mut repo = match Repository::new(".") {
        Ok(r) => r,
        Err(e) => exit_with_error(&format!("fatal: {}", e)),
    };
    let git_path = Path::new(".").join(".ash");
    let merge_head_path = git_path.join("MERGE_HEAD");
    if !merge_head_path.exists() {
        exit_with_error("fatal: There is no merge to abort");
    }
    let _ = std::fs::remove_file(merge_head_path);
    let _ = std::fs::remove_file(git_path.join("MERGE_MSG"));
    let orig_head_path = git_path.join(ORIG_HEAD);
    let orig_head = match std::fs::read_to_string(&orig_head_path) {
        Ok(content) => content.trim().to_string(),
        Err(e) => exit_with_error(&format!("fatal: Failed to read ORIG_HEAD: {}", e)),
    };
    match ResetCommand::execute(&[orig_head], false, false, true, true, None) {
        Ok(_) => {
            println!("Merge aborted");
            process::exit(0);
        },
        Err(e) => exit_with_error(&format!("fatal: Failed to reset to ORIG_HEAD: {}", e)),
    }
}

/// Handles the 'sprint start' command, starting a new sprint with the given name and duration.
fn handle_sprint_start_command(name: &str, duration: u32) {
    match SprintStartCommand::execute(name, duration) {
        Ok(_) => process::exit(0),
        Err(e) => exit_with_error(&format!("fatal: {}", e)),
    }
}

/// Handles the 'sprint info' command, displaying information about the current sprint.
fn handle_sprint_info_command() {
    match SprintInfoCommand::execute() {
        Ok(_) => process::exit(0),
        Err(e) => exit_with_error(&format!("fatal: {}", e)),
    }
}

/// Handles the 'sprint commitmap' command, showing commit mapping for a sprint.
fn handle_sprint_commitmap_command(sprint_name: Option<&str>) {
    match SprintCommitMapCommand::execute(sprint_name) {
        Ok(_) => process::exit(0),
        Err(e) => exit_with_error(&format!("fatal: {}", e)),
    }
}

/// Handles the 'sprint burndown' command, displaying the burndown chart for a sprint.
fn handle_sprint_burndown_command(sprint_name: Option<&str>) {
    match SprintBurndownCommand::execute(sprint_name) {
        Ok(_) => process::exit(0),
        Err(e) => exit_with_error(&format!("fatal: {}", e)),
    }
}

/// Handles the 'sprint velocity' command, showing sprint velocity statistics.
fn handle_sprint_velocity_command() {
    match SprintVelocityCommand::execute() {
        Ok(_) => process::exit(0),
        Err(e) => exit_with_error(&format!("fatal: {}", e)),
    }
}

/// Handles the 'sprint advance' command, advancing a sprint to new dates.
fn handle_sprint_advance_command(name: &str, start_date: &str, end_date: &str) {
    match SprintAdvanceCommand::execute(name, start_date, end_date) {
        Ok(_) => process::exit(0),
        Err(e) => exit_with_error(&format!("fatal: {}", e)),
    }
}

/// Handles the 'sprint view' command, displaying a summary of the current sprint.
fn handle_sprint_view_command() {
    match SprintViewCommand::execute() {
        Ok(_) => process::exit(0),
        Err(e) => exit_with_error(&format!("fatal: {}", e)),
    }
}

/// Handles the 'task create' command, creating a new task with the given details.
fn handle_task_create_command(id: &str, description: &str, story_points: Option<u32>) {
    match TaskCreateCommand::execute(id, description, story_points) {
        Ok(_) => process::exit(0),
        Err(e) => exit_with_error(&format!("fatal: {}", e)),
    }
}

/// Handles the 'task complete' command, marking a task as completed and optionally merging changes.
fn handle_task_complete_command(id: &str, auto_merge: bool) {
    match TaskCompleteCommand::execute(id, auto_merge) {
        Ok(_) => process::exit(0),
        Err(e) => exit_with_error(&format!("fatal: {}", e)),
    }
}

/// Handles the 'task status' command, displaying the status of a specific task.
fn handle_task_status_command(id: &str) {
    match TaskStatusCommand::execute(id) {
        Ok(_) => process::exit(0),
        Err(e) => exit_with_error(&format!("fatal: {}", e)),
    }
}

/// Handles the 'task list' command, listing all tasks in the repository.
fn handle_task_list_command(args: &[String]) {
    let command = TaskListCommand {
        repo_path: String::from("."),
        args: args.to_vec(),
    };
    match command.execute() {
        Ok(_) => process::exit(0),
        Err(e) => exit_with_error(&format!("fatal: {}", e)),
    }
}