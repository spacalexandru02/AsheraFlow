// src/commands/revert.rs
// Implementation of the revert command that undoes the changes introduced by a commit
use std::time::Instant;
use std::fs;
use crate::errors::error::Error;
use crate::core::repository::repository::Repository;
use crate::core::revision::Revision;
use crate::core::database::commit::Commit;
use crate::core::database::author::Author;
use crate::core::merge::inputs::Inputs;
use crate::core::merge::resolve::Resolve;
use crate::core::refs::ORIG_HEAD;

pub struct RevertCommand;

impl RevertCommand {
    pub fn execute(commit_id: &str, continue_revert: bool, abort: bool) -> Result<(), Error> {
        let start_time = Instant::now();
        let mut repo = Repository::new(".")?;
        
        // Handle continue/abort options
        if continue_revert {
            return Self::continue_revert(&mut repo);
        }
        
        if abort {
            return Self::abort_revert(&mut repo);
        }
        
        // Get current HEAD
        let head_oid = match repo.refs.read_head()? {
            Some(oid) => oid,
            None => return Err(Error::Generic("No HEAD commit found".to_string())),
        };
        
        // Resolve the commit to revert
        let mut revision = Revision::new(&mut repo, commit_id);
        let commit_oid = match revision.resolve("commit") {
            Ok(oid) => oid,
            Err(e) => {
                // Print any errors from revision resolution
                for err in revision.errors {
                    eprintln!("error: {}", err.message);
                    for hint in &err.hint {
                        eprintln!("hint: {}", hint);
                    }
                }
                return Err(e);
            }
        };
        
        // Check for conflicts or local changes
        Self::check_for_conflicts(&mut repo)?;
        
        // Load the commit to revert
        let commit_obj = repo.database.load(&commit_oid)?;
        let commit = match commit_obj.as_any().downcast_ref::<Commit>() {
            Some(c) => c,
            None => return Err(Error::Generic(format!("Object {} is not a commit", commit_oid))),
        };

        // Get the parent commit (the one we'll revert to)
        let parent_oid = match commit.get_parent() {
            Some(oid) => oid.clone(),
            None => return Err(Error::Generic(format!("Cannot revert {} - it has no parent", commit_oid))),
        };
        
        // Get the commit title for messages
        let commit_title = commit.title_line();
        let short_oid = &commit_oid[0..std::cmp::min(7, commit_oid.len())];
        
        println!("Reverting commit {}: {}", short_oid, commit_title);
        
        // Save the current HEAD to ORIG_HEAD
        let orig_head_path = std::path::Path::new(ORIG_HEAD);
        repo.refs.update_ref_file(orig_head_path, &head_oid)?;
        
        // Set up the merge inputs for reverting
        let inputs = Self::create_revert_inputs(&mut repo, &head_oid, &parent_oid, &commit_oid)?;
        
        // Lock index for updates
        repo.index.load_for_update()?;
        
        // Create the merge resolver 
        let mut merge_resolver = Resolve::new(&mut repo.database, &repo.workspace, &mut repo.index, &inputs);
        merge_resolver.on_progress = |msg| println!("{}", msg);
        
        // Perform the merge
        match merge_resolver.execute() {
            Ok(_) => {
                // No conflicts, we can auto-commit
                
                // Write updated index
                repo.index.write_updates()?;
                
                // Prepare revert commit message
                let message = Self::revert_commit_message(&commit_title, &commit_oid);
                
                // Create the revert commit
                Self::create_revert_commit(&mut repo, &message)?;
                
                println!("Successfully reverted commit {}", short_oid);
            },
            Err(e) => {
                if e.to_string().contains("Automatic merge failed") || e.to_string().contains("fix conflicts") {
                    // We have conflicts that need manual resolution
                    
                    // Write the index with conflicts
                    if let Err(write_err) = repo.index.write_updates() {
                        eprintln!("Failed to write index with conflicts: {}", write_err);
                        repo.index.rollback()?;
                        return Err(e);
                    }
                    
                    // Save revert state for later continuation
                    Self::save_revert_state(&repo, &commit_oid, &commit_title)?;
                    
                    println!("Revert failed due to conflicts");
                    println!("Fix the conflicts and run 'ash revert --continue'");
                    println!("Or run 'ash revert --abort' to cancel the revert operation");
                    
                    // Return the error
                    return Err(e);
                } else {
                    // Other error occurred
                    repo.index.rollback()?;
                    return Err(e);
                }
            }
        }
        
        let elapsed = start_time.elapsed();
        println!("Revert completed in {:.2}s", elapsed.as_secs_f32());
        
        Ok(())
    }
    
    // Create revert inputs for merge algorithm
    fn create_revert_inputs(
        repo: &mut Repository, 
        head_oid: &str, 
        parent_oid: &str,
        commit_oid: &str
    ) -> Result<Inputs, Error> {
        // When reverting a commit, we're essentially taking HEAD and applying the inverse of the commit
        // This means:
        // - left_oid is the current HEAD
        // - right_oid is the parent of the commit being reverted
        // - base_oids are the commit being reverted
        
        let inputs = Inputs::new(
            &mut repo.database,
            &repo.refs,
            "HEAD".to_string(),
            format!("parent of {}", commit_oid)
        )?;
        
        Ok(inputs)
    }
    
    // Generate revert commit message
    fn revert_commit_message(commit_title: &str, commit_oid: &str) -> String {
        format!(
            "Revert \"{}\"\n\nThis reverts commit {}.",
            commit_title.trim(),
            commit_oid
        )
    }
    
