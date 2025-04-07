use std::time::Instant;
use std::env;
use std::path::{Path, PathBuf};
use std::collections::{HashMap, HashSet};
use crate::errors::error::Error;
use crate::core::merge::inputs::Inputs;
use crate::core::merge::resolve::Resolve;
use crate::core::refs::Refs;
use crate::core::database::database::Database;
use crate::core::database::commit::Commit;
use crate::core::database::author::Author;
use crate::core::path_filter::PathFilter;
use crate::core::workspace::Workspace;
use crate::core::database::tree::{Tree, TreeEntry};
use crate::core::file_mode::FileMode;
use crate::core::database::entry::DatabaseEntry;
use crate::core::index::index::Index;
use crate::core::repository::repository::Repository; // Importat pentru a avea acces la toate componentele

// Importuri noi necesare pentru verificare
use crate::core::repository::inspector::{Inspector, ChangeType};

use log::{debug, info, warn, error};

pub struct MergeCommand;

impl MergeCommand {
    pub fn execute(revision: &str, message: Option<&str>) -> Result<(), Error> {
        let start_time = Instant::now();

        info!("Merge started...");

        // --- Debug: Print environment details ---
        debug!("==== Merge Environment Debug ====");
        // ... (restul codului de debug pentru mediu) ...
        println!("================================");
        // --- End Debug ---

        // Inițializează Repository complet
        let mut repo = Repository::new(".")?;

        // Obține referințe
        let workspace = &repo.workspace;
        let refs = &repo.refs;
        let mut database = &mut repo.database; // Acum mutabil
        let index = &mut repo.index;

        // --- Lock index EARLY ---
        if !index.load_for_update()? {
             return Err(Error::Lock("Failed to acquire lock on index".to_string()));
        }

        let result = (|| { // Start closure

            // Verificare conflicte existente în index
            if index.has_conflict() {
                 error!("Cannot merge with existing conflicts in the index.");
                return Err(Error::Generic("Cannot merge with conflicts. Fix conflicts and commit first.".into()));
            }

            // Obține HEAD OID
            let head_oid = match refs.read_head()? {
                Some(oid) => oid,
                None => {
                    error!("No HEAD commit found.");
                    return Err(Error::Generic("No HEAD commit found. Create an initial commit first.".into()));
                }
            };

            // Calculează inputurile pentru merge
            // Trecem &mut database pentru că Inputs::new poate avea nevoie să încarce obiecte
            let inputs = Inputs::new(&mut database, &refs, "HEAD".to_string(), revision.to_string())?;
            debug!("Merge inputs prepared: Base OIDs: {:?}, Left OID: {}, Right OID: {}", inputs.base_oids, inputs.left_oid, inputs.right_oid);

            let base_oid = inputs.base_oids.first().map(String::as_str);
            let target_oid = &inputs.right_oid;

            // *** START: Verificări pre-merge ***
            // 1. Verifică conflictele cu fișiere neurmărite
            info!("Checking for untracked files that would be overwritten...");
            // Trecem &mut database și pentru această funcție, deoarece calculează tree_diff
            Self::check_untracked_conflicts(workspace, index, &mut database, base_oid, target_oid)?;
            info!("Check for untracked files complete. No conflicts found.");

            // 2. Verifică conflictele cu modificări locale necomise
            info!("Checking for uncommitted changes that would be overwritten...");
            // Trecem &mut database și aici, Inspector poate accesa database
            Self::check_local_modifications_conflict(workspace, index, &mut database)?;
            info!("Check for uncommitted changes complete. No conflicts found.");
            // *** END: Verificări pre-merge ***


            // --- Logica de Merge (Fast-forward sau Recursiv) ---
            if inputs.already_merged() {
                println!("Already up to date.");
                index.rollback()?;
                return Ok(());
            }

            if inputs.is_fast_forward() {
                info!("Fast-forward possible.");
                return Self::handle_fast_forward(
                    database, // Acum este &mut Database
                    workspace,
                    index,
                    refs,
                    &inputs.left_oid,
                    target_oid
                );
            }

            // --- Merge Recursiv ---
             info!("Performing recursive merge.");
             // Resolve are nevoie de &mut database și &mut index
            let mut merge_resolver = Resolve::new(database, workspace, index, &inputs);
            merge_resolver.on_progress = |msg| info!("{}", msg);

             let merge_result = merge_resolver.execute();

             if let Err(e) = merge_result {
                  error!("Merge resolution failed: {}", e);
                  if e.to_string().contains("Automatic merge failed") || e.to_string().contains("fix conflicts") {
                       if let Err(write_err) = index.write_updates() {
                            error!("Failed to write index with conflicts: {}", write_err);
                            index.rollback()?;
                            return Err(e);
                       }
                       info!("Index with conflicts written successfully.");
                       return Err(e);
                  } else {
                       return Err(e);
                  }
             }

            // --- Merge reușit fără conflicte ---
            info!("Merge resolved without conflicts. Writing index...");
            if !index.write_updates()? {
                 warn!("Index write reported no changes after successful merge resolution.");
            } else {
                 info!("Index written successfully after merge resolution.");
            }


            // --- Commit pentru merge reușit ---
            info!("Creating merge commit...");
            let commit_message = message.map(|s| s.to_string()).unwrap_or_else(|| {
                format!("Merge branch '{}'", revision)
            });
             let author_name = env::var("GIT_AUTHOR_NAME").unwrap_or_else(|_| {
                 warn!("GIT_AUTHOR_NAME not set. Using default.");
                 "Default Author".to_string()
             });
             let author_email = env::var("GIT_AUTHOR_EMAIL").unwrap_or_else(|_| {
                  warn!("GIT_AUTHOR_EMAIL not set. Using default.");
                  "author@example.com".to_string()
             });
            let author = Author::new(author_name, author_email);
            
            // Create committer with current timestamp
            let committer_name = env::var("GIT_COMMITTER_NAME").unwrap_or_else(|_| {
                env::var("GIT_AUTHOR_NAME").unwrap_or_else(|_| {
                    env::var("USER").unwrap_or_else(|_| "Default Committer".to_string())
                })
            });
            let committer_email = env::var("GIT_COMMITTER_EMAIL").unwrap_or_else(|_| {
                env::var("GIT_AUTHOR_EMAIL").unwrap_or_else(|_| {
                    format!("{}@localhost", committer_name)
                })
            });
            let committer = Author::new(committer_name, committer_email);

            // Trecem &mut database și aici
            let tree_oid = Self::write_tree_from_index(database, index)?;
            info!("Merge commit tree OID: {}", tree_oid);

            let parent1 = head_oid.clone();

             let mut commit = Commit::new_with_committer(
                 Some(parent1), 
                 tree_oid.clone(), 
                 author, 
                 committer,
                 commit_message
             );

             database.store(&mut commit)?; // Trecem &mut database
             let commit_oid = commit.get_oid().cloned().ok_or(Error::Generic("Commit OID not set after storage".into()))?;
             info!("Created merge commit: {}", commit_oid);

             refs.update_head(&commit_oid)?;
             info!("Updated HEAD to merge commit: {}", commit_oid);

             let elapsed = start_time.elapsed();
             println!("Merge completed successfully in {:.2}s", elapsed.as_secs_f32());

            Ok(()) // Success for recursive merge

        })(); // End closure

        // --- Ensure rollback if closure returned error ---
         if result.is_err() {
              if let Err(ref e) = result {
                   let msg = e.to_string();
                   if msg.contains("fix conflicts") {
                        error!("Merge failed due to conflicts.");
                       return result;
                   }
                   if msg.contains("untracked working tree files would be overwritten") {
                        error!("Merge failed due to untracked files.");
                        index.rollback()?;
                        return result;
                   }
                   if msg.contains("Your local changes to the following files would be overwritten by merge") {
                        error!("Merge failed due to uncommitted changes.");
                        index.rollback()?;
                        return result;
                   }
              }
              error!("Merge command failed, rolling back index lock.");
              index.rollback()?;
         }

        result
    }

