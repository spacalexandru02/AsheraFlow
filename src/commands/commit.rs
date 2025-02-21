use std::path::Path;
use crate::core::workspace::Workspace;
use crate::core::database::Database;
use crate::core::blob::Blob;
use crate::core::entry::Entry;
use crate::core::tree::Tree;
use crate::errors::error::Error;

pub struct CommitCommand;

impl CommitCommand {
    pub fn execute(message: &str) -> Result<(), Error> {
        let root_path = Path::new(".");
        let git_path = root_path.join(".ash");
        let db_path = git_path.join("objects");

        let workspace = Workspace::new(root_path);
        let mut database = Database::new(db_path);

        // Create blobs for all files
        let entries: Vec<Entry> = workspace
            .list_files()?
            .into_iter()
            .map(|path| {
                let data = workspace.read_file(&path)?;
                let mut blob = Blob::new(data);
                database.store(&mut blob)?;
                Ok(Entry::new(path, blob.get_oid().unwrap().clone()))
            })
            .collect::<Result<Vec<Entry>, Error>>()?;

        // Create and store the tree
        let mut tree = Tree::new(entries);
        database.store(&mut tree)?;

        println!("tree: {}", tree.get_oid().unwrap());
        println!("Commit message: {}", message);

        Ok(())
    }
}