    // Create the revert commit
    fn create_revert_commit(repo: &mut Repository, message: &str) -> Result<(), Error> {
        // Get current HEAD
        let head_oid = match repo.refs.read_head()? {
            Some(oid) => oid,
            None => return Err(Error::Generic("No HEAD commit found".to_string())),
        };
        
        // Write tree from index
        let tree_oid = Self::write_tree_from_index(&mut repo.database, &repo.index)?;
        
        // Create author
        let author_name = std::env::var("GIT_AUTHOR_NAME").unwrap_or_else(|_| {
            "Default Author".to_string()
        });
        let author_email = std::env::var("GIT_AUTHOR_EMAIL").unwrap_or_else(|_| {
            "author@example.com".to_string()
        });
        let author = Author::new(author_name, author_email);
        
        // Create commit
        let mut commit = Commit::new(
            Some(head_oid.clone()), 
            tree_oid, 
            author.clone(), 
            message.to_string()
        );
        
        repo.database.store(&mut commit)?;
        let commit_oid = commit.get_oid().cloned().ok_or(Error::Generic("Commit OID not set after storage".into()))?;
        
        // Update HEAD
        repo.refs.update_head(&commit_oid)?;
        
        println!("Created revert commit: {}", &commit_oid[0..std::cmp::min(7, commit_oid.len())]);
        
        Ok(())
    }
    
    // Save revert state for later continuation
    fn save_revert_state(repo: &Repository, commit_oid: &str, commit_title: &str) -> Result<(), Error> {
        let revert_dir = repo.path.join(".ash/revert");
        fs::create_dir_all(&revert_dir)?;
        
        // Save commit info
        let message_path = revert_dir.join("message");
        let message = Self::revert_commit_message(&commit_title, &commit_oid);
        fs::write(message_path, message.as_bytes())?;
        
        // Save commit OID
        let commit_path = revert_dir.join("commit");
        fs::write(commit_path, commit_oid.as_bytes())?;
        
        Ok(())
    }
    
    // Continue an in-progress revert
    fn continue_revert(repo: &mut Repository) -> Result<(), Error> {
        // Check if there's a revert in progress
        let revert_dir = repo.path.join(".ash/revert");
        if !revert_dir.exists() {
            return Err(Error::Generic("No revert in progress".to_string()));
        }
        
        // Check if there are still conflicts
        repo.index.load()?;
        if repo.index.has_conflict() {
            return Err(Error::Generic("You must fix conflicts first".to_string()));
        }
        
        // Read saved message
        let message_path = revert_dir.join("message");
        let message = match fs::read_to_string(&message_path) {
            Ok(msg) => msg,
            Err(_) => return Err(Error::Generic("Failed to read revert message".to_string())),
        };
        
        // Create revert commit
        Self::create_revert_commit(repo, &message)?;
        
        // Clean up revert state
        fs::remove_dir_all(revert_dir)?;
        
        println!("Revert continued successfully");
        
        Ok(())
    }
    
    // Abort an in-progress revert
    fn abort_revert(repo: &mut Repository) -> Result<(), Error> {
        // Check if there's a revert in progress
        let revert_dir = repo.path.join(".ash/revert");
        if !revert_dir.exists() {
            return Err(Error::Generic("No revert in progress".to_string()));
        }
        
        // Restore from ORIG_HEAD
        let orig_head_path = repo.path.join(".ash").join(ORIG_HEAD);
        let orig_head = match fs::read_to_string(&orig_head_path) {
            Ok(oid) => oid.trim().to_string(),
            Err(_) => return Err(Error::Generic("Failed to read ORIG_HEAD".to_string())),
        };
        
        // Perform a hard reset to ORIG_HEAD
        println!("Restoring state before revert from ORIG_HEAD");
        
        // Lock the index
        repo.index.load_for_update()?;
        
        // Reset the index to ORIG_HEAD
        let tree_diff = repo.tree_diff(Some(&orig_head), None)?;
        let mut migration = repo.migration(tree_diff);
        migration.apply_changes()?;
        
        // Write the index
        repo.index.write_updates()?;
        
        // Update HEAD to ORIG_HEAD
        repo.refs.update_head(&orig_head)?;
        
        // Clean up revert state
        fs::remove_dir_all(revert_dir)?;
        
        println!("Revert aborted successfully");
        
        Ok(())
    }
    
    // Check for local changes that would be overwritten
    fn check_for_conflicts(repo: &mut Repository) -> Result<(), Error> {
        // Create Inspector to help analyze the repository state
        let inspector = crate::core::repository::inspector::Inspector::new(
            &repo.workspace,
            &repo.index,
            &repo.database
        );
        
        // Check for uncommitted changes
        let workspace_changes = inspector.analyze_workspace_changes()?;
        
        if !workspace_changes.is_empty() {
            let mut error_message = String::from("Cannot revert with uncommitted changes. Please commit or stash them first:\n");
            for (path, _) in workspace_changes {
                error_message.push_str(&format!("  {}\n", path));
            }
            return Err(Error::Generic(error_message));
        }
        
        Ok(())
    }
    
    // Write tree from index
    fn write_tree_from_index(database: &mut crate::core::database::database::Database, index: &crate::core::index::index::Index) -> Result<String, Error> {
        let database_entries: Vec<_> = index.each_entry()
            .filter(|entry| entry.stage == 0)
            .map(|index_entry| {
                crate::core::database::entry::DatabaseEntry::new(
                    index_entry.get_path().to_string(),
                    index_entry.get_oid().to_string(),
                    &index_entry.mode_octal()
                )
            })
            .collect();
        
        let mut root = crate::core::database::tree::Tree::build(database_entries.iter())?;
        root.traverse(|tree| database.store(tree).map(|_| ()))?;
        let tree_oid = root.get_oid().ok_or(Error::Generic("Tree OID not set after storage".into()))?;
        
        Ok(tree_oid.clone())
    }
}