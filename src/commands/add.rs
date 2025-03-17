// src/commands/add.rs - With improved tree traversal
use std::path::{Path, PathBuf};
use std::collections::{HashSet, HashMap};
use std::time::Instant;
use crate::core::database::blob::Blob;
use crate::core::database::database::{Database, GitObject};
use crate::core::database::tree::{Tree, TreeEntry, TREE_MODE};
use crate::core::database::commit::Commit;
use crate::core::file_mode::FileMode;
use crate::core::index::index::Index;
use crate::core::workspace::Workspace;
use crate::core::refs::Refs;
use crate::errors::error::Error;
use std::fs;

pub struct AddCommand;

impl AddCommand {
    pub fn execute(paths: &[String]) -> Result<(), Error> {
        let start_time = Instant::now();
        
        if paths.is_empty() {
            return Err(Error::Generic("No paths specified for add command".into()));
        }

        let root_path = Path::new(".");
        let git_path = root_path.join(".ash");
        
        // Verify .ash directory exists
        if !git_path.exists() {
            return Err(Error::Generic("Not an ash repository (or any of the parent directories): .ash directory not found".into()));
        }
        
        let workspace = Workspace::new(root_path);
        let mut database = Database::new(git_path.join("objects"));
        let mut index = Index::new(git_path.join("index"));
        let refs = Refs::new(&git_path);
        
        // Prepare a set to deduplicate files (in case of overlapping path arguments)
        let mut files_to_add: HashSet<PathBuf> = HashSet::new();
        let mut had_missing_files = false;
        
        // Verify all paths exist before making any changes
        for path_str in paths {
            let path = PathBuf::from(path_str);
            match workspace.list_files_from(&path) {
                Ok(found_files) => {
                    if found_files.is_empty() {
                        println!("warning: '{path_str}' didn't match any files");
                    } else {
                        // Add files to our set (automatically deduplicates)
                        for file in found_files {
                            files_to_add.insert(file);
                        }
                    }
                },
                Err(Error::InvalidPath(_)) => {
                    println!("fatal: pathspec '{path_str}' did not match any files");
                    had_missing_files = true;
                },
                Err(e) => return Err(e),
            }
        }
        
        // If any paths were invalid, exit without modifying the index
        if had_missing_files {
            return Err(Error::Generic("Adding files failed: some paths don't exist".into()));
        }
        
        // If no files were found, exit early
        if files_to_add.is_empty() {
            println!("No files to add");
            return Ok(());
        }
        
        // Check for existing index.lock file before trying to acquire lock
        let lock_path = git_path.join("index.lock");
        if lock_path.exists() {
            return Err(Error::Lock(format!(
                "Unable to create '.ash/index.lock': File exists.\n\
                Another ash process seems to be running in this repository.\n\
                If it still fails, a process may have crashed in this repository earlier:\n\
                remove the file manually to continue."
            )));
        }
        
        // Try to acquire the lock on the index
        if !index.load_for_update()? {
            return Err(Error::Lock(format!(
                "Unable to acquire lock on index. Another process may be using it. \
                If not, the .ash/index.lock file may need to be manually removed."
            )));
        }
        
        // Get current files in index to avoid unnecessary operations
        let mut existing_oids = HashMap::new();
        for entry in index.each_entry() {
            existing_oids.insert(entry.get_path().to_string(), entry.oid.clone());
        }
        
        // Track the number of files we successfully add
        let mut added_count = 0;
        let mut unchanged_count = 0;
        
        // Create a buffer for batch processing
        let mut blobs_to_save: Vec<(PathBuf, Vec<u8>, fs::Metadata)> = Vec::with_capacity(files_to_add.len());
        
        // First pass: read all files and check for errors before we start modifying anything
        for file_path in &files_to_add {
            // Try to read file content and metadata
            match (
                workspace.read_file(file_path),
                workspace.stat_file(file_path)
            ) {
                (Ok(data), Ok(stat)) => {
                    // Check if file is already in index with same content
                    let file_key = file_path.to_string_lossy().to_string();
                    
                    // Pre-compute hash to check if the file has changed
                    // Use the refactored Database's hash_file_data method
                    let new_oid = database.hash_file_data(&data);
                    
                    if let Some(old_oid) = existing_oids.get(&file_key) {
                        if old_oid == &new_oid {
                            // File exists in index with same content, skip it
                            unchanged_count += 1;
                            continue;
                        }
                    }
                    
                    // Queue file for processing
                    blobs_to_save.push((file_path.clone(), data, stat));
                },
                (Err(Error::IO(e)), _) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                    // Permission denied error
                    index.rollback()?;
                    return Err(Error::Generic(format!(
                        "error: open('{}'): Permission denied\nfatal: adding files failed",
                        file_path.display()
                    )));
                },
                (Err(e), _) => {
                    // Other read errors
                    index.rollback()?;
                    return Err(Error::Generic(format!(
                        "error: Failed to read '{}': {}\nfatal: adding files failed",
                        file_path.display(), e
                    )));
                },
                (_, Err(e)) => {
                    // Metadata errors
                    index.rollback()?;
                    return Err(Error::Generic(format!(
                        "error: Failed to get stats for '{}': {}\nfatal: adding files failed",
                        file_path.display(), e
                    )));
                }
            }
        }
        
