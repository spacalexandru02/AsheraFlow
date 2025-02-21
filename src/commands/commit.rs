use std::path::Path;
use crate::core::workspace::Workspace;
use crate::core::database::Database;
use crate::core::blob::Blob;
use crate::errors::error::Error;

pub struct CommitCommand;

impl CommitCommand {
    pub fn execute(message: &str) -> Result<(), Error> {
        let root_path = Path::new(".");
        let git_path = root_path.join(".ash");
        let db_path = git_path.join("objects");

        let workspace = Workspace::new(root_path);
        let mut database = Database::new(db_path);

        let files = workspace.list_files()?;
        for file in files {
            let data = workspace.read_file(&file)?;
            let mut blob = Blob::new(data);
            database.store(&mut blob)?;
            println!("Stored blob {} for file: {}", blob.get_oid().unwrap(), file);
        }

        println!("Commit message: {}", message);
        Ok(())
    }
}