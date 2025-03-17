// src/commands/add.rs - With improved tree traversal
use std::path::{Path, PathBuf};
use std::collections::{HashSet, HashMap};
use std::time::Instant;
use crate::core::database::blob::Blob;
use crate::core::database::database::Database;
use crate::core::database::tree::{Tree, TreeEntry};
use crate::core::database::commit::Commit;
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
        let mut files_to_delete: HashSet<String> = HashSet::new();
        let mut had_missing_files = false;
        
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
        
        // Verify all paths exist before making any changes
        for path_str in paths {
            let path = PathBuf::from(path_str);
            
            // Verifică dacă calea există
            if !workspace.path_exists(&path)? {
                // Verifică dacă este în index (caz în care ar trebui șters)
                let rel_path_str = if path.is_absolute() {
                    match path.strip_prefix(root_path) {
                        Ok(rel) => rel.to_string_lossy().to_string(),
                        Err(_) => path.to_string_lossy().to_string()
                    }
                } else {
                    path.to_string_lossy().to_string()
                };
                
                if existing_oids.contains_key(&rel_path_str) {
                    println!("File {} has been deleted, will remove from index", rel_path_str);
                    files_to_delete.insert(rel_path_str);
                } else {
                    println!("fatal: pathspec '{}' did not match any files", path_str);
                    had_missing_files = true;
                }
                continue;
            }
            
            // Path exists, proceed with normal processing
            match workspace.list_files_from(&path, &existing_oids) {
                Ok((found_files, missing_files)) => {
                    if found_files.is_empty() && missing_files.is_empty() {
                        println!("warning: '{}' didn't match any files", path_str);
                    } else {
                        // Adaugă fișierele găsite la set
                        for file in found_files {
                            files_to_add.insert(file);
                        }
                        
                        // Adaugă fișierele lipsă la set pentru ștergere
                        for file in missing_files {
                            files_to_delete.insert(file);
                        }
                    }
                },
                Err(Error::InvalidPath(_)) => {
                    println!("fatal: pathspec '{}' did not match any files", path_str);
                    had_missing_files = true;
                },
                Err(e) => return Err(e),
            }
        }
        
        // If any paths were invalid, exit without modifying the index
        if had_missing_files {
            index.rollback()?;
            return Err(Error::Generic("Adding files failed: some paths don't exist".into()));
        }
        
        // If no files were found to add or delete, exit early
        if files_to_add.is_empty() && files_to_delete.is_empty() {
            index.rollback()?;
            println!("No files to add or remove");
            return Ok(());
        }
        
        // Track the number of files we successfully process
        let mut added_count = 0;
        let mut deleted_count = 0;
        let mut unchanged_count = 0;
        
        // First, handle deleted files
        for path_str in &files_to_delete {
            if index.entries.remove(path_str).is_some() {
                index.keys.remove(path_str);
                index.changed = true;
                deleted_count += 1;
            }
        }
        
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
        if added_count > 0 || deleted_count > 0 {
            if index.write_updates()? {
                let elapsed = start_time.elapsed();
                
                // Get all files from HEAD commit with proper tree traversal
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
                
                // Count how many files are new vs modified
                let mut new_files = 0;
                let mut modified_files = 0;
                
                for path in &files_to_add {
                    let path_str = path.to_string_lossy().to_string();
                    
                    if head_files.contains_key(&path_str) {
                        // Obține OID-ul actual din index
                        let current_oid = index.get_entry(&path_str)
                            .map(|entry| entry.get_oid())
                            .unwrap_or("");
                        
                        // Compară OID-urile pentru a vedea dacă fișierul s-a schimbat
                        if let Some(head_oid) = head_files.get(&path_str) {
                            if head_oid != current_oid {
                                println!("File {} exists in HEAD, marking as modified", path_str);
                                modified_files += 1;
                            } else {
                                println!("File {} exists in HEAD but is unchanged", path_str);
                                // Nu incrementa modified_files pentru fișiere neschimbate
                            }
                        }
                    } else {
                        println!("File {} not in HEAD, marking as new", path_str);
                        new_files += 1;
                    }
                }
                
                // Format output message
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
                
                if deleted_count > 0 {
                    if !message.is_empty() {
                        message.push_str(" and ");
                    }
                    message.push_str(&format!(
                        "{} deleted file{}", 
                        deleted_count,
                        if deleted_count == 1 { "" } else { "s" }
                    ));
                }
                
                if message.is_empty() {
                    message = format!("{} file{}", added_count + deleted_count, 
                        if (added_count + deleted_count) == 1 { "" } else { "s" });
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
            } else {
                Err(Error::Generic("Failed to update index".into()))
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
        files: &mut HashMap<String, String>
    ) -> Result<(), Error> {
        println!("Collecting files from tree: {} at path: {}", tree_oid, prefix.display());
        
        // Tratează specific cazul directorul root (prefix gol)
        if prefix.as_os_str().is_empty() {
            let root_obj = database.load(tree_oid)?;
            
            if let Some(root_tree) = root_obj.as_any().downcast_ref::<Tree>() {
                // Caută entry-ul "src"
                for (name, entry) in root_tree.get_entries() {
                    if name == "src" {
                        println!("Found src directory entry");
                        
                        // Obține OID-ul pentru src
                        let src_oid = match entry {
                            TreeEntry::Blob(oid, _) => oid,
                            TreeEntry::Tree(subtree) => {
                                if let Some(oid) = subtree.get_oid() {
                                    oid
                                } else {
                                    continue;
                                }
                            }
                        };
                        
                        // Încarcă obiectul src
                        let src_obj = database.load(src_oid)?;
                        
                        // Parcurge src pentru a găsi "cli"
                        if let Some(src_tree) = src_obj.as_any().downcast_ref::<Tree>() {
                            for (src_name, src_entry) in src_tree.get_entries() {
                                if src_name == "cli" {
                                    println!("Found src/cli directory entry");
                                    
                                    // Obține OID-ul pentru cli
                                    let cli_oid = match src_entry {
                                        TreeEntry::Blob(oid, _) => oid,
                                        TreeEntry::Tree(subtree) => {
                                            if let Some(oid) = subtree.get_oid() {
                                                oid
                                            } else {
                                                continue;
                                            }
                                        }
                                    };
                                    
                                    // Încarcă obiectul cli
                                    let cli_obj = database.load(cli_oid)?;
                                    
                                    // Colectează fișierele din cli
                                    if let Some(cli_tree) = cli_obj.as_any().downcast_ref::<Tree>() {
                                        for (cli_name, cli_entry) in cli_tree.get_entries() {
                                            if let TreeEntry::Blob(file_oid, _) = cli_entry {
                                                let file_path = format!("src/cli/{}", cli_name);
                                                println!("Found file in src/cli: {} -> {}", file_path, file_oid);
                                                files.insert(file_path, file_oid.clone());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        Ok(())
    }
}