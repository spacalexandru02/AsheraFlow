#[derive(Debug)]
pub enum Command {
    Init { path: String },
    Commit { message: String },
    Add { paths: Vec<String> },
    Status { porcelain: bool, color: String }, 
    Diff { paths: Vec<String>, cached: bool },
    Branch { 
        name: String, 
        start_point: Option<String>,
        verbose: bool,
        delete: bool,
        force: bool
    },
    Checkout { target: String },
    Log {
        revisions: Vec<String>,
        abbrev: bool,
        format: String,
        patch: bool,
        decorate: String,
    },
    Merge {
        branch: String,
        message: Option<String>,
        abort: bool,
        continue_merge: bool,
    },
    Reset {
        revision: String,
        paths: Vec<String>,
        soft: bool,
        mixed: bool,
        hard: bool,
    },
    Revert {
        commit: String,
        continue_revert: bool,
        abort: bool,
    },
    Rm {
        paths: Vec<String>,
        cached: bool,
        force: bool,
        recursive: bool,
    },
    Unknown { name: String },
}

#[derive(Debug)]
pub struct CliArgs {
    pub command: Command,
}
