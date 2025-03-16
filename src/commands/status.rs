// src/commands/status.rs - Cu verificare între HEAD, Index și Workspace
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::core::database::database::{Database, GitObject};
use crate::core::database::entry::Entry as DatabaseEntry;
use crate::core::database::tree::{Tree, TreeEntry};
use crate::core::database::commit::Commit;
use crate::core::file_mode::FileMode;
use crate::core::index::entry::Entry;
use crate::core::index::index::Index;
use crate::core::refs::Refs;
use crate::core::workspace::Workspace;
use crate::errors::error::Error;
use crate::core::database::tree::TREE_MODE;

const REGULAR_MODE: u32 = 0o100644;
const EXECUTABLE_MODE: u32 = 0o100755;

// Enum pentru tipuri de modificări
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
enum ChangeType {
    WorkspaceModified,
    WorkspaceDeleted,
    IndexAdded,
    IndexModified,
    IndexDeleted,
}

pub struct StatusCommand;

impl StatusCommand {
    /// Verifică dacă metadatele fișierului corespund cu intrarea din index
    fn stat_match(entry: &Entry, stat: &fs::Metadata) -> bool {
        // Verifică dimensiunea fișierului
        let size_matches = entry.get_size() as u64 == stat.len();
        
        // Verifică modul fișierului
        let entry_mode = entry.get_mode();
        let file_mode = Self::mode_for_stat(stat);
        let mode_matches = FileMode::are_equivalent(entry_mode, file_mode);
        
        size_matches && mode_matches
    }
    
    /// Verifică dacă timestamp-urile fișierului corespund cu intrarea din index
    fn times_match(entry: &Entry, stat: &fs::Metadata) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            
            // Convertește în secunde și nanosecunde pentru comparație
            let stat_mtime_sec = stat.mtime() as u32;
            let stat_mtime_nsec = stat.mtime_nsec() as u32;

            println!("Comparare timestamps pentru {}", entry.path);
            println!("Index mtime: {}.{}", entry.get_mtime(), entry.get_mtime_nsec());
            println!("File mtime: {}.{}", stat_mtime_sec, stat_mtime_nsec);
            // SFÂRȘITUL CODULUI DE DEBUGGING
            
