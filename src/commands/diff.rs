// src/commands/diff.rs
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::time::Instant;

use crate::core::database::database::Database;
use crate::core::index::index::Index;
use crate::core::database::commit::Commit;
use crate::core::refs::Refs;
use crate::core::workspace::Workspace;
use crate::core::diff::diff;
use crate::core::diff::myers::{diff_lines,format_diff};
use crate::errors::error::Error;

pub struct DiffCommand;

impl DiffCommand {
    /// Execute diff command between index/HEAD and working tree
    pub fn execute(paths: &[String], cached: bool) -> Result<(), Error> {
        let start_time = Instant::now();
        
        let root_path = Path::new(".");
        let git_path = root_path.join(".ash");
        
        // Verify .ash directory exists
        if !git_path.exists() {
            return Err(Error::Generic("fatal: not an ash repository (or any of the parent directories): .ash directory not found".into()));
        }
        
        let workspace = Workspace::new(root_path);
        let mut database = Database::new(git_path.join("objects"));
        let mut index = Index::new(git_path.join("index"));
        let refs = Refs::new(&git_path);
        
        // Load the index
        index.load()?;
        
        // Determine what to compare based on paths and --cached flag
        if paths.is_empty() {
            // Handle full repository diff
            Self::diff_all(&workspace, &mut database, &index, &refs, cached)?;
        } else {
            // Handle specific paths
            for path_str in paths {
                let path = PathBuf::from(path_str);
                Self::diff_path(&workspace, &mut database, &index, &refs, &path, cached)?;
            }
        }
        
        let elapsed = start_time.elapsed();
        println!("Diff completed in {:.2}s", elapsed.as_secs_f32());
        
        Ok(())
    }
    
    /// Diff all changed files in the repository
    fn diff_all(
        workspace: &Workspace,
        database: &mut Database,
        index: &Index,
        refs: &Refs,
        cached: bool
    ) -> Result<(), Error> {
        // If cached flag is set, compare index with HEAD
        if cached {
            return Self::diff_index_vs_head(workspace, database, index, refs);
        }
        
        // Otherwise compare working tree with index
        let mut has_changes = false;
        
        // Get all files from the index
        for entry in index.each_entry() {
            let path = Path::new(entry.get_path());
            
            // Skip if the file doesn't exist in workspace
            if !workspace.path_exists(path)? {
                println!("diff --ash a/{} b/{}", path.display(), path.display());
                println!("deleted file mode {}", entry.mode_octal());
                println!("--- a/{}", path.display());
                println!("+++ /dev/null");
                
                // Get blob content from database
                let blob_obj = database.load(entry.get_oid())?;
                let content = blob_obj.to_bytes();
                let lines = diff::split_lines(&String::from_utf8_lossy(&content));
                
                // Show deletion diff
                for line in &lines {
                    println!("-{}", line);
                }
                
                has_changes = true;
                continue;
            }
            
            // Read file content
            let file_content = workspace.read_file(path)?;
            
            // Calculate hash for the file content
            let file_hash = database.hash_file_data(&file_content);
            
            // If the hash matches, there's no change
            if file_hash == entry.get_oid() {
                continue;
            }
            
            has_changes = true;
            
            // Print diff header
            println!("diff --ash a/{} b/{}", path.display(), path.display());
            
            // Get diff between index and working copy
            let diff_output = diff::diff_with_database(workspace, database, path, entry.get_oid(), 3)?;
            print!("{}", diff_output);
        }
        
        if !has_changes {
            println!("No changes");
        }
        
        Ok(())
    }
    
