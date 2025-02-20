#[derive(Debug)]
pub enum Command {
    Init { path: String },
    Unknown { name: String },
}

#[derive(Debug)]
pub struct CliArgs {
    pub command: Command,
}