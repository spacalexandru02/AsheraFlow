#[derive(Debug)]
pub enum Command {
    Init { path: String },
    Commit { message: String },
    Add { paths: Vec<String> },
    Unknown { name: String },
}

#[derive(Debug)]
pub struct CliArgs {
    pub command: Command,
}