// src/commands/commit.rs

use std::path::PathBuf;
use std::{env, path::Path, time::Instant};
use std::collections::{HashMap, HashSet};
use crate::core::database::tree::{TreeEntry, TREE_MODE};
use crate::{core::{database::{author::Author, commit::Commit, database::Database, entry::DatabaseEntry, tree::Tree}, index::index::Index, refs::Refs}, errors::error::Error};

// Adaugă use pentru logging
use log::{debug, info, warn, error};
use log;

pub struct CommitCommand;

impl CommitCommand {
    pub fn execute(message: &str) -> Result<(), Error> {
        let start_time = Instant::now();

        // Validate the commit message
        if message.trim().is_empty() {
            // Folosim error! pentru logarea internă a motivului erorii
            error!("Commit aborted due to empty commit message.");
            return Err(Error::Generic("Aborting commit due to empty commit message".into()));
        }

        info!("Starting commit execution"); // Info

        let root_path = Path::new(".");
        let git_path = root_path.join(".ash");

        // Verify .ash directory exists
        if !git_path.exists() {
            error!(".ash directory not found at {}", root_path.display()); // Error
            return Err(Error::Generic("Not an ash repository (or any of the parent directories): .ash directory not found".into()));
        }

        let db_path = git_path.join("objects");

        debug!("Initializing components"); // Debug
        let mut database = Database::new(db_path);

        // Check for the index file
        let index_path = git_path.join("index");
        if !index_path.exists() {
            error!("Index file not found at {}", index_path.display()); // Error
            return Err(Error::Generic("No index file found. Please add some files first.".into()));
        }

        // Check for existing index.lock file before trying to load the index
        let index_lock_path = git_path.join("index.lock");
        if index_lock_path.exists() {
            error!("Index lock file exists: {}", index_lock_path.display()); // Error
            return Err(Error::Lock(format!(
                "Unable to create '.ash/index.lock': File exists.\n\
                Another ash process seems to be running in this repository.\n\
                If it still fails, a process may have crashed in this repository earlier:\n\
                remove the file manually to continue."
            )));
        }

        let mut index = Index::new(index_path);

        info!("Loading index"); // Info
        // Load the index (read-only is sufficient for commit)
        match index.load() {
            Ok(_) => info!("Index loaded successfully"), // Info
            Err(e) => {
                error!("Error loading index: {}", e); // Error
                return Err(Error::Generic(format!("Error loading index: {}", e)));
            }
        }

        // Check for HEAD lock
        let head_lock_path = git_path.join("HEAD.lock");
        if head_lock_path.exists() {
            error!("HEAD lock file exists: {}", head_lock_path.display()); // Error
            return Err(Error::Lock(format!(
                "Unable to create '.ash/HEAD.lock': File exists.\n\
                Another ash process seems to be running in this repository.\n\
                If it still fails, a process may have crashed in this repository earlier:\n\
                remove the file manually to continue."
            )));
        }

        let refs = Refs::new(&git_path);

        info!("Reading HEAD"); // Info
        // Get the parent commit OID
        let parent = match refs.read_head() {
            Ok(p) => {
                info!("HEAD read successfully: {:?}", p); // Info
                p
            },
            Err(e) => {
                error!("Error reading HEAD: {:?}", e); // Error
                return Err(e);
            }
        };

        // Convert index entries to database entries (only stage 0)
        let database_entries: Vec<DatabaseEntry> = index.each_entry()
            .filter(|entry| entry.stage == 0)
            .map(|index_entry| {
                DatabaseEntry::new(
                    index_entry.path.clone(),
                    index_entry.oid.clone(),
                    &index_entry.mode_octal()
                )
            })
            .collect();
        debug!("Collected {} stage 0 entries from index.", database_entries.len()); // Debug

        // --- Verificare Modificări ---
        debug!("Building tree from index entries..."); // Debug
        let mut root = Tree::build(database_entries.iter())?;

        debug!("Storing trees to database to determine current tree OID..."); // Debug
        root.traverse(|tree| database.store(tree).map(|_| ()))?;
        let tree_oid = root.get_oid()
            .ok_or(Error::Generic("Tree OID not set after storage".into()))?
            .clone();
        info!("Current index tree OID: {}", tree_oid); // Info

        let mut no_changes = false;
        if let Some(parent_oid) = &parent {
            match database.load(parent_oid) {
                Ok(parent_obj) => {
                    if let Some(parent_commit) = parent_obj.as_any().downcast_ref::<Commit>() {
                        let parent_tree_oid = parent_commit.get_tree();
                        info!("Parent commit tree OID: {}", parent_tree_oid); // Info
                        if &tree_oid == parent_tree_oid {
                            info!("Tree OIDs match. No changes detected."); // Info
                            no_changes = true;
                        } else {
                            debug!("Tree OIDs differ: Current={}, Parent={}", tree_oid, parent_tree_oid); // Debug
                        }
                    } else {
                        warn!("Parent OID {} did not resolve to a Commit object.", parent_oid); // Warn
                    }
                },
                Err(e) => {
                    warn!("Could not load parent commit object {}: {}", parent_oid, e); // Warn
                     if database_entries.is_empty() {
                         no_changes = true;
                     }
                }
            }
        } else {
            if database_entries.is_empty() {
                info!("Index is empty for root commit. No changes."); // Info
                no_changes = true;
            } else {
                 info!("Root commit with entries detected."); // Info
            }
        }

        if no_changes {
            return Err(Error::Generic("No changes staged for commit.".into()));
        }
        // --- Sfârșit Verificare Modificări ---

        // --- Creare Commit ---
        info!("Proceeding with commit creation..."); // Info
        debug!("Index entries being committed:"); // Debug
        for entry in &database_entries {
            debug!("  Path: {}  OID: {}  Mode: {}", entry.get_name(), entry.get_oid(), entry.get_mode()); // Debug
        }

        // Verificare obiecte lipsă (opțional, dar sigur)
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
            let mut error_msg = String::from("Error: The following objects needed for the commit are missing from the object database:\n");
            for (oid, path) in &missing_objects { // Use reference
                error_msg.push_str(&format!("  {} (referenced by {})\n", oid, path));
            }
            error_msg.push_str("\nAborting commit. This might indicate index corruption or an incomplete 'add'. Try adding the affected files again.");
            error!("Commit aborted due to missing objects: {:?}", missing_objects); // Error
            return Err(Error::Generic(error_msg));
        }