        // Second pass: process all files that need to be updated
        for (file_path, data, stat) in blobs_to_save {
            // Create and store the blob
            let mut blob = Blob::new(data);
            if let Err(e) = database.store(&mut blob) {
                // Release the lock if we fail to store the blob
                index.rollback()?;
                return Err(Error::Generic(format!(
                    "Failed to store blob for '{}': {}", file_path.display(), e
                )));
            }
            
            // Get the OID
            let oid = match blob.get_oid() {
                Some(id) => id,
                None => {
                    // Release the lock if the blob has no OID
                    index.rollback()?;
                    return Err(Error::Generic(
                        "Blob OID not set after storage".into()
                    ));
                }
            };
            
            // Add to index
            if let Err(e) = index.add(&file_path, oid, &stat) {
                index.rollback()?;
                return Err(e);
            }
            
            added_count += 1;
        }
        
        // Write index updates
        if added_count > 0 {
            match index.write_updates()? {
                true => {
                    let elapsed = start_time.elapsed();
                    
                    // Step 1: Get all files from HEAD commit with proper tree traversal
                    let mut head_files = HashMap::<String, String>::new(); // path -> oid
                    
                    // Only load from HEAD if we have a commit
                    if let Ok(Some(head_oid)) = refs.read_head() {
                        println!("Examining HEAD commit: {}", head_oid);
                        
                        if let Ok(commit_obj) = database.load(&head_oid) {
                            if let Some(commit) = commit_obj.as_any().downcast_ref::<Commit>() {
                                let root_tree_oid = commit.get_tree();
                                println!("Root tree OID: {}", root_tree_oid);
                                
                                // Recursively collect all files from HEAD tree
                                Self::collect_files_from_tree(&mut database, root_tree_oid, PathBuf::new(), &mut head_files)?;
                                
                                println!("Found {} files in HEAD", head_files.len());
                                for (path, oid) in &head_files {
                                    println!("  {} -> {}", path, oid);
                                }
                            }
                        }
                    }
                    
                    // Step 2: Count how many files are new vs modified
                    let mut new_files = 0;
                    let mut modified_files = 0;
                    
                    for path in &files_to_add {
                        let path_str = path.to_string_lossy().to_string();
                        
                        if head_files.contains_key(&path_str) {
                            println!("File {} exists in HEAD, marking as modified", path_str);
                            modified_files += 1;
                        } else {
                            println!("File {} not in HEAD, marking as new", path_str);
                            new_files += 1;
                        }
                    }
                    
                    // Step 3: Format output message
                    let mut message = String::new();
                    
                    if new_files > 0 {
                        message.push_str(&format!(
                            "{} new file{}", 
                            new_files,
                            if new_files == 1 { "" } else { "s" }
                        ));
                    }
                    
                    if modified_files > 0 {
                        if !message.is_empty() {
                            message.push_str(" and ");
                        }
                        message.push_str(&format!(
                            "{} modified file{}", 
                            modified_files,
                            if modified_files == 1 { "" } else { "s" }
                        ));
                    }
                    
                    if message.is_empty() {
                        message = format!("{} file{}", added_count, if added_count == 1 { "" } else { "s" });
                    }
                    
                    if unchanged_count > 0 {
                        println!(
                            "{} added to index, {} file{} unchanged ({:.2}s)",
                            message,
                            unchanged_count,
                            if unchanged_count == 1 { "" } else { "s" },
                            elapsed.as_secs_f32()
                        );
                    } else {
                        println!(
                            "{} added to index ({:.2}s)",
                            message,
                            elapsed.as_secs_f32()
                        );
                    }
                    Ok(())
                },
                false => Err(Error::Generic("Failed to update index".into())),
            }
        } else if unchanged_count > 0 {
            // If we didn't add any files, release the lock
            index.rollback()?;
            println!(
                "No files changed, {} file{} already up to date",
                unchanged_count,
                if unchanged_count == 1 { "" } else { "s" }
            );
            Ok(())
        } else {
            // If we didn't add any files, release the lock
            index.rollback()?;
            println!("No changes were made to the index");
            Ok(())
        }
    }
    
    /// Recursively collect all files from a tree and its subtrees
