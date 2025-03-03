use std::env;
use std::path::Path;
use crate::core::refs::Refs;
use crate::core::workspace::Workspace;
use crate::core::database::Database;
use crate::core::index::Index;
use crate::core::database::entry::Entry as DatabaseEntry;
use crate::core::tree::Tree;
use crate::core::author::Author;
use crate::core::commit::Commit;
use crate::errors::error::Error;

pub struct CommitCommand;

impl CommitCommand {
    pub fn execute(message: &str) -> Result<(), Error> {
        let root_path = Path::new(".");
        let git_path = root_path.join(".ash");
        let db_path = git_path.join("objects");

        let _workspace = Workspace::new(root_path);
        let mut database = Database::new(db_path);
        let mut index = Index::new(git_path.join("index"));
        let refs = Refs::new(&git_path);
        
        // Load the index (read-only)
        index.load()?;
        
        // Get the parent commit OID
        let parent = refs.read_head()?;
        
        // Convert index entries to database entries
        let database_entries: Vec<DatabaseEntry> = index.each_entry()
            .map(|index_entry| {
                DatabaseEntry::new(
                    index_entry.path.clone(),
                    index_entry.oid.clone(),
                    &index_entry.mode_octal()
                )
            })
            .collect();
        
        // Build tree from index entries
        let mut root = Tree::build(database_entries.iter())?;
        
        // Store all trees
        root.traverse(|tree| database.store(tree))?;
        
        // Get the root tree OID
        let tree_oid = root.get_oid()
            .ok_or(Error::Generic("Tree OID not set after storage".into()))?; 

        // Create and store the commit
        let name = env::var("GIT_AUTHOR_NAME").map_err(|_| {
            Error::Generic("GIT_AUTHOR_NAME environment variable is not set".to_string())
        })?;
        let email = env::var("GIT_AUTHOR_EMAIL").map_err(|_| {
            Error::Generic("GIT_AUTHOR_EMAIL environment variable is not set".to_string())
        })?;
        let author = Author::new(name, email);
        let mut commit = Commit::new(
            parent.clone(),
            tree_oid.clone(),
            author,
            message.to_string()
        );
        database.store(&mut commit)?;

        let commit_oid = commit.get_oid()
            .ok_or(Error::Generic("Commit OID not set after storage".into()))?;

        // Update HEAD
        refs.update_head(commit_oid)?;

        // Print commit message
        let is_root = if parent.is_none() { "(root-commit) " } else { "" };
        let first_line = message.lines().next().unwrap_or("");
        println!("[{}{}] {}", is_root, commit.get_oid().unwrap(), first_line);

        Ok(())
    }
}