    // --- Verifică conflictele cu fișiere neurmărite (neschimbat) ---
    fn check_untracked_conflicts(
        workspace: &Workspace,
        index: &Index,
        database: &mut Database,
        base_oid: Option<&str>,
        target_oid: &str,
    ) -> Result<(), Error> {
        let path_filter = PathFilter::new();
        let diff = database.tree_diff(base_oid, Some(target_oid), &path_filter)?;
        debug!("Calculated diff for untracked check: {} changes.", diff.len());
        let mut conflicts = Vec::new();
        for (path, (_old_entry, new_entry_opt)) in diff {
            if let Some(new_entry) = new_entry_opt {
                if !new_entry.get_file_mode().is_directory() {
                    let path_str = path.to_string_lossy().to_string();
                    debug!("Checking path from diff: {}", path_str);
                    if workspace.path_exists(&path)? {
                        debug!("  Path exists in workspace.");
                        if !index.tracked(&path_str) {
                            debug!("  Path is untracked. Conflict detected!");
                            conflicts.push(path_str);
                        } else { debug!("  Path is tracked by index."); }
                    } else { debug!("  Path does not exist in workspace."); }
                }
            }
        }
        if !conflicts.is_empty() {
            conflicts.sort();
            let mut error_message = String::from("The following untracked working tree files would be overwritten by merge:\n");
            for path in conflicts { error_message.push_str(&format!("  {}\n", path)); }
            error_message.push_str("Please move or remove them before you merge.\n");
            error_message.push_str("Aborting");
            Err(Error::Generic(error_message))
        } else {
             debug!("No untracked file conflicts found.");
            Ok(())
        }
    }

