// src/commands/commit.rs - updated version
use std::path::{Path, PathBuf};
use std::time::Instant;
use std::env;
use std::collections::{HashMap, HashSet};
use crate::core::database::tree::{TreeEntry, TREE_MODE};
use crate::core::database::author::Author;
use crate::core::database::commit::Commit;
use crate::core::database::tree::Tree;
use crate::core::editor::Editor;
use crate::core::index::index::Index;
use crate::core::refs::Refs;
use crate::core::write_commit::{WriteCommit, WriteCommitOptions, EditOption};
use crate::errors::error::Error;
use log::{debug, info, warn, error};

const COMMIT_NOTES: &str = "\
Please enter the commit message for your changes. Lines starting
with '#' will be ignored, and an empty message aborts the commit.
";

pub struct CommitCommand;

impl CommitCommand {
    pub fn execute(message: &str) -> Result<(), Error> {
        let start_time = Instant::now();
        
        info!("Starting commit execution");

        let root_path = Path::new(".");
        let git_path = root_path.join(".ash");
        
        // Verify .ash directory exists
        if !git_path.exists() {
            error!(".ash directory not found at {}", root_path.display());
            return Err(Error::Generic("Not an ash repository (or any of the parent directories): .ash directory not found".into()));
        }

        let db_path = git_path.join("objects");

        debug!("Initializing components");
        let mut database = crate::core::database::database::Database::new(db_path);

        // Check for the index file
        let index_path = git_path.join("index");
        if !index_path.exists() {
            error!("Index file not found at {}", index_path.display());
            return Err(Error::Generic("No index file found. Please add some files first.".into()));
        }

        // Check for existing index.lock file before trying to load the index
        let index_lock_path = git_path.join("index.lock");
        if index_lock_path.exists() {
            error!("Index lock file exists: {}", index_lock_path.display());
            return Err(Error::Lock(format!(
                "Unable to create '.ash/index.lock': File exists.\n\
                Another ash process seems to be running in this repository.\n\
                If it still fails, a process may have crashed in this repository earlier:\n\
                remove the file manually to continue."
            )));
        }

        let mut index = Index::new(index_path);

        info!("Loading index");
        // Load the index
        match index.load() {
            Ok(_) => info!("Index loaded successfully"),
            Err(e) => {
                error!("Error loading index: {}", e);
                return Err(Error::Generic(format!("Error loading index: {}", e)));
            }
        }

        // Check for HEAD lock
        let head_lock_path = git_path.join("HEAD.lock");
        if head_lock_path.exists() {
            error!("HEAD lock file exists: {}", head_lock_path.display());
            return Err(Error::Lock(format!(
                "Unable to create '.ash/HEAD.lock': File exists.\n\
                Another ash process seems to be running in this repository.\n\
                If it still fails, a process may have crashed in this repository earlier:\n\
                remove the file manually to continue."
            )));
        }

        let refs = Refs::new(&git_path);
        
        // Create WriteCommitOptions
        let options = WriteCommitOptions {
            message: if message.is_empty() { None } else { Some(message.to_string()) },
            file: None,
            edit: if env::var("ASH_EDIT").unwrap_or_default() == "1" { 
                EditOption::Always 
            } else { 
                EditOption::Auto 
            },
        };
        
        // Create WriteCommit struct
        let mut write_commit = WriteCommit::new(
            &mut database,
            &mut index,
            &refs,
            root_path,
            &options
        );
        
        // Get initial message
        let initial_message = write_commit.read_message()?;
        
        // Compose final message
        let composed_message = write_commit.compose_message(initial_message, COMMIT_NOTES)?;
        
        // If no message is provided, abort the commit
        let message = match composed_message {
            Some(msg) => msg,
            None => {
                println!("Aborting commit due to empty commit message");
                return Ok(());
            }
        };
        
        info!("Reading HEAD");
        // Get the parent commit OID
        let parent = match refs.read_head() {
            Ok(p) => {
                info!("HEAD read successfully: {:?}", p);
                p
            },
            Err(e) => {
                error!("Error reading HEAD: {:?}", e);
                return Err(e);
            }
        };
        
        // Create a clone of parent for later use
        let parent_clone = parent.clone();
        
        // Convert optional parent to Vec
        let parents = match &parent {
            Some(p) => vec![p.clone()],
            None => vec![],
        };
        
        // Check for changes
        let tree_oid = write_commit.write_tree()?;
        
        // Check if parent tree matches current tree
        let mut no_changes = false;
        
        // Create a separate Database for this operation
        let mut temp_database = crate::core::database::database::Database::new(
            git_path.join("objects")
        );
        
        if let Some(parent_oid) = &parent_clone {
            match temp_database.load(parent_oid) {
                Ok(parent_obj) => {
                    if let Some(parent_commit) = parent_obj.as_any().downcast_ref::<Commit>() {
                        let parent_tree_oid = parent_commit.get_tree();
                        info!("Parent commit tree OID: {}", parent_tree_oid);
                        if &tree_oid == parent_tree_oid {
                            info!("Tree OIDs match. No changes detected.");
                            no_changes = true;
                        } else {
                            debug!("Tree OIDs differ: Current={}, Parent={}", tree_oid, parent_tree_oid);
                        }
                    }
                },
                Err(_) => {
                    // Unable to load parent commit - assume there are changes
                }
            }
        }

        if no_changes {
            return Err(Error::Generic("No changes staged for commit.".into()));
        }
        
        // Create commit
        let commit = write_commit.create_commit(parents, message)?;
        
        // Update HEAD reference
        refs.update_head(commit.get_oid().unwrap_or(&String::new()))?;
        
        // Print commit info
        write_commit.print_commit(&commit);
        
        // Count changed files with a separate database instance
        let mut counting_database = crate::core::database::database::Database::new(
            git_path.join("objects")
        );
        let changed_files_count = Self::count_changed_files(&commit, &mut counting_database)?;
        
        // Show elapsed time
        let elapsed = start_time.elapsed();
        println!(
            "{} file{} changed ({:.2}s)",
            changed_files_count,
            if changed_files_count == 1 { "" } else { "s" },
            elapsed.as_secs_f32()
        );

        Ok(())
    }
    
