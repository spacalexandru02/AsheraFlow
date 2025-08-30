/// Defines all supported commands for the AsheraFlow CLI.
#[derive(Debug)]
pub enum Command {
    /// Initializes a new AsheraFlow repository at the specified path.
    Init { path: String },
    /// Creates a new commit or amends an existing one.
    Commit { 
        message: String,
        amend: bool,
        reuse_message: Option<String>,
        edit: bool,
    },
    /// Stages files for commit.
    Add { paths: Vec<String> },
    /// Displays the current state of the working directory and index.
    Status { porcelain: bool, color: String }, 
    /// Shows changes between commits, commit and working tree, etc.
    Diff { paths: Vec<String>, cached: bool },
    /// Manages branches (create, delete, list, etc.).
    Branch { 
        name: String, 
        start_point: Option<String>,
        verbose: bool,
        delete: bool,
        force: bool
    },
    /// Switches branches or restores working tree files.
    Checkout { target: String },
    /// Displays commit logs with various formatting options.
    Log {
        revisions: Vec<String>,
        abbrev: bool,
        format: String,
        patch: bool,
        decorate: String,
    },
    /// Merges changes from another branch into the current branch.
    Merge {
        branch: String,
        message: Option<String>,
        abort: bool,
        continue_merge: bool,
        tool: Option<String>, 
    },
    /// Removes files from the working tree and/or index.
    Rm {
        files: Vec<String>,
        cached: bool,
        force: bool,
        recursive: bool,
    },
    /// Resets current HEAD to the specified state.
    Reset {
        files: Vec<String>,
        soft: bool,
        mixed: bool,
        hard: bool,
        force: bool,
        reuse_message: Option<String>,
    },
    /// Applies changes from specific commits.
    CherryPick {
        args: Vec<String>,
        r#continue: bool,
        abort: bool,
        quit: bool,
        mainline: Option<u32>,
    },
    /// Reverts changes from specific commits.
    Revert {
        args: Vec<String>,
        r#continue: bool,
        abort: bool,
        quit: bool,
        mainline: Option<u32>,
    },
    /// Sprint management commands
    SprintStart {
        name: String,
        duration: u32,
    },
    /// Displays information about the current sprint.
    SprintInfo {},
    /// Shows commit mapping for a sprint.
    SprintCommitMap {
        sprint_name: Option<String>,
    },
    /// Displays the burndown chart for a sprint.
    SprintBurndown {
        sprint_name: Option<String>,
    },
    /// Shows sprint velocity statistics.
    SprintVelocity {},
    /// Advances a sprint to new dates.
    SprintAdvance {
        name: String,
        start_date: String,
        end_date: String,
    },
    /// Displays a summary of the current sprint.
    SprintView {},
    /// Task management commands
    TaskCreate {
        id: String,
        description: String,
        story_points: Option<u32>,
    },
    /// Marks a task as completed and optionally merges changes.
    TaskComplete {
        id: String,
        story_points: Option<i32>,
        auto_merge: bool,
    },
    /// Displays the status of a specific task.
    TaskStatus {
        id: String,
    },
    /// Lists all tasks in the repository.
    TaskList {
        args: Vec<String>,
    },
    /// Represents an unknown or unsupported command.
    Unknown { name: String },
}

/// Structure holding parsed CLI arguments and the selected command.
#[derive(Debug)]
pub struct CliArgs {
    pub command: Command,
}