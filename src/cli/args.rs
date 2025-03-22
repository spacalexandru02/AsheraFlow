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
    Unknown { name: String },
}

#[derive(Debug)]
pub struct CliArgs {
    pub command: Command,
}
