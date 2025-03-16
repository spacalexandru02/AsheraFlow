use std::{env, path::Path, time::Instant};
use std::collections::HashSet;

use crate::core::database::tree::TreeEntry;
use crate::{core::{database::{author::Author, commit::Commit, database::Database, entry::Entry, tree::Tree}, index::index::Index, refs::Refs}, errors::error::Error};

pub struct CommitCommand;

impl CommitCommand {
    pub fn execute(message: &str) -> Result<(), Error> {
        let start_time = Instant::now();
        
        // Validate the commit message
        if message.trim().is_empty() {
            return Err(Error::Generic("Aborting commit due to empty commit message".into()));
        }
        
        println!("Starting commit execution");
        
        let root_path = Path::new(".");
        let git_path = root_path.join(".ash");
        
        // Verify .ash directory exists
        if !git_path.exists() {
            return Err(Error::Generic("Not an ash repository (or any of the parent directories): .ash directory not found".into()));
        }
        
        let db_path = git_path.join("objects");
        
        println!("Initializing components");
        let mut database = Database::new(db_path);
        
        // Check for the index file
        let index_path = git_path.join("index");
        if !index_path.exists() {
            return Err(Error::Generic("No index file found. Please add some files first.".into()));
        }
        
        // Check for existing index.lock file before trying to load the index
        let index_lock_path = git_path.join("index.lock");
        if index_lock_path.exists() {
            return Err(Error::Lock(format!(
                "Unable to create '.ash/index.lock': File exists.\n\
                Another ash process seems to be running in this repository.\n\
                If it still fails, a process may have crashed in this repository earlier:\n\
                remove the file manually to continue."
            )));
        }
        
        let mut index = Index::new(index_path);
        
        println!("Loading index");
        // Load the index (read-only is sufficient for commit)
        match index.load() {
            Ok(_) => println!("Index loaded successfully"),
            Err(e) => return Err(Error::Generic(format!("Error loading index: {}", e))),
        }
        
        // Check if the index is empty
        if index.entries.is_empty() {
            return Err(Error::Generic("No changes staged for commit. Use 'ash add' to add files.".into()));
        }
        
        // Check for HEAD lock
        let head_lock_path = git_path.join("HEAD.lock");
        if head_lock_path.exists() {
            return Err(Error::Lock(format!(
                "Unable to create '.ash/HEAD.lock': File exists.\n\
                Another ash process seems to be running in this repository.\n\
                If it still fails, a process may have crashed in this repository earlier:\n\
                remove the file manually to continue."
            )));
        }
        
        let refs = Refs::new(&git_path);
        
        println!("Reading HEAD");
        // Get the parent commit OID
        let parent = match refs.read_head() {
            Ok(p) => {
                println!("HEAD read successfully: {:?}", p);
                p
            },
            Err(e) => {
                println!("Error reading HEAD: {:?}", e);
                return Err(e);
            }
        };
        
        // Convert index entries to database entries
        let database_entries: Vec<Entry> = index.each_entry()
            .map(|index_entry| {
                Entry::new(
                    index_entry.path.clone(),
                    index_entry.oid.clone(),
                    &index_entry.mode_octal()
                )
            })
            .collect();
        
        // Add this at the beginning of CommitCommand::execute to properly validate the paths
        println!("Index entries before building tree:");
        for entry in &database_entries {
            println!("  Path: {}  OID: {}  Mode: {}", entry.get_name(), entry.get_oid(), entry.get_mode());
        }
        
        // Verify all objects exist in the database
        let mut missing_objects = Vec::new();
        let mut unique_oids = HashSet::new();
        
        for entry in &database_entries {
            let oid = entry.get_oid();
            if !unique_oids.contains(oid) && !database.exists(oid) {
                missing_objects.push((oid.to_string(), entry.get_name().to_string()));
                unique_oids.insert(oid.to_string());
            }
        }
        
        if !missing_objects.is_empty() {
            let mut error_msg = String::from("Error: The following objects are missing from the object database:\n");
            for (oid, path) in missing_objects {
                error_msg.push_str(&format!("  {} {}\n", oid, path));
            }
            error_msg.push_str("\nAborting commit. Run 'ash add' on these files to add them to the object database.");
            return Err(Error::Generic(error_msg));
        }
        
        // Build tree from index entries
        let mut root = match Tree::build(database_entries.iter()) {
            Ok(tree) => tree,
            Err(e) => return Err(Error::Generic(format!("Failed to build tree: {}", e))),
        };
        
        // Add this right after the Tree::build call
        println!("\nTree structure after building:");
        println!("Root entries: {}", root.get_entries().len());
        for (name, entry) in root.get_entries() {
            match entry {
                TreeEntry::Blob(oid, mode) => {
                    println!("  {} (blob, mode {}) -> {}", name, mode, oid);
                },
                TreeEntry::Tree(subtree) => {
                    let oid_str = if let Some(oid) = subtree.get_oid() {
                        format!("Some(\"{}\")", oid)
                    } else {
                        "None".to_string()
                    };
                    println!("  {} (tree) -> {}", name, oid_str);
                    
                    // Recursively print the first level of the subtree
                    for (sub_name, sub_entry) in subtree.get_entries() {
                        match sub_entry {
                            TreeEntry::Blob(sub_oid, sub_mode) => {
                                println!("    {}/{} (blob, mode {}) -> {}", name, sub_name, sub_mode, sub_oid);
                            },
                            TreeEntry::Tree(sub_subtree) => {
                                let sub_oid_str = if let Some(oid) = sub_subtree.get_oid() {
                                    format!("Some(\"{}\")", oid)
                                } else {
                                    "None".to_string()
                                };
                                println!("    {}/{} (tree) -> {}", name, sub_name, sub_oid_str);
                            }
                        }
                    }
                }
            }
        }
        
        // Replace the tree storage code in CommitCommand::execute with this:

        // Store all trees
        println!("\nStoring trees to database...");
        let mut tree_counter = 0;
        if let Err(e) = root.traverse(|tree| {
            tree_counter += 1;
            println!("Storing tree #{} with {} entries...", tree_counter, tree.get_entries().len());
            
            // Debug: Print entries before storing
            for (name, entry) in tree.get_entries() {
                match entry {
                    TreeEntry::Blob(oid, mode) => {
                        println!("  Entry: {} (blob) -> {}", name, oid);
                    },
                    TreeEntry::Tree(subtree) => {
                        if let Some(oid) = subtree.get_oid() {
                            println!("  Entry: {} (tree) -> {}", name, oid);
                        } else {
                            println!("  Entry: {} (tree) -> <no OID>", name);
                        }
                    }
                }
            }
            
            // Now store returns the OID as Ok(String), but we don't need it here
            // since Tree.set_oid() is called inside the store method
            match database.store(tree) {
                Ok(oid) => {
                    println!("  Tree stored with OID: {}", oid);
                    Ok(())
                },
                Err(e) => {
                    println!("  Error storing tree: {}", e);
                    Err(e)
                }
            }
        }) {
            return Err(Error::Generic(format!("Failed to store trees: {}", e)));
        }
        
        // Get the root tree OID
        let tree_oid = root.get_oid()
            .ok_or(Error::Generic("Tree OID not set after storage".into()))?;
        
        // With this fixed version:
        println!("\nChecking stored tree structure:");
        let stored_tree_obj = database.load(&tree_oid)?;
        let stored_tree = stored_tree_obj.as_any().downcast_ref::<Tree>().unwrap();
        println!("Stored root entries: {}", stored_tree.get_entries().len());
        for (name, entry) in stored_tree.get_entries() {
            match entry {
                TreeEntry::Blob(oid, mode) => {
                    println!("  {} (blob, mode {}) -> {}", name, mode, oid);
                },
                TreeEntry::Tree(subtree) => {
                    let oid_str = if let Some(oid) = subtree.get_oid() {
                        format!("Some(\"{}\")", oid)
                    } else {
                        "None".to_string()
                    };
                    println!("  {} (tree) -> {}", name, oid_str);
                }
            }
        }
        
        // Adaugă aceste linii de debug
        println!("Tree OID: {}", tree_oid);
        if let Some(parent_oid) = &parent {
            println!("Parent OID: {}", parent_oid);
        }
        println!("Message: {}", message);
        
        // Create and store the commit
        let name = match env::var("GIT_AUTHOR_NAME").or_else(|_| env::var("USER")) {
            Ok(name) => name,
            Err(_) => return Err(Error::Generic(
                "Unable to determine author name. Please set GIT_AUTHOR_NAME environment variable".into()
            )),
        };
        
        let email = match env::var("GIT_AUTHOR_EMAIL") {
            Ok(email) => email,
            Err(_) => format!("{}@{}", name, "localhost"), // Fallback email
        };
        
        let author = Author::new(name, email);
        let mut commit = Commit::new(
            parent.clone(),
            tree_oid.clone(),
            author,
            message.to_string()
        );
        
        if let Err(e) = database.store(&mut commit) {
            return Err(Error::Generic(format!("Failed to store commit: {}", e)));
        }
        
        let commit_oid = commit.get_oid()
            .ok_or(Error::Generic("Commit OID not set after storage".into()))?;
        
        // Update HEAD
        if let Err(e) = refs.update_head(commit_oid) {
            return Err(Error::Generic(format!("Failed to update HEAD: {}", e)));
        }
        
        // Print commit message
        let is_root = if parent.is_none() { "(root-commit) " } else { "" };
        let first_line = message.lines().next().unwrap_or("");
        
        let elapsed = start_time.elapsed();
        println!(
            "[{}{}] {} ({:.2}s)", 
            is_root, 
            commit.get_oid().unwrap(), 
            first_line,
            elapsed.as_secs_f32()
        );
        
        // Print a summary of the commit
        let entry_count = database_entries.len();
        println!(
            "{} file{} changed", 
            entry_count, 
            if entry_count == 1 { "" } else { "s" }
        );
        
        Ok(())
    }
}