    // --- Verifică conflictele cu modificări locale necomise (MODIFICAT) ---
    fn check_local_modifications_conflict(
        workspace: &Workspace,
        index: &Index,
        database: &mut Database,
    ) -> Result<(), Error> {
        // 1. Identifică fișierele modificate local (Index vs Workspace)
        let inspector = Inspector::new(workspace, index, database);
        let workspace_changes = inspector.analyze_workspace_changes()?;
    
        let locally_modified_paths: Vec<String> = workspace_changes
            .into_iter()
            .filter_map(|(path, change_type)| {
                // Ne interesează fișierele modificate sau șterse local față de index
                // *** CORECAT AICI: Folosește ChangeType::Deleted ***
                if change_type == ChangeType::Modified || change_type == ChangeType::Deleted {
                     debug!("Found uncommitted change: {:?} for path {}", change_type, path);
                    Some(path)
                } else {
                    None
                }
            })
            .collect();
    
        // 2. Dacă lista *nu* este goală, avem un conflict
        if !locally_modified_paths.is_empty() {
            let mut sorted_conflicts = locally_modified_paths;
            sorted_conflicts.sort();
            let mut error_message = String::from("Your local changes to the following files would be overwritten by merge:\n");
            for path in sorted_conflicts {
                error_message.push_str(&format!("  {}\n", path));
            }
            error_message.push_str("Please commit your changes or stash them before you merge.\n");
            error_message.push_str("Aborting");
            error!("Merge aborted due to uncommitted changes: {:?}", error_message); // Loghează eroarea detaliată
            Err(Error::Generic(error_message))
        } else {
            debug!("No conflicts found between local modifications and merge changes.");
            Ok(())
        }
    }
    // --- handle_fast_forward (neschimbat funcțional, doar ajustat tipul `index`) ---
    fn handle_fast_forward(
        database: &mut Database,
        workspace: &Workspace,
        index: &mut Index, // Tipul corect
        refs: &Refs,
        current_oid: &str,
        target_oid: &str,
    ) -> Result<(), Error> {
        let a_short = &current_oid[0..std::cmp::min(8, current_oid.len())];
        let b_short = &target_oid[0..std::cmp::min(8, target_oid.len())];
        info!("Updating {}..{}", a_short, b_short);
        info!("Fast-forward");
        let target_commit_obj = database.load(target_oid)?;
        let target_commit = match target_commit_obj.as_any().downcast_ref::<Commit>() {
            Some(c) => c,
            None => return Err(Error::Generic(format!("Target OID {} is not a commit", target_oid))),
        };
        let target_tree_oid = target_commit.get_tree();
        debug!("Target tree OID: {}", target_tree_oid);
        let current_commit_obj = database.load(current_oid)?;
        let current_commit = match current_commit_obj.as_any().downcast_ref::<Commit>() {
            Some(c) => c,
            None => return Err(Error::Generic(format!("Current HEAD OID {} is not a commit", current_oid))),
        };
        let current_tree_oid = current_commit.get_tree();
        debug!("Current tree OID: {}", current_tree_oid);
        let path_filter = PathFilter::new();
        debug!("Calculating tree diff between current ({}) and target ({})", current_tree_oid, target_tree_oid);
        let tree_diff = database.tree_diff(Some(current_tree_oid), Some(target_tree_oid), &path_filter)?;
        debug!("Tree diff calculated, {} changes found", tree_diff.len());
        let mut diff_applied = false;
        if tree_diff.is_empty() {
            info!("No tree changes detected between commits.");
            index.set_changed(false);
        } else {
            for (path, (old_entry, new_entry)) in &tree_diff {
                debug!("Applying change for: {}", path.display());
                match (old_entry, new_entry) {
                    (Some(_), Some(new)) => {
                        if new.get_file_mode().is_directory() {
                            debug!("  -> Modified Directory (ensuring exists)");
                            workspace.make_directory(&path)?;
                            let tree_obj = database.load(new.get_oid())?;
                            if let Some(tree) = tree_obj.as_any().downcast_ref::<Tree>() {
                                Self::process_tree_entries(tree, &path, database, workspace, index)?;
                            }
                        } else {
                            debug!("  -> Modified File");
                            Self::update_workspace_file(database, workspace, index, &path, new.get_oid(), &new.get_file_mode())?;
                        }
                    },
                    (None, Some(new)) => {
                        if new.get_file_mode().is_directory() {
                            debug!("  -> Added Directory");
                            workspace.make_directory(&path)?;
                            let tree_obj = database.load(new.get_oid())?;
                            if let Some(tree) = tree_obj.as_any().downcast_ref::<Tree>() {
                                Self::process_tree_entries(tree, &path, database, workspace, index)?;
                            }
                        } else {
                            debug!("  -> Added File");
                            Self::update_workspace_file(database, workspace, index, &path, new.get_oid(), &new.get_file_mode())?;
                        }
                    },
                    (Some(old), None) => {
                        debug!("  -> Deleted");
                        let path_str = path.to_string_lossy().to_string();
                        if old.get_file_mode().is_directory() {
                            debug!("  -> Removing directory: {}", path.display());
                            workspace.force_remove_directory(&path)?;
                        } else {
                            debug!("  -> Removing file: {}", path.display());
                            workspace.remove_file(&path)?;
                        }
                        index.remove(&path_str)?;
                    },
                    (None, None) => { warn!("  -> Diff entry with no old or new state for {}", path.display()); }
                }
            }
            diff_applied = true;
            index.set_changed(true);
        }
        info!("Attempting to write index updates...");
        match index.write_updates() {
            Ok(updated) => {
                if updated { info!("Index successfully written."); }
                else if !diff_applied { info!("Index write skipped: No changes were applied."); }
                else { warn!("Index write reported no changes, but diff was applied."); }
            },
            Err(e) => { error!("ERROR writing index updates: {}", e); return Err(e); }
        }
        info!("Attempting to update HEAD to {}", target_oid);
        match refs.update_head(target_oid) {
            Ok(_) => info!("Successfully updated HEAD"),
            Err(e) => { error!("ERROR updating HEAD: {}", e); return Err(e); }
        }
        info!("Fast-forward merge completed.");
        Ok(())
    }