    /// Diff between index and HEAD
    fn diff_index_vs_head(
        workspace: &Workspace,
        database: &mut Database,
        index: &Index,
        refs: &Refs
    ) -> Result<(), Error> {
        // Get HEAD commit
        let head_oid = match refs.read_head()? {
            Some(oid) => oid,
            None => {
                println!("No HEAD commit found. Index contains initial version.");
                return Ok(());
            }
        };
        
        // Load HEAD commit
        let commit_obj = database.load(&head_oid)?;
        let commit = match commit_obj.as_any().downcast_ref::<Commit>() {
            Some(c) => c,
            None => return Err(Error::Generic("HEAD is not a commit".into())),
        };
        
        // Get files from HEAD
        let mut head_files = HashMap::new();
        Self::collect_files_from_commit(database, commit, &mut head_files)?;
        
        let mut has_changes = false;
        
        // Compare files in index with HEAD
        for entry in index.each_entry() {
            let path = entry.get_path();
            
            if let Some(head_oid) = head_files.get(path) {
                // File exists in both index and HEAD
                if head_oid == entry.get_oid() {
                    // No change
                    continue;
                }
                
                // File changed
                has_changes = true;
                println!("diff --ash a/{} b/{}", path, path);
                
                // Load both versions
                let head_obj = database.load(head_oid)?;
                let index_obj = database.load(entry.get_oid())?;
                
                let head_content = head_obj.to_bytes();
                let index_content = index_obj.to_bytes();
                
                let head_lines = diff::split_lines(&String::from_utf8_lossy(&head_content));
                let index_lines = diff::split_lines(&String::from_utf8_lossy(&index_content));
                
                // Calculate diff
                let edits = diff_lines(&head_lines, &index_lines);
                let diff = format_diff(&head_lines, &index_lines, &edits, 3);
                
                print!("{}", diff);
            } else {
                // File exists in index but not in HEAD (new file)
                has_changes = true;
                println!("diff --ash a/{} b/{}", path, path);
                println!("new file mode {}", entry.mode_octal());
                println!("--- /dev/null");
                println!("+++ b/{}", path);
                
                // Load index version
                let index_obj = database.load(entry.get_oid())?;
                let content = index_obj.to_bytes();
                let lines = diff::split_lines(&String::from_utf8_lossy(&content));
                
                // Show addition diff
                for line in &lines {
                    println!("+{}", line);
                }
            }
        }
        
        // Check for files in HEAD that were removed from index
        for (path, head_oid) in &head_files {
            if !index.tracked(path) {
                // File was in HEAD but removed from index
                has_changes = true;
                println!("diff --ash a/{} b/{}", path, path);
                println!("deleted file");
                println!("--- a/{}", path);
                println!("+++ /dev/null");
                
                // Load HEAD version
                let head_obj = database.load(head_oid)?;
                let content = head_obj.to_bytes();
                let lines = diff::split_lines(&String::from_utf8_lossy(&content));
                
                // Show deletion diff
                for line in &lines {
                    println!("-{}", line);
                }
            }
        }
        
        if !has_changes {
            println!("No changes staged for commit");
        }
        
        Ok(())
    }
    
