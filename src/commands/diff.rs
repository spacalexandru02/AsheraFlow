// src/commands/diff.rs
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::time::Instant;

use crate::core::color::Color;
use crate::core::database::database::Database;
use crate::core::database::tree::{Tree, TreeEntry, TREE_MODE};
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
        println!("{}", Color::cyan(&format!("Diff completed in {:.2}s", elapsed.as_secs_f32())));
        
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
                let path_str = path.display().to_string();
                println!("diff --ash a/{} b/{}", Color::cyan(&path_str), Color::cyan(&path_str));
                println!("{} {}", Color::red("deleted file mode"), Color::red(&entry.mode_octal()));
                println!("--- a/{}", Color::red(&path_str));
                println!("+++ {}", Color::red("/dev/null"));
                
                // Get blob content from database
                let blob_obj = database.load(entry.get_oid())?;
                let content = blob_obj.to_bytes();
                let lines = diff::split_lines(&String::from_utf8_lossy(&content));
                
                // Show deletion diff
                for line in &lines {
                    println!("{}", Color::red(&format!("-{}", line)));
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
            let path_str = path.display().to_string();
            println!("diff --ash a/{} b/{}", Color::cyan(&path_str), Color::cyan(&path_str));
            
            // Get diff between index and working copy
            let raw_diff_output = diff::diff_with_database(workspace, database, path, entry.get_oid(), 3)?;
            
            // Add colors to the diff output
            let colored_diff = Self::colorize_diff_output(&raw_diff_output);
            print!("{}", colored_diff);
        }
        
        if !has_changes {
            println!("{}", Color::green("No changes"));
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
                println!("{}", Color::yellow("No HEAD commit found. Index contains initial version."));
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
                println!("diff --ash a/{} b/{}", Color::cyan(path), Color::cyan(path));
                
                // Load both versions
                let head_obj = database.load(head_oid)?;
                let index_obj = database.load(entry.get_oid())?;
                
                let head_content = head_obj.to_bytes();
                let index_content = index_obj.to_bytes();
                
                let head_lines = diff::split_lines(&String::from_utf8_lossy(&head_content));
                let index_lines = diff::split_lines(&String::from_utf8_lossy(&index_content));
                
                // Calculate diff
                let edits = diff_lines(&head_lines, &index_lines);
                let raw_diff = format_diff(&head_lines, &index_lines, &edits, 3);
                
                // Colorize and print the diff
                let colored_diff = Self::colorize_diff_output(&raw_diff);
                print!("{}", colored_diff);
            } else {
                // File exists in index but not in HEAD (new file)
                has_changes = true;
                println!("diff --ash a/{} b/{}", Color::cyan(path), Color::cyan(path));
                println!("{} {}", Color::green("new file mode"), Color::green(&entry.mode_octal()));
                println!("--- {}", Color::red("/dev/null"));
                println!("+++ b/{}", Color::green(path));
                
                // Load index version
                let index_obj = database.load(entry.get_oid())?;
                let content = index_obj.to_bytes();
                let lines = diff::split_lines(&String::from_utf8_lossy(&content));
                
                // Show addition diff
                for line in &lines {
                    println!("{}", Color::green(&format!("+{}", line)));
                }
            }
        }
        
        // Check for files in HEAD that were removed from index
        for (path, head_oid) in &head_files {
            if !index.tracked(path) {
                // File was in HEAD but removed from index
                has_changes = true;
                println!("diff --ash a/{} b/{}", Color::cyan(path), Color::cyan(path));
                println!("{}", Color::red("deleted file"));
                println!("--- a/{}", Color::red(path));
                println!("+++ {}", Color::red("/dev/null"));
                
                // Load HEAD version
                let head_obj = database.load(head_oid)?;
                let content = head_obj.to_bytes();
                let lines = diff::split_lines(&String::from_utf8_lossy(&content));
                
                // Show deletion diff
                for line in &lines {
                    println!("{}", Color::red(&format!("-{}", line)));
                }
            }
        }
        
        if !has_changes {
            println!("{}", Color::green("No changes staged for commit"));
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
                        println!("diff --ash a/{} b/{}", Color::cyan(&path_str), Color::cyan(&path_str));
                        println!("{} {}", Color::green("new file mode"), Color::green(&entry.mode_octal()));
                        println!("--- {}", Color::red("/dev/null"));
                        println!("+++ b/{}", Color::green(&path_str));
                        
                        // Load index version
                        let index_obj = database.load(entry.get_oid())?;
                        let content = index_obj.to_bytes();
                        let lines = diff::split_lines(&String::from_utf8_lossy(&content));
                        
                        for line in &lines {
                            println!("{}", Color::green(&format!("+{}", line)));
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
                        println!("{}", Color::green(&format!("No changes staged for {}", path_str)));
                        return Ok(());
                    }
                    
                    // Compare HEAD and index versions
                    println!("diff --ash a/{} b/{}", Color::cyan(&path_str), Color::cyan(&path_str));
                    
                    // Load both versions
                    let head_obj = database.load(head_oid)?;
                    let index_obj = database.load(entry.get_oid())?;
                    
                    let head_content = head_obj.to_bytes();
                    let index_content = index_obj.to_bytes();
                    
                    let head_lines = diff::split_lines(&String::from_utf8_lossy(&head_content));
                    let index_lines = diff::split_lines(&String::from_utf8_lossy(&index_content));
                    
                    // Calculate diff
                    let edits = diff_lines(&head_lines, &index_lines);
                    let raw_diff = format_diff(&head_lines, &index_lines, &edits, 3);
                    
                    // Colorize and print the diff
                    let colored_diff = Self::colorize_diff_output(&raw_diff);
                    print!("{}", colored_diff);
                } else {
                    // File is in index but not in HEAD (new file)
                    println!("diff --ash a/{} b/{}", Color::cyan(&path_str), Color::cyan(&path_str));
                    println!("{} {}", Color::green("new file mode"), Color::green(&entry.mode_octal()));
                    println!("--- {}", Color::red("/dev/null"));
                    println!("+++ b/{}", Color::green(&path_str));
                    
                    // Load index version
                    let index_obj = database.load(entry.get_oid())?;
                    let content = index_obj.to_bytes();
                    let lines = diff::split_lines(&String::from_utf8_lossy(&content));
                    
                    for line in &lines {
                        println!("{}", Color::green(&format!("+{}", line)));
                    }
                }
            } else {
                // Compare index with working tree
                if !workspace.path_exists(path)? {
                    println!("diff --ash a/{} b/{}", Color::cyan(&path_str), Color::cyan(&path_str));
                    println!("{} {}", Color::red("deleted file mode"), Color::red(&entry.mode_octal()));
                    println!("--- a/{}", Color::red(&path_str));
                    println!("+++ {}", Color::red("/dev/null"));
                    
                    // Load index version
                    let index_obj = database.load(entry.get_oid())?;
                    let content = index_obj.to_bytes();
                    let lines = diff::split_lines(&String::from_utf8_lossy(&content));
                    
                    for line in &lines {
                        println!("{}", Color::red(&format!("-{}", line)));
                    }
                    
                    return Ok(());
                }
                
                // Read working copy
                let file_content = workspace.read_file(path)?;
                
                // Calculate hash for the file content
                let file_hash = database.hash_file_data(&file_content);
                
                // If the hash matches, there's no change
                if file_hash == entry.get_oid() {
                    println!("{}", Color::green(&format!("No changes in {}", path_str)));
                    return Ok(());
                }
                
                // Show diff between index and working copy
                println!("diff --ash a/{} b/{}", Color::cyan(&path_str), Color::cyan(&path_str));
                
                let raw_diff_output = diff::diff_with_database(workspace, database, path, entry.get_oid(), 3)?;
                
                // Colorize and print the diff
                let colored_diff = Self::colorize_diff_output(&raw_diff_output);
                print!("{}", colored_diff);
            }
        } else {
            // Path not in index
            if workspace.path_exists(path)? {
                println!("{}", Color::red(&format!("error: path '{}' is untracked", path_str)));
            } else {
                println!("{}", Color::red(&format!("error: path '{}' does not exist", path_str)));
            }
        }
        
        Ok(())
    }
    
    /// Helper method to colorize diff output
    fn colorize_diff_output(diff: &str) -> String {
        let mut result = String::new();
        
        for line in diff.lines() {
            if line.starts_with("@@") && line.contains("@@") {
                // Hunk header
                result.push_str(&Color::cyan(line));
                result.push('\n');
            } else if line.starts_with('+') {
                // Added line
                result.push_str(&Color::green(line));
                result.push('\n');
            } else if line.starts_with('-') {
                // Removed line
                result.push_str(&Color::red(line));
                result.push('\n');
            } else {
                // Context line
                result.push_str(line);
                result.push('\n');
            }
        }
        
        result
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
    
    fn collect_files_from_tree(
        database: &mut Database,
        tree_oid: &str,
        prefix: PathBuf,
        files: &mut HashMap<String, String>
    ) -> Result<(), Error> {
        println!("Traversing tree: {} at path: {}", tree_oid, prefix.display());
        
        // Load the object
        let obj = database.load(tree_oid)?;
        
        // Check if the object is a tree
        if let Some(tree) = obj.as_any().downcast_ref::<Tree>() {
            // Process each entry in the tree
            for (name, entry) in tree.get_entries() {
                let entry_path = if prefix.as_os_str().is_empty() {
                    PathBuf::from(name)
                } else {
                    prefix.join(name)
                };
                
                let entry_path_str = entry_path.to_string_lossy().to_string();
                
                match entry {
                    TreeEntry::Blob(oid, mode) => {
                        // If this is a directory entry masquerading as a blob
                        if *mode == TREE_MODE || mode.is_directory() {
                            println!("Found directory stored as blob: {} -> {}", entry_path_str, oid);
                            // Recursively process this directory
                            Self::collect_files_from_tree(database, oid, entry_path, files)?;
                        } else {
                            // Regular file
                            println!("Found file: {} -> {}", entry_path_str, oid);
                            files.insert(entry_path_str, oid.clone());
                        }
                    },
                    TreeEntry::Tree(subtree) => {
                        if let Some(subtree_oid) = subtree.get_oid() {
                            println!("Found directory: {} -> {}", entry_path_str, subtree_oid);
                            // Recursively process this directory
                            Self::collect_files_from_tree(database, subtree_oid, entry_path, files)?;
                        } else {
                            println!("Warning: Tree entry without OID: {}", entry_path_str);
                        }
                    }
                }
            }
            
            return Ok(());
        }
        
        // If object is a blob, try to parse it as a tree
        if obj.get_type() == "blob" {
            println!("Object is a blob, attempting to parse as tree...");
            
            // Attempt to parse blob as a tree (this handles directories stored as blobs)
            let blob_data = obj.to_bytes();
            match Tree::parse(&blob_data) {
                Ok(parsed_tree) => {
                    println!("Successfully parsed blob as tree with {} entries", parsed_tree.get_entries().len());
                    
                    // Process each entry in the parsed tree
                    for (name, entry) in parsed_tree.get_entries() {
                        let entry_path = if prefix.as_os_str().is_empty() {
                            PathBuf::from(name)
                        } else {
                            prefix.join(name)
                        };
                        
                        let entry_path_str = entry_path.to_string_lossy().to_string();
                        
                        match entry {
                            TreeEntry::Blob(oid, mode) => {
                                if *mode == TREE_MODE || mode.is_directory() {
                                    println!("Found directory in parsed tree: {} -> {}", entry_path_str, oid);
                                    // Recursively process this directory
                                    Self::collect_files_from_tree(database, oid, entry_path, files)?;
                                } else {
                                    println!("Found file in parsed tree: {} -> {}", entry_path_str, oid);
                                    files.insert(entry_path_str, oid.clone());
                                }
                            },
                            TreeEntry::Tree(subtree) => {
                                if let Some(subtree_oid) = subtree.get_oid() {
                                    println!("Found directory in parsed tree: {} -> {}", entry_path_str, subtree_oid);
                                    // Recursively process this directory
                                    Self::collect_files_from_tree(database, subtree_oid, entry_path, files)?;
                                } else {
                                    println!("Warning: Tree entry without OID in parsed tree: {}", entry_path_str);
                                }
                            }
                        }
                    }
                    
                    return Ok(());
                },
                Err(e) => {
                    // If we're at a non-root path, this might be a file
                    if !prefix.as_os_str().is_empty() {
                        let path_str = prefix.to_string_lossy().to_string();
                        println!("Adding file at path: {} -> {}", path_str, tree_oid);
                        files.insert(path_str, tree_oid.to_string());
                        return Ok(());
                    }
                    
                    println!("Failed to parse blob as tree: {}", e);
                }
            }
        }
        
        // Special case for top-level entries that might need deeper traversal
        // This handles cases where we have entries like "src" but need to explore "src/commands"
        if prefix.as_os_str().is_empty() {
            // Check all found entries in the root
            for (path, oid) in files.clone() {  // Clone to avoid borrowing issues
                // Only look at top-level directory entries (no path separators)
                if !path.contains('/') {
                    println!("Checking top-level entry for deeper traversal: {} -> {}", path, oid);
                    
                    // Try to load and traverse it as a directory
                    let dir_path = PathBuf::from(&path);
                    if let Err(e) = Self::collect_files_from_tree(database, &oid, dir_path, files) {
                        println!("Error traversing {}: {}", path, e);
                        // Continue with other entries even if this one fails
                    }
                }
            }
        }
        
        println!("Object {} is neither a tree nor a blob that can be parsed as a tree", tree_oid);
        Ok(())
    }
}