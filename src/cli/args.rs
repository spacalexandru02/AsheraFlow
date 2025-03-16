#[derive(Debug)]
pub enum Command {
    Init { path: String },
    Commit { message: String },
    Add { paths: Vec<String> },
    Status { porcelain: bool }, // Add the Status command with porcelain option
    Validate,
    Unknown { name: String },
}

#[derive(Debug)]
pub struct CliArgs {
    pub command: Command,
}