    /// Diff a specific path
    fn diff_path(
        workspace: &Workspace,
        database: &mut Database,
        index: &Index,
        refs: &Refs,
        path: &Path,
        cached: bool
    ) -> Result<(), Error> {
        let path_str = path.to_string_lossy().to_string();
        
        // If path is in index
        if let Some(entry) = index.get_entry(&path_str) {
            if cached {
                // Compare index with HEAD
                let head_oid = match refs.read_head()? {
                    Some(oid) => oid,
                    None => {
                        // No HEAD, show as new file
                        println!("diff --ash a/{} b/{}", path_str, path_str);
                        println!("new file mode {}", entry.mode_octal());
                        println!("--- /dev/null");
                        println!("+++ b/{}", path_str);
                        
                        // Load index version
                        let index_obj = database.load(entry.get_oid())?;
                        let content = index_obj.to_bytes();
                        let lines = diff::split_lines(&String::from_utf8_lossy(&content));
                        
                        for line in &lines {
                            println!("+{}", line);
                        }
                        
                        return Ok(());
                    }
                };
                
                // Get file from HEAD commit
                let mut head_files = HashMap::new();
                let commit_obj = database.load(&head_oid)?;
                let commit = match commit_obj.as_any().downcast_ref::<Commit>() {
                    Some(c) => c,
                    None => return Err(Error::Generic("HEAD is not a commit".into())),
                };
                
                Self::collect_files_from_commit(database, commit, &mut head_files)?;
                
                if let Some(head_oid) = head_files.get(&path_str) {
                    // File exists in both HEAD and index
                    if head_oid == entry.get_oid() {
                        println!("No changes staged for {}", path_str);
                        return Ok(());
                    }
                    
                    // Compare HEAD and index versions
                    println!("diff --ash a/{} b/{}", path_str, path_str);
                    
                    // Load both versions
                    let head_obj = database.load(head_oid)?;
                    let index_obj = database.load(entry.get_oid())?;
                    
                    let head_content = head_obj.to_bytes();
                    let index_content = index_obj.to_bytes();
                    
                    let head_lines = diff::split_lines(&String::from_utf8_lossy(&head_content));
                    let index_lines = diff::split_lines(&String::from_utf8_lossy(&index_content));
                    
                    // Calculate diff
                    let edits = diff_lines(&head_lines, &index_lines);
                    let diff = format_diff(&head_lines, &index_lines, &edits, 3);
                    
                    print!("{}", diff);
                } else {
                    // File is in index but not in HEAD (new file)
                    println!("diff --ash a/{} b/{}", path_str, path_str);
                    println!("new file mode {}", entry.mode_octal());
                    println!("--- /dev/null");
                    println!("+++ b/{}", path_str);
                    
                    // Load index version
                    let index_obj = database.load(entry.get_oid())?;
                    let content = index_obj.to_bytes();
                    let lines = diff::split_lines(&String::from_utf8_lossy(&content));
                    
                    for line in &lines {
                        println!("+{}", line);
                    }
                }
            } else {
                // Compare index with working tree
                if !workspace.path_exists(path)? {
                    println!("diff --ash a/{} b/{}", path_str, path_str);
                    println!("deleted file mode {}", entry.mode_octal());
                    println!("--- a/{}", path_str);
                    println!("+++ /dev/null");
                    
                    // Load index version
                    let index_obj = database.load(entry.get_oid())?;
                    let content = index_obj.to_bytes();
                    let lines = diff::split_lines(&String::from_utf8_lossy(&content));
                    
                    for line in &lines {
                        println!("-{}", line);
                    }
                    
                    return Ok(());
                }
                
                // Read working copy
                let file_content = workspace.read_file(path)?;
                
                // Calculate hash for the file content
                let file_hash = database.hash_file_data(&file_content);
                
                // If the hash matches, there's no change
                if file_hash == entry.get_oid() {
                    println!("No changes in {}", path_str);
                    return Ok(());
                }
                
                // Show diff between index and working copy
                println!("diff --ash a/{} b/{}", path_str, path_str);
                
                let diff_output = diff::diff_with_database(workspace, database, path, entry.get_oid(), 3)?;
                print!("{}", diff_output);
            }
        } else {
            // Path not in index
            if workspace.path_exists(path)? {
                println!("error: path '{}' is untracked", path_str);
            } else {
                println!("error: path '{}' does not exist", path_str);
            }
        }
        
        Ok(())
    }
    
    /// Collect all files from a commit
    fn collect_files_from_commit(
        database: &mut Database,
        commit: &Commit,
        files: &mut HashMap<String, String>
    ) -> Result<(), Error> {
        // Get the tree OID from the commit
        let tree_oid = commit.get_tree();
        
        // Collect files from the tree
        Self::collect_files_from_tree(database, tree_oid, PathBuf::new(), files)?;
        
        Ok(())
    }
    
    /// Recursively collect files from a tree
    fn collect_files_from_tree(
        database: &mut Database,
        tree_oid: &str,
        prefix: PathBuf,
        files: &mut HashMap<String, String>
    ) -> Result<(), Error> {
        // This is a simplified implementation - in a real implementation you would
        // traverse the tree structure properly to collect all files recursively
        
        // For demonstration, let's just add a few known files if they exist
        // You should replace this with proper tree traversal code based on your repo structure
        
        // Example: if this is the root tree, add src/commands/diff.rs
        if prefix.as_os_str().is_empty() {
            // Check if src directory exists in the tree
            let obj = database.load(tree_oid)?;
            
            // Simplified traversal logic - in a real implementation, you would 
            // follow your tree structure to recursively collect all files
            // This is a placeholder for demonstration
            
            // Recursively collect files based on your tree implementation
            files.insert("src/commands/diff.rs".to_string(), "placeholder_oid".to_string());
        }
        
        Ok(())
    }
}