        debug!("Using pre-calculated Tree OID: {}", tree_oid); // Debug
        if let Some(parent_oid) = &parent {
            debug!("Using Parent OID: {}", parent_oid); // Debug
        }
        debug!("Message: {}", message); // Debug

        let name = match env::var("GIT_AUTHOR_NAME").or_else(|_| env::var("USER")) {
            Ok(name) => name,
            Err(_) => {
                 error!("Unable to determine author name."); // Error
                 return Err(Error::Generic(
                    "Unable to determine author name. Please set GIT_AUTHOR_NAME environment variable".into()
                 ));
            }
        };
        let email = match env::var("GIT_AUTHOR_EMAIL") {
            Ok(email) => email,
            Err(_) => {
                 warn!("GIT_AUTHOR_EMAIL not set, using fallback."); // Warn
                 format!("{}@{}", name, "localhost")
            },
        };
        let author = Author::new(name, email);
        debug!("Commit author: {}", author); // Debug

        let mut commit = Commit::new( parent.clone(), tree_oid.clone(), author, message.to_string() );

        if let Err(e) = database.store(&mut commit) {
            error!("Failed to store commit object: {}", e); // Error
            return Err(Error::Generic(format!("Failed to store commit: {}", e)));
        }

        let commit_oid = commit.get_oid()
            .ok_or_else(|| { // Folosim or_else pentru a loga eroarea
                error!("Commit OID was not set after storage."); // Error
                Error::Generic("Commit OID not set after storage".into())
            })?
            .clone();
        info!("Stored commit object with OID: {}", commit_oid); // Info

        // Update HEAD
        if let Err(e) = refs.update_head(&commit_oid) {
            error!("Failed to update HEAD to {}: {}", commit_oid, e); // Error
            return Err(Error::Generic(format!("Failed to update HEAD: {}", e)));
        }
        info!("Updated HEAD to {}", commit_oid); // Info