    // --- process_tree_entries (neschimbat) ---
    fn process_tree_entries(
        tree: &Tree, parent_path: &Path, database: &mut Database, workspace: &Workspace, index: &mut Index
    ) -> Result<(), Error> {
        debug!("Processing directory contents recursively: {}", parent_path.display());
        let mut target_entries = HashMap::new();
        for (name, entry) in tree.get_entries() { target_entries.insert(name.clone(), entry.clone()); }
        let mut current_files = HashSet::new();
        let full_dir_path = workspace.root_path.join(parent_path);
        if full_dir_path.exists() && full_dir_path.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&full_dir_path) {
                for entry_result in entries {
                    if let Ok(entry) = entry_result {
                        let file_name = entry.file_name().to_string_lossy().to_string();
                        if !file_name.starts_with('.') && file_name != ".ash" { current_files.insert(file_name); }
                    }
                }
            }
        }
        debug!("  Found {} existing non-hidden items in workspace directory.", current_files.len());
        for (name, entry) in tree.get_entries() {
            let entry_path = parent_path.join(name);
            match entry {
                TreeEntry::Blob(oid, mode) => {
                    debug!("  -> Writing file in directory: {}", entry_path.display());
                    Self::update_workspace_file(database, workspace, index, &entry_path, &oid, &mode)?;
                    current_files.remove(name);
                },
                TreeEntry::Tree(subtree) => {
                    debug!("  -> Processing subdirectory: {}", entry_path.display());
                    workspace.make_directory(&entry_path)?;
                    if let Some(subtree_oid) = subtree.get_oid() {
                        let subtree_obj = database.load(subtree_oid)?;
                        if let Some(loaded_subtree) = subtree_obj.as_any().downcast_ref::<Tree>() {
                            Self::process_tree_entries(loaded_subtree, &entry_path, database, workspace, index)?;
                        } else { warn!("Object {} for subtree {} is not a Tree", subtree_oid, entry_path.display()); }
                    } else { warn!("Subtree entry {} has no OID", entry_path.display()); }
                    current_files.remove(name);
                }
            }
        }
        for old_name in current_files {
            let old_path = parent_path.join(&old_name);
            let path_str = old_path.to_string_lossy().to_string();
            debug!("  -> Removing file/dir not in target tree: {}", old_path.display());
            let full_path = workspace.root_path.join(&old_path);
            if full_path.is_dir() { workspace.force_remove_directory(&old_path)?; }
            else { workspace.remove_file(&old_path)?; }
            index.remove(&path_str)?;
        }
        Ok(())
    }

    // --- update_workspace_file (neschimbat) ---
    fn update_workspace_file(
        database: &mut Database, workspace: &Workspace, index: &mut Index, path: &PathBuf, oid: &str, _mode: &FileMode,
    ) -> Result<(), Error> {
        debug!("Updating workspace file '{}' with OID {}", path.display(), oid);
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                let parent_full_path = workspace.root_path.join(parent);
                if !parent_full_path.exists() {
                     debug!("Creating parent directory: {}", parent.display());
                    workspace.make_directory(parent)?;
                }
            }
        }
        let blob_obj = database.load(oid)?;
        let content = blob_obj.to_bytes();
        workspace.write_file(&path, &content)?;
        let stat = workspace.stat_file(&path)?;
        index.add(&path, oid, &stat)?;
        debug!("File '{}' updated in workspace and index.", path.display());
        Ok(())
    }

    // --- write_tree_from_index (neschimbat) ---
    fn write_tree_from_index(database: &mut Database, index: &Index) -> Result<String, Error> {
        let database_entries: Vec<_> = index.each_entry()
            .filter(|entry| entry.stage == 0)
            .map(|index_entry| {
                DatabaseEntry::new(
                    index_entry.get_path().to_string(),
                    index_entry.get_oid().to_string(),
                    &index_entry.mode_octal()
                )
            })
            .collect();
         debug!("Building tree from {} stage 0 index entries.", database_entries.len());
         if database_entries.is_empty() {
              info!("Index is empty, creating empty tree.");
              let mut empty_tree = Tree::new();
              database.store(&mut empty_tree)?;
              return empty_tree.get_oid().cloned().ok_or_else(|| Error::Generic("Failed to get OID for empty tree".into()));
         }
        let mut root = crate::core::database::tree::Tree::build(database_entries.iter())?;
        root.traverse(|tree| database.store(tree).map(|_| ()))?;
        let tree_oid = root.get_oid().ok_or(Error::Generic("Tree OID not set after storage".into()))?;
        info!("Successfully built and stored tree with OID: {}", tree_oid);
        Ok(tree_oid.clone())
    }
}