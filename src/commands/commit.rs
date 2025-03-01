use std::env;
use std::path::Path;
use crate::core::refs::Refs;
use crate::core::workspace::Workspace;
use crate::core::database::Database;
use crate::core::blob::Blob;
use crate::core::entry::Entry;
use crate::core::tree::Tree;
use crate::core::author::Author;
use crate::core::commit::Commit;
use crate::errors::error::Error;
use std::collections::HashSet;

pub struct CommitCommand;

impl CommitCommand {
    pub fn execute(message: &str) -> Result<(), Error> {
        let root_path = Path::new(".");
        let git_path = root_path.join(".ash");
        let db_path = git_path.join("objects");

        let workspace = Workspace::new(root_path);
        let mut database = Database::new(db_path);
        let refs = Refs::new(&git_path);
        let parent = refs.read_head()?;

        // Crează un set pentru a evita duplicatele
        let mut unique_files = HashSet::new();

        // Creează blob-uri pentru toate fișierele
        let entries: Vec<Entry> = workspace
            .list_files()?
            .into_iter()
            .filter(|path| unique_files.insert(path.clone())) // Evită duplicatele
            .map(|path| {
                let data = workspace.read_file(&path)?;
                let mut blob = Blob::new(data);
                database.store(&mut blob)?;
                let mode = "100644"; // Sau altă valoare corespunzătoare
                Ok(Entry::new(path.to_string_lossy().to_string(), blob.get_oid().unwrap().clone(), mode))
            })
            .collect::<Result<Vec<Entry>, Error>>()?;

        // Creează și stochează arborele
        let mut tree = Tree::new(entries)?;
        database.store(&mut tree)?;
        let tree_oid = tree.get_oid()
    .ok_or(Error::Generic("Tree OID not set after storage".into()))?; 

        // Creează și stochează commit-ul
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

        // Actualizează HEAD
        refs.update_head(commit_oid)?;

        // Afișează mesajul
        let is_root = if parent.is_none() { "(root-commit) " } else { "" };
        let first_line = message.lines().next().unwrap_or("");
        println!("[{}{}] {}", is_root, commit.get_oid().unwrap(), first_line);

        println!("[(root-commit) {}] {}", commit.get_oid().unwrap(), message.lines().next().unwrap_or(""));
        Ok(())
    }
}