        // --- Calculare fișiere modificate pentru rezumat ---
        let mut changed_files = 0;
        if let Some(parent_oid) = &parent {
            if let Ok(parent_obj) = database.load(parent_oid) {
                if let Some(parent_commit) = parent_obj.as_any().downcast_ref::<Commit>() {
                    let parent_tree_oid = parent_commit.get_tree();
                    let mut parent_files = HashMap::<String, String>::new();
                    let mut current_files = HashMap::<String, String>::new();
                    let _ = Self::collect_files_from_tree(&mut database, parent_tree_oid, PathBuf::new(), &mut parent_files);
                    let _ = Self::collect_files_from_tree(&mut database, &tree_oid, PathBuf::new(), &mut current_files);
                    debug!("Files in parent commit tree ({}): {}", parent_tree_oid, parent_files.len()); // Debug
                    debug!("Files in current commit tree ({}): {}", tree_oid, current_files.len()); // Debug
                    let all_paths: HashSet<_> = parent_files.keys().chain(current_files.keys()).collect();
                    for path in all_paths {
                        match (parent_files.get(path), current_files.get(path)) {
                            (Some(old_oid), Some(new_oid)) if old_oid != new_oid => changed_files += 1,
                            (None, Some(_)) => changed_files += 1,
                            (Some(_), None) => changed_files += 1,
                            _ => {}
                        }
                    }
                    debug!("Changed files calculated by diff: {}", changed_files); // Debug
                }
            } else {
                 warn!("Could not load parent commit {} during change calculation.", parent_oid); // Warn
                 changed_files = database_entries.len(); // Fallback: count all entries
            }
        } else {
            changed_files = database_entries.len();
             debug!("Changed files (root commit): {}", changed_files); // Debug
        }
        // --- Sfârșit calcul fișiere modificate ---

        // --- Afișare Rezumat Commit (pe stdout) ---
        let is_root = if parent.is_none() { "(root-commit) " } else { "" };
        let first_line = message.lines().next().unwrap_or("");
        let short_oid = &commit_oid[0..std::cmp::min(7, commit_oid.len())];
        let elapsed = start_time.elapsed();

        // Folosim println! pentru output-ul final către utilizator
        println!(
            "[{}{}] {} ({:.2}s)",
            is_root,
            short_oid,
            first_line,
            elapsed.as_secs_f32()
        );
        println!(
            "{} file{} changed",
            changed_files,
            if changed_files == 1 { "" } else { "s" }
        );
        // --- Sfârșit Rezumat Commit ---

        // Optional: Inspect tree structure for debugging (poate fi comentat)
        // debug!("Inspecting final tree structure:");
        // Tree::inspect_tree_structure(&mut database, &tree_oid, 0)?;

        Ok(())
    }

    // Funcția de colectare a fișierelor din arbore (rămasă neschimbată funcțional, dar logurile sunt debug)
    fn collect_files_from_tree(
        database: &mut Database,
        tree_oid: &str,
        prefix: PathBuf,
        files: &mut HashMap<String, String>
    ) -> Result<(), Error> {
        //debug!("Traversing tree: {} at path: {}", tree_oid, prefix.display());

        let obj = database.load(tree_oid)?;

        if let Some(tree) = obj.as_any().downcast_ref::<Tree>() {
            for (name, entry) in tree.get_entries() {
                let entry_path = if prefix.as_os_str().is_empty() { PathBuf::from(name) } else { prefix.join(name) };
                let entry_path_str = entry_path.to_string_lossy().to_string();
                match entry {
                    TreeEntry::Blob(oid, mode) => {
                        if *mode == TREE_MODE || mode.is_directory() {
                            //debug!("Found directory stored as blob: {} -> {}", entry_path_str, oid);
                            Self::collect_files_from_tree(database, &oid, entry_path, files)?;
                        } else {
                            //debug!("Found file: {} -> {}", entry_path_str, oid);
                            files.insert(entry_path_str, oid.clone());
                        }
                    },
                    TreeEntry::Tree(subtree) => {
                        if let Some(subtree_oid) = subtree.get_oid() {
                            //debug!("Found directory: {} -> {}", entry_path_str, subtree_oid);
                            Self::collect_files_from_tree(database, subtree_oid, entry_path, files)?;
                        } else { warn!("Tree entry without OID: {}", entry_path_str); }
                    }
                }
            }
            return Ok(());
        }

        // ... (restul funcției rămâne la fel, logurile pot fi ajustate la debug/trace) ...
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
                                } else { warn!("Tree entry without OID in parsed tree: {}", entry_path_str); }
                            }
                        }
                    }
                    return Ok(());
                },
                Err(_e) => {
                    if !prefix.as_os_str().is_empty() {
                        let path_str = prefix.to_string_lossy().to_string();
                        files.insert(path_str, tree_oid.to_string());
                        return Ok(());
                    }
                     // debug!("Failed to parse blob {} as tree: {}", tree_oid, e); // Poate fi prea zgomotos
                }
            }
        }
        // debug!("Object {} is neither a tree nor a blob that can be parsed as a tree", tree_oid); // Poate fi prea zgomotos
        Ok(())
    }
}