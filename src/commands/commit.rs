use std::path::Path;
use crate::core::workspace::Workspace;
use crate::errors::error::Error;

pub struct CommitCommand;

impl CommitCommand {
    pub fn execute(message: &str) -> Result<(), Error> {
        let root_path = Path::new(".");
        let workspace = Workspace::new(root_path);

        let files = workspace.list_files()?;
        println!("Files to be committed: {:?}", files);

        // TODO: Implement actual commit logic (e.g., create tree, write commit object)
        println!("Commit message: {}", message);

        Ok(())
    }
}