fn collect_files_from_tree(
    database: &mut Database,
    tree_oid: &str,
    prefix: PathBuf,
    files: &mut HashMap<String, String> // map of path -> oid
) -> Result<(), Error> {
    println!("Collecting files from tree: {} at path: {}", tree_oid, prefix.display());
    
    // Load the object
    let obj = database.load(tree_oid)?;
    println!("Loaded object type: {}", obj.get_type());
    
    // Step 1: Try to process as a tree
    if let Some(tree) = obj.as_any().downcast_ref::<Tree>() {
        println!("Processing tree with {} entries", tree.get_entries().len());
        
        // Process each entry in the tree
        for (name, entry) in tree.get_entries() {
            let entry_path = if prefix.as_os_str().is_empty() {
                PathBuf::from(name)
            } else {
                prefix.join(name)
            };
            
            let entry_path_str = entry_path.to_string_lossy().to_string();
            
            match entry {
                TreeEntry::Blob(blob_oid, mode) => {
                    // Check if this is actually a directory entry
                    if *mode == TREE_MODE || FileMode::is_directory(*mode) {
                        println!("Found directory as blob: {} -> {}", entry_path_str, blob_oid);
                        // Recursively process this directory
                        Self::collect_files_from_tree(database, blob_oid, entry_path, files)?;
                    } else {
                        // Regular file entry
                        println!("Found file: {} -> {}", entry_path_str, blob_oid);
                        files.insert(entry_path_str, blob_oid.clone());
                    }
                },
                TreeEntry::Tree(subtree) => {
                    if let Some(subtree_oid) = subtree.get_oid() {
                        println!("Found subtree: {} -> {}", entry_path_str, subtree_oid);
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
    
    // Step 2: If not a tree, try to parse as a tree (handle directories stored as blobs)
    if obj.get_type() == "blob" {
        println!("Object is a blob, trying to parse as tree: {}", tree_oid);
        
        let blob_data = obj.to_bytes();
        match Tree::parse(&blob_data) {
            Ok(tree) => {
                println!("Successfully parsed blob as tree with {} entries", tree.get_entries().len());
                
                // Process each entry in the parsed tree
                for (name, entry) in tree.get_entries() {
                    let entry_path = if prefix.as_os_str().is_empty() {
                        PathBuf::from(name)
                    } else {
                        prefix.join(name)
                    };
                    
                    let entry_path_str = entry_path.to_string_lossy().to_string();
                    
                    match entry {
                        TreeEntry::Blob(blob_oid, mode) => {
                            if *mode == TREE_MODE || FileMode::is_directory(*mode) {
                                println!("Found directory in parsed tree: {} -> {}", entry_path_str, blob_oid);
                                // Recursively process this directory
                                Self::collect_files_from_tree(database, blob_oid, entry_path, files)?;
                            } else {
                                println!("Found file in parsed tree: {} -> {}", entry_path_str, blob_oid);
                                files.insert(entry_path_str, blob_oid.clone());
                            }
                        },
                        TreeEntry::Tree(subtree) => {
                            if let Some(subtree_oid) = subtree.get_oid() {
                                println!("Found subtree in parsed tree: {} -> {}", entry_path_str, subtree_oid);
                                // Recursively process this directory
                                Self::collect_files_from_tree(database, subtree_oid, entry_path, files)?;
                            }
                        }
                    }
                }
            },
            Err(e) => {
                println!("Failed to parse blob as tree: {}", e);
                
                // If this is the root directory, that's a problem
                if prefix.as_os_str().is_empty() {
                    println!("Warning: Root tree cannot be parsed");
                } else {
                    // Otherwise, this might be a regular file
                    let path_str = prefix.to_string_lossy().to_string();
                    println!("Treating {} as a regular file", path_str);
                    files.insert(path_str, tree_oid.to_string());
                }
            }
        }
        
        return Ok(());
    }
    
    println!("Object {} is neither a tree nor a blob that can be parsed as a tree", tree_oid);
    Ok(())
}
}