    // Helper method to count changed files
    fn count_changed_files(commit: &Commit, database: &mut crate::core::database::database::Database) -> Result<usize, Error> {
        let mut count = 0;
        
        // If it's a root commit, just count entries in the tree
        if commit.get_parent().is_none() {
            let tree_oid = commit.get_tree();
            let mut files = HashMap::<String, String>::new();
            Self::collect_files_from_tree(database, tree_oid, PathBuf::new(), &mut files)?;
            return Ok(files.len());
        }
        
        // Compare with parent commit
        let parent_oid = commit.get_parent().unwrap();
        let parent_obj = database.load(parent_oid)?;
        
        if let Some(parent_commit) = parent_obj.as_any().downcast_ref::<Commit>() {
            let parent_tree_oid = parent_commit.get_tree();
            let tree_oid = commit.get_tree();
            
            let mut parent_files = HashMap::<String, String>::new();
            let mut current_files = HashMap::<String, String>::new();
            
            Self::collect_files_from_tree(database, parent_tree_oid, PathBuf::new(), &mut parent_files)?;
            Self::collect_files_from_tree(database, tree_oid, PathBuf::new(), &mut current_files)?;
            
            let all_paths: HashSet<_> = parent_files.keys().chain(current_files.keys()).collect();
            
            for path in all_paths {
                match (parent_files.get(path), current_files.get(path)) {
                    (Some(old_oid), Some(new_oid)) if old_oid != new_oid => count += 1,
                    (None, Some(_)) => count += 1,
                    (Some(_), None) => count += 1,
                    _ => {}
                }
            }
        }
        
        Ok(count)
    }
    
    // Existing helper method to collect files - unchanged
    fn collect_files_from_tree(
        database: &mut crate::core::database::database::Database,
        tree_oid: &str,
        prefix: PathBuf,
        files: &mut HashMap<String, String>
    ) -> Result<(), Error> {
        let obj = database.load(tree_oid)?;

        if let Some(tree) = obj.as_any().downcast_ref::<Tree>() {
            for (name, entry) in tree.get_entries() {
                let entry_path = if prefix.as_os_str().is_empty() { PathBuf::from(name) } else { prefix.join(name) };
                let entry_path_str = entry_path.to_string_lossy().to_string();
                match entry {
                    TreeEntry::Blob(oid, mode) => {
                        if *mode == TREE_MODE || mode.is_directory() {
                            Self::collect_files_from_tree(database, &oid, entry_path, files)?;
                        } else {
                            files.insert(entry_path_str, oid.clone());
                        }
                    },
                    TreeEntry::Tree(subtree) => {
                        if let Some(subtree_oid) = subtree.get_oid() {
                            Self::collect_files_from_tree(database, subtree_oid, entry_path, files)?;
                        }
                    }
                }
            }
            return Ok(());
        }

        if obj.get_type() == "blob" {
            let blob_data = obj.to_bytes();
            match Tree::parse(&blob_data) {
                Ok(parsed_tree) => {
                    for (name, entry) in parsed_tree.get_entries() {
                        let entry_path = if prefix.as_os_str().is_empty() { PathBuf::from(name) } else { prefix.join(name) };
                        let entry_path_str = entry_path.to_string_lossy().to_string();
                        match entry {
                            TreeEntry::Blob(oid, mode) => {
                                if *mode == TREE_MODE || mode.is_directory() {
                                    Self::collect_files_from_tree(database, &oid, entry_path, files)?;
                                } else {
                                    files.insert(entry_path_str, oid.clone());
                                }
                            },
                            TreeEntry::Tree(subtree) => {
                                if let Some(subtree_oid) = subtree.get_oid() {
                                    Self::collect_files_from_tree(database, subtree_oid, entry_path, files)?;
                                }
                            }
                        }
                    }
                    return Ok(());
                },
                Err(_) => {
                    if !prefix.as_os_str().is_empty() {
                        let path_str = prefix.to_string_lossy().to_string();
                        files.insert(path_str, tree_oid.to_string());
                        return Ok(());
                    }
                }
            }
        }
        Ok(())
    }
}