            // Compară timpii de modificare
            entry.get_mtime() == stat_mtime_sec && entry.get_mtime_nsec() == stat_mtime_nsec
        }
        
        #[cfg(not(unix))]
        {
            // Pe Windows, nu avem aceeași granularitate, așa că convertim în secunde
            if let Ok(mtime) = stat.modified() {
                if let Ok(duration) = mtime.duration_since(std::time::UNIX_EPOCH) {
                    let stat_mtime_sec = duration.as_secs() as u32;
                    return entry.get_mtime() == stat_mtime_sec;
                }
            }
            
            // Dacă nu putem obține timpul de modificare, presupunem că nu se potrivesc
            false
        }
    }
    
    /// Determină modul fișierului din metadata (executabil vs regular)
    fn mode_for_stat(stat: &fs::Metadata) -> u32 {
        FileMode::from_metadata(stat)
    }
    
    /// Verifică dacă un director conține fișiere care pot fi urmărite (recursiv)
    fn is_trackable_dir(dir_path: &Path) -> Result<bool, Error> {
        if !dir_path.is_dir() {
            return Ok(false);
        }
        
        // Verifică dacă directorul conține fișiere non-ascunse
        match std::fs::read_dir(dir_path) {
            Ok(entries) => {
                for entry_result in entries {
                    match entry_result {
                        Ok(entry) => {
                            let path = entry.path();
                            let file_name = entry.file_name();
                            
                            // Skip hidden files and directories
                            if let Some(name) = file_name.to_str() {
                                if name.starts_with('.') {
                                    continue;
                                }
                            }
                            
                            if path.is_file() {
                                // Am găsit un fișier care poate fi urmărit
                                return Ok(true);
                            } else if path.is_dir() {
                                // Verifică recursiv subdirectoarele
                                if Self::is_trackable_dir(&path)? {
                                    return Ok(true);
                                }
                            }
                        },
                        Err(e) => return Err(Error::IO(e)),
                    }
                }
                
                // Nu s-au găsit fișiere care pot fi urmărite
                Ok(false)
            },
            Err(e) => Err(Error::IO(e)),
        }
    }
    
    /// Returnează statusul pentru un anumit path bazat pe tipurile de modificări
    fn status_for(path: &str, changes: &HashMap<String, HashSet<ChangeType>>) -> String {
        let mut left = " ";
        let mut right = " ";
        
        if let Some(change_set) = changes.get(path) {
            // Status pentru prima coloană (HEAD -> Index)
            if change_set.contains(&ChangeType::IndexAdded) {
                left = "A";
            } else if change_set.contains(&ChangeType::IndexModified) {
                left = "M";
            } else if change_set.contains(&ChangeType::IndexDeleted) {
                left = "D";
            }
            
            // Status pentru a doua coloană (Index -> Workspace)
            if change_set.contains(&ChangeType::WorkspaceDeleted) {
                right = "D";
            } else if change_set.contains(&ChangeType::WorkspaceModified) {
                right = "M";
            }
        }
        
        format!("{}{}", left, right)
    }
    
    /// Înregistrează o modificare pentru un anumit path
    fn record_change(
        changed: &mut HashSet<String>,
        changes: &mut HashMap<String, HashSet<ChangeType>>,
        path: String,
        change_type: ChangeType
    ) {
        changed.insert(path.clone());
        changes.entry(path)
              .or_insert_with(HashSet::new)
              .insert(change_type);
    }
    
    /// Încarcă tree-ul din HEAD commit
    fn load_head_tree(
        refs: &Refs, 
        database: &mut Database
    ) -> Result<HashMap<String, DatabaseEntry>, Error> {
        let mut head_tree = HashMap::new();
        
        // ADAUGĂ ACEST COD DE DEBUGGING
        println!("Încărcare HEAD tree");
        // SFÂRȘITUL CODULUI DE DEBUGGING
        
        // Citește HEAD
        if let Some(head_oid) = refs.read_head()? {
            // ADAUGĂ ACEST COD DE DEBUGGING
            println!("HEAD OID: {}", head_oid);
            // SFÂRȘITUL CODULUI DE DEBUGGING
            
            // Încarcă commit-ul din HEAD
            let commit_obj = database.load(&head_oid)?;
            let commit = commit_obj.as_any().downcast_ref::<Commit>().unwrap();
            
            // ADAUGĂ ACEST COD DE DEBUGGING
            println!("Commit tree OID: {}", commit.get_tree());
            // SFÂRȘITUL CODULUI DE DEBUGGING
            
            // Citește tree-ul recursiv
            Self::read_tree(database, commit.get_tree(), Path::new(""), &mut head_tree)?;
        } else {
            // ADAUGĂ ACEST COD DE DEBUGGING
            println!("Nu s-a găsit HEAD, tree gol");
            // SFÂRȘITUL CODULUI DE DEBUGGING
        }
        
        // ADAUGĂ ACEST COD DE DEBUGGING
        println!("Entries în HEAD tree: {}", head_tree.len());
        for (path, entry) in &head_tree {
            println!("  {} -> {}", path, entry.get_oid());
        }
        // SFÂRȘITUL CODULUI DE DEBUGGING
        
        Ok(head_tree)
    }
    
    /// Citește recursiv un tree și adaugă intrările la head_tree
    fn read_tree(
        database: &mut Database,
        tree_oid: &str,
        prefix: &Path,
        head_tree: &mut HashMap<String, DatabaseEntry>
    ) -> Result<(), Error> {
        // Încarcă tree-ul din baza de date
        let tree_obj = database.load(tree_oid)?;
        let tree = tree_obj.as_any().downcast_ref::<Tree>().unwrap();
        
        // Procesează toate intrările
        for (name, entry) in tree.get_entries() {
            let path = if prefix.as_os_str().is_empty() {
                PathBuf::from(name)
            } else {
                prefix.join(name)
            };
            
            match entry {
                TreeEntry::Tree(subtree) => {
                    if let Some(oid) = subtree.get_oid() {
                        // În loc să doar adăugăm recursiv, adăugăm și o intrare pentru directorul însuși
                        let dir_path = path.to_string_lossy().to_string();
                        let db_entry = DatabaseEntry::new(
                            dir_path.clone(), // Aici folosim clone pentru a evita eroarea
                            oid.clone(),
                            &TREE_MODE.to_string(), // Folosește modul pentru directoare
                        );
                        head_tree.insert(dir_path, db_entry);
                        
                        // Acum procesează recursiv
                        Self::read_tree(database, oid, &path, head_tree)?;
                    }
                },
                TreeEntry::Blob(oid, mode) => {
                    // Codul existent pentru blob-uri
                    let db_entry = DatabaseEntry::new(
                        path.to_string_lossy().to_string(),
                        oid.clone(),
                        &mode.to_string(),
                    );
                    head_tree.insert(path.to_string_lossy().to_string(), db_entry);
                }
            }
        }
        
        Ok(())
    }
    
    /// Verifică index-ul în raport cu HEAD tree
    // În funcția check_index_against_head_tree sau echivalent
    // În funcția check_index_against_head_tree sau echivalent
    fn check_index_against_head_tree(
        index_entry: &Entry,
        head_tree: &HashMap<String, DatabaseEntry>,
        changed: &mut HashSet<String>,
        changes: &mut HashMap<String, HashSet<ChangeType>>
    ) {
        let path = index_entry.get_path();
        
        println!("Comparare index cu HEAD pentru {}", path);
        println!("Index OID: {}", index_entry.get_oid());
        
        // Verifică dacă HEAD tree este gol - cazul pentru primul commit
        if head_tree.is_empty() {
            // Când nu există HEAD, toate fișierele din index sunt noi
            println!("HEAD tree gol, fișier marcat ca nou: {}", path);
            Self::record_change(changed, changes, path.to_string(), ChangeType::IndexAdded);
            return;
        }
        
        if let Some(head_entry) = head_tree.get(path) {
            println!("HEAD OID: {}", head_entry.get_oid());
            
            // Comparăm OID-urile
            let oids_match = index_entry.get_oid() == head_entry.get_oid();
            println!("OIDs egale: {}", oids_match);
            
            // Convertim modurile și le comparăm
            let index_mode = index_entry.get_mode();
            
            // Curăță și parsează modul din head_entry
            let head_mode_str = head_entry.get_mode().trim();
            let head_mode = if head_mode_str.starts_with("0") {
                u32::from_str_radix(&head_mode_str[1..], 8).unwrap_or(0)
            } else {
                u32::from_str_radix(head_mode_str, 8).unwrap_or(0)
            };
            
            println!("Index mode: {} (decimal)", index_mode);
            println!("HEAD mode: {} (octal) -> {} (decimal)", head_entry.get_mode(), head_mode);
            
            // Decidem dacă modurile sunt compatibile (ignorăm diferențele specifice)
            let modes_compatible = (index_mode & 0o777) == (head_mode & 0o777);
            println!("Moduri compatibile: {}", modes_compatible);
            
            // Comparăm doar OID-urile, ignorăm modurile pentru acum
            if !oids_match {
                println!("Hash-uri diferite, fișier marcat ca modificat");
                Self::record_change(changed, changes, path.to_string(), ChangeType::IndexModified);
            } else {
                println!("Hash-uri egale, fișierul nu este modificat");
            }
        } else {
            // Fișierul nu există în HEAD, a fost adăugat în index
            println!("Fișier marcat ca adăugat: {} (nu există în HEAD)", path);
            Self::record_change(changed, changes, path.to_string(), ChangeType::IndexAdded);
        }
    }
    /// Verifică HEAD tree în raport cu index
    fn check_head_tree_against_index(
        head_tree: &HashMap<String, DatabaseEntry>,
        index: &Index,
        changed: &mut HashSet<String>,
        changes: &mut HashMap<String, HashSet<ChangeType>>
    ) {
        for (path, head_entry) in head_tree {
            // Verifică dacă fișierul există în index
            if !index.tracked(path) {
                // Fișierul a fost în HEAD dar nu mai este în index
                Self::record_change(changed, changes, path.clone(), ChangeType::IndexDeleted);
            }
        }
    }
    
    pub fn execute(porcelain: bool) -> Result<(), Error> {
        let start_time = Instant::now();
        
        let root_path = Path::new(".");
        let git_path = root_path.join(".ash");
        
        // Verifică dacă directorul .ash există
        if !git_path.exists() {
            return Err(Error::Generic("Not an ash repository (or any of the parent directories): .ash directory not found".into()));
        }
        
        let workspace = Workspace::new(root_path);
        let mut database = Database::new(git_path.join("objects"));
        let mut index = Index::new(git_path.join("index"));
        let refs = Refs::new(&git_path);
        
        // Încarcă indexul pentru actualizare (deoarece am putea actualiza timestamp-urile)
        if !index.load_for_update()? {
            return Err(Error::Generic("Failed to acquire lock on index file".to_string()));
        }
        
        // Încarcă HEAD tree
        let head_tree = Self::load_head_tree(&refs, &mut database)?;
        
        // Obține fișierele urmărite din index
        let index_entries: HashMap<String, String> = index
            .each_entry()
            .map(|entry| (entry.get_path().to_string(), entry.get_oid().to_string()))
            .collect();
        
        // Sortează fișierele în categorii
        let mut untracked = HashSet::new();    // Fișiere în workspace dar nu în index
        let mut changed = HashSet::new();      // Fișiere cu orice tip de modificare
        let mut changes = HashMap::new();      // Map de path -> set de tipuri de modificări
        let mut stats_cache = HashMap::new();  // Cache pentru metadata fișierelor
        
        // Verifică pentru fișiere neurmărite prin scanarea workspace-ului
        let mut tracked_dirs = HashSet::new();
        
        // Colectează toate directoarele părinte ale fișierelor urmărite
        for path in index_entries.keys() {
            let path_buf = PathBuf::from(path);
            let mut current = path_buf.clone();
            
            while let Some(parent) = current.parent() {
                if parent.as_os_str().is_empty() {
                    break;
                }
                tracked_dirs.insert(parent.to_path_buf());
                current = parent.to_path_buf();
            }
        }
        
        // Procesează fișierele din workspace
        Self::scan_workspace(
            &workspace, 
            &mut untracked, 
            &index_entries, 
            &tracked_dirs,
            root_path,
            &PathBuf::new(),
            &mut stats_cache
        )?;
        
        // Verifică relațiile between HEAD, index și workspace
        
        // 1. Verifică fișiere din index față de HEAD
        for entry in index.each_entry() {
            Self::check_index_against_head_tree(
                entry, 
                &head_tree, 
                &mut changed, 
                &mut changes
            );
        }
        
        // 2. Verifică fișiere din HEAD față de index pentru a găsi fișiere șterse
        Self::check_head_tree_against_index(
            &head_tree,
            &index,
            &mut changed,
            &mut changes
        );
        
        // 3. Verifică fișiere din index față de workspace
        for (path, oid) in &index_entries {
            let path_buf = PathBuf::from(path);
            
            // Verifică dacă fișierul există
            if !workspace.path_exists(&path_buf)? {
                // Fișierul este în index dar nu în workspace (a fost șters)
                Self::record_change(&mut changed, &mut changes, path.clone(), ChangeType::WorkspaceDeleted);
                continue;
            }
            
            // Sari dacă a fost deja marcat ca neurmărit (nu ar trebui să se întâmple)
            if untracked.contains(path) {
                continue;
            }
            
            // Verifică dacă fișierul este modificat folosind metadata din cache
            if let Some(metadata) = stats_cache.get(path) {
                // Obține intrarea din index pentru comparație
                let index_entry = index.get_entry(path).unwrap();
                
                // Mai întâi verifică rapid metadatele fișierului (dimensiune și mod)
                if !Self::stat_match(index_entry, &metadata) {
                    Self::record_change(&mut changed, &mut changes, path.clone(), ChangeType::WorkspaceModified);
                    continue;
                }
                
                // Optimizare: Verifică timestamp-urile - dacă se potrivesc, presupune că conținutul nu s-a schimbat
                if Self::times_match(index_entry, &metadata) {
                    // Timestamp-urile se potrivesc, presupune că fișierul nu s-a schimbat
                    continue;
                }
                
                // Dacă timestamp-urile nu se potrivesc, trebuie să verificăm hash-ul conținutului
                match workspace.read_file(&path_buf) {
                    Ok(data) => {
                        // Folosește baza de date pentru a calcula hash-ul eficient
                        let computed_oid = database.hash_file_data(&data);
                        println!("Verificare fișier: {}", path);
                        println!("Hash în index: {}", oid);
                        println!("Hash calculat: {}", computed_oid);
                        if let Some(metadata) = stats_cache.get(path) {
                            #[cfg(unix)]
                            {
                                use std::os::unix::fs::MetadataExt;
                                let file_mtime = metadata.mtime() as u32;
                                let file_mtime_nsec = metadata.mtime_nsec() as u32;
                                println!("Timestamp fișier: {}.{}", file_mtime, file_mtime_nsec);
                            }
                        }
                        
                        // Obține intrarea din index pentru a afișa metadata
                        if let Some(index_entry) = index.get_entry(path) {
                            println!("Timestamp index: {}.{}", index_entry.get_mtime(), index_entry.get_mtime_nsec());
                            println!("Mod index: {}, Mod fișier: {}", index_entry.get_mode(), StatusCommand::mode_for_stat(&metadata));
                            println!("Size index: {}, Size fișier: {}", index_entry.get_size(), metadata.len());
                        }
                        if &computed_oid != oid {
                            // Fișierul s-a schimbat, marchează-l ca modificat
                            Self::record_change(&mut changed, &mut changes, path.clone(), ChangeType::WorkspaceModified);
                        } else {
                            // Fișierul nu s-a schimbat de fapt, doar timestamp-urile
                            // Actualizează intrarea din index cu noile timestamp-uri pentru a evita recitirea data viitoare
                            index.update_entry_stat(path, &metadata)?;
                        }
                    },
                    Err(_) => {
                        // Dacă nu putem citi fișierul din orice motiv, îl considerăm modificat
                        Self::record_change(&mut changed, &mut changes, path.clone(), ChangeType::WorkspaceModified);
                    }
                }
            } else {
                // Nu există metadata în cache pentru un fișier indexat, presupunem că a fost șters
                Self::record_change(&mut changed, &mut changes, path.clone(), ChangeType::WorkspaceDeleted);
            }
        }
        
        // Scrie eventualele actualizări de timestamp în index
        if index.is_changed() {
            index.write_updates()?;
        } else {
            // Nu sunt modificări la index, eliberează lock-ul
            index.rollback()?;
        }
        
        // Afișează rezultatele
        if porcelain {
            // Ieșire pentru mașină (opțiunea --porcelain)
            Self::print_porcelain(&untracked, &changed, &changes);
        } else {
            // Ieșire pentru oameni
            Self::print_human_readable(&untracked, &changed, &changes);
        }
        
        let elapsed = start_time.elapsed();
        if !porcelain {
            println!("\nStatus completed in {:.2}s", elapsed.as_secs_f32());
        }
        
        Ok(())
    }
    
    fn scan_workspace(
        workspace: &Workspace,
        untracked: &mut HashSet<String>,
        index_entries: &HashMap<String, String>,
        tracked_dirs: &HashSet<PathBuf>,
        root_path: &Path,
        prefix: &Path,
        stats_cache: &mut HashMap<String, fs::Metadata>,
    ) -> Result<(), Error> {
        let current_path = if prefix.as_os_str().is_empty() {
            root_path.to_path_buf()
        } else {
            root_path.join(prefix)
        };
        
        // Listează fișierele din directorul curent
        match std::fs::read_dir(&current_path) {
            Ok(entries) => {
                for entry_result in entries {
                    match entry_result {
                        Ok(entry) => {
                            let file_name = entry.file_name();
                            let entry_path = entry.path();
                            
                            // Sari peste directorul .ash
                            if file_name == ".ash" {
                                continue;
                            }
                            
                            // Obține calea relativă față de root
                            let rel_path = if prefix.as_os_str().is_empty() {
                                PathBuf::from(file_name)
                            } else {
                                prefix.join(file_name)
                            };
                            
                            let rel_path_str = rel_path.to_string_lossy().to_string();
                            
                            // Verifică dacă calea este urmărită în index
                            let is_tracked = index_entries.contains_key(&rel_path_str);
                            let is_in_tracked_dir = tracked_dirs.contains(&rel_path);
                            
                            if entry_path.is_dir() {
                                if is_tracked || is_in_tracked_dir {
                                    // Dacă directorul este urmărit sau conține fișiere urmărite, 
                                    // scanează-l recursiv
                                    Self::scan_workspace(
                                        workspace, 
                                        untracked, 
                                        index_entries, 
                                        tracked_dirs,
                                        root_path,
                                        &rel_path,
                                        stats_cache
                                    )?;
                                } else if Self::is_trackable_dir(&entry_path)? {
                                    // Dacă directorul conține fișiere urmăribile, marchează-l
                                    untracked.insert(format!("{}/", rel_path_str));
                                }
                                // Dacă directorul este gol sau conține doar fișiere ignorate, îl sărim
                            } else if !is_tracked {
                                // Fișierul nu este urmărit în index
                                untracked.insert(rel_path_str);
                            } else {
                                // Fișierul este urmărit - pune metadata în cache pentru comparații ulterioare
                                if let Ok(metadata) = entry_path.metadata() {
                                    stats_cache.insert(rel_path_str, metadata);
                                }
                            }
                        },
                        Err(e) => return Err(Error::IO(e)),
                    }
                }
            },
            Err(e) => return Err(Error::IO(e)),
        }
        
        Ok(())
    }
    
    fn print_porcelain(
        untracked: &HashSet<String>,
        changed: &HashSet<String>,
        changes: &HashMap<String, HashSet<ChangeType>>,
    ) {
        // Colectează toate fișierele pentru a le sorta
        let mut all_files: Vec<String> = Vec::new();
        
        // Adaugă fișierele modificate
        for path in changed {
            all_files.push(path.clone());
        }
        
        // Adaugă fișierele neurmărite
        for path in untracked {
            all_files.push(path.clone());
        }
        
        // Sortează toate fișierele
        all_files.sort();
        
        // Afișează status pentru fiecare fișier
        for path in &all_files {
            if untracked.contains(path) {
                println!("?? {}", path);
            } else {
                let status = Self::status_for(path, changes);
                println!("{} {}", status, path);
            }
        }
    }
    
    fn print_human_readable(
        untracked: &HashSet<String>,
        changed: &HashSet<String>,
        changes: &HashMap<String, HashSet<ChangeType>>,
    ) {
        // Grupăm modificările după tip
        let mut changes_to_be_committed = Vec::new();
        let mut changes_not_staged = Vec::new();
        
        for path in changed {
            if let Some(change_set) = changes.get(path) {
                // Modificări între HEAD și index
                if change_set.contains(&ChangeType::IndexAdded) {
                    changes_to_be_committed.push((path, "new file"));
                } else if change_set.contains(&ChangeType::IndexModified) {
                    changes_to_be_committed.push((path, "modified"));
                } else if change_set.contains(&ChangeType::IndexDeleted) {
                    changes_to_be_committed.push((path, "deleted"));
                }
                
                // Modificări între index și workspace
                if change_set.contains(&ChangeType::WorkspaceModified) {
                    changes_not_staged.push((path, "modified"));
                } else if change_set.contains(&ChangeType::WorkspaceDeleted) {
                    changes_not_staged.push((path, "deleted"));
                }
            }
        }
        
        println!("On branch master");
        
        // Afișează modificările din index (HEAD -> Index)
        if !changes_to_be_committed.is_empty() {
            println!("\nChanges to be committed:");
            println!("  (use \"ash reset HEAD <file>...\" to unstage)");
            
            // Sortează pentru ieșire consistentă
            changes_to_be_committed.sort();
            
            for (path, status) in &changes_to_be_committed {
                println!("        {}: {}", status, path);
            }
        }
        
        // Afișează modificările din workspace (Index -> Workspace)
        if !changes_not_staged.is_empty() {
            println!("\nChanges not staged for commit:");
            println!("  (use \"ash add <file>...\" to update what will be committed)");
            println!("  (use \"ash checkout -- <file>...\" to discard changes in working directory)");
            
            // Sortează pentru ieșire consistentă
            changes_not_staged.sort();
            
            for (path, status) in &changes_not_staged {
                println!("        {}: {}", status, path);
            }
        }
        
        // Afișează fișierele neurmărite
        if !untracked.is_empty() {
            println!("\nUntracked files:");
            println!("  (use \"ash add <file>...\" to include in what will be committed)");
            
            let mut sorted_untracked: Vec<&String> = untracked.iter().collect();
            sorted_untracked.sort();
            
            for path in sorted_untracked {
                println!("        {}", path);
            }
        }
        
        // Dacă nu sunt modificări, arată mesajul "working tree clean"
        if changes_to_be_committed.is_empty() && changes_not_staged.is_empty() && untracked.is_empty() {
            println!("nothing to commit, working tree clean");
        }
    }
}