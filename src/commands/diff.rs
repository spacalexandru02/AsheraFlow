    // src/commands/diff.rs - versiune îmbunătățită
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
use crate::core::diff::myers::{diff_lines, format_diff, is_binary_content};
use crate::errors::error::Error;

pub struct DiffCommand;


impl DiffCommand {
/// Execute diff command between index/HEAD and working tree
    pub fn execute(paths: &[String], cached: bool) -> Result<(), Error> {
        let start_time = Instant::now();
        
        let root_path = Path::new(".");
        let git_path = root_path.join(".ash");
        
        // Verifică dacă directorul .ash există
        if !git_path.exists() {
            return Err(Error::Generic("fatal: not an ash repository (or any of the parent directories): .ash directory not found".into()));
        }
        
        let workspace = Workspace::new(root_path);
        let mut database = Database::new(git_path.join("objects"));
        let mut index = Index::new(git_path.join("index"));
        let refs = Refs::new(&git_path);
        
        // Încarcă indexul
        index.load()?;
        
        // Determină ce să compari în funcție de căi și flag-ul --cached
        if paths.is_empty() {
            // Tratează diff-ul pentru întregul repository
            Self::diff_all(&workspace, &mut database, &index, &refs, cached)?;
        } else {
            // Tratează căi specifice
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
        // Dacă flag-ul cached este setat, compară indexul cu HEAD
        if cached {
            return Self::diff_index_vs_head(workspace, database, index, refs);
        }
        
        // În caz contrar, compară arborele de lucru cu indexul
        let mut has_changes = false;
        
        // Obține toate fișierele din index
        for entry in index.each_entry() {
            let path = Path::new(entry.get_path());
            
            // Sări dacă fișierul nu există în workspace
            if !workspace.path_exists(path)? {
                has_changes = true;
                let path_str = path.display().to_string();
                println!("diff --ash a/{} b/{}", Color::cyan(&path_str), Color::cyan(&path_str));
                println!("{} {}", Color::red("deleted file mode"), Color::red(&entry.mode_octal()));
                println!("--- a/{}", Color::red(&path_str));
                println!("+++ {}", Color::red("/dev/null"));
                
                // Obține conținutul blob-ului din baza de date
                let blob_obj = database.load(entry.get_oid())?;
                let content = blob_obj.to_bytes();
                
                // Verifică dacă conținutul este binar
                if is_binary_content(&content) {
                    println!("Binary file a/{} has been deleted", path_str);
                    continue;
                }
                
                let lines = diff::split_lines(&String::from_utf8_lossy(&content));
                
                // Arată diff-ul de ștergere
                for line in &lines {
                    println!("{}", Color::red(&format!("-{}", line)));
                }
                
                continue;
            }
            
            // Citește conținutul fișierului
            let file_content = workspace.read_file(path)?;
            
            // Calculează hash-ul pentru conținutul fișierului
            let file_hash = database.hash_file_data(&file_content);
            
            // Dacă hash-ul se potrivește, nu există nicio modificare
            if file_hash == entry.get_oid() {
                continue;
            }
            
            has_changes = true;
            
            // Tipărește antetul diff-ului
            let path_str = path.display().to_string();
            println!("diff --ash a/{} b/{}", Color::cyan(&path_str), Color::cyan(&path_str));
            
            // Verifică dacă fișierul este binar
            if is_binary_content(&file_content) {
                println!("Binary files a/{} and b/{} differ", path_str, path_str);
                continue;
            }
            
            // Obține diff-ul între index și copia de lucru
            let raw_diff_output = diff::diff_with_database(workspace, database, path, entry.get_oid(), 3)?;
            
            // Adaugă culori la ieșirea diff-ului
            let colored_diff = Self::colorize_diff_output(&raw_diff_output);
            print!("{}", colored_diff);
        }
        
        if !has_changes {
            println!("{}", Color::green("No changes"));
        }
        
        Ok(())
    }

    /// Metodă helper pentru colorarea ieșirii diff-ului
    fn colorize_diff_output(diff: &str) -> String {
        let mut result = String::new();
        
        for line in diff.lines() {
            if line.starts_with("Binary files") {
                // Mesaje despre fișiere binare
                result.push_str(&Color::yellow(line));
                result.push('\n');
            } else if line.starts_with("@@") && line.contains("@@") {
                // Antet de hunk
                result.push_str(&Color::cyan(line));
                result.push('\n');
            } else if line.starts_with('+') {
                // Linie adăugată
                result.push_str(&Color::green(line));
                result.push('\n');
            } else if line.starts_with('-') {
                // Linie eliminată
                result.push_str(&Color::red(line));
                result.push('\n');
            } else {
                // Linie de context
                result.push_str(line);
                result.push('\n');
            }
        }
        
        result
    }

    /// Colectează toate fișierele dintr-un commit
    fn collect_files_from_commit(
        database: &mut Database,
        commit: &Commit,
        files: &mut HashMap<String, String>
    ) -> Result<(), Error> {
        // Obține OID-ul arborelui din commit
        let tree_oid = commit.get_tree();
        
        // Colectează fișierele din arbore
        Self::collect_files_from_tree(database, tree_oid, PathBuf::new(), files)?;
        
        Ok(())
    }

    // Implementare îmbunătățită pentru a trata recursiv traversarea arborilor
    fn collect_files_from_tree(
        database: &mut Database,
        tree_oid: &str,
        prefix: PathBuf,
        files: &mut HashMap<String, String>
    ) -> Result<(), Error> {
        // Încarcă obiectul
        let obj = match database.load(tree_oid) {
            Ok(obj) => obj,
            Err(e) => {
                println!("Warning: Could not load object {}: {}", tree_oid, e);
                return Ok(());
            }
        };
        
        // Verifică dacă obiectul este un arbore
        if let Some(tree) = obj.as_any().downcast_ref::<Tree>() {
            // Procesează fiecare intrare din arbore
            for (name, entry) in tree.get_entries() {
                let entry_path = if prefix.as_os_str().is_empty() {
                    PathBuf::from(name)
                } else {
                    prefix.join(name)
                };
                
                let entry_path_str = entry_path.to_string_lossy().to_string();
                
                match entry {
                    TreeEntry::Blob(oid, mode) => {
                        // Dacă aceasta este o intrare de director deghizată ca blob
                        if *mode == TREE_MODE || mode.is_directory() {
                            // Procesează recursiv acest director
                            if let Err(e) = Self::collect_files_from_tree(database, oid, entry_path, files) {
                                println!("Warning: Error traversing directory '{}': {}", entry_path_str, e);
                            }
                        } else {
                            // Fișier normal
                            files.insert(entry_path_str, oid.clone());
                        }
                    },
                    TreeEntry::Tree(subtree) => {
                        if let Some(subtree_oid) = subtree.get_oid() {
                            // Procesează recursiv acest director
                            if let Err(e) = Self::collect_files_from_tree(database, subtree_oid, entry_path, files) {
                                println!("Warning: Error traversing subtree '{}': {}", entry_path_str, e);
                            }
                        }
                    }
                }
            }
            
            return Ok(());
        }
        
        // Dacă obiectul este un blob, încearcă să-l parsezi ca arbore
        if obj.get_type() == "blob" {
            // Încearcă să parsezi blob-ul ca arbore (aceasta tratează directoare stocate ca blob-uri)
            let blob_data = obj.to_bytes();
            if let Ok(parsed_tree) = Tree::parse(&blob_data) {
                // Procesează fiecare intrare din arborele parsat
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
                                // Procesează recursiv acest director
                                if let Err(e) = Self::collect_files_from_tree(database, oid, entry_path, files) {
                                    println!("Warning: Error traversing directory '{}': {}", entry_path_str, e);
                                }
                            } else {
                                // Fișier normal
                                files.insert(entry_path_str, oid.clone());
                            }
                        },
                        TreeEntry::Tree(subtree) => {
                            if let Some(subtree_oid) = subtree.get_oid() {
                                // Procesează recursiv acest director
                                if let Err(e) = Self::collect_files_from_tree(database, subtree_oid, entry_path, files) {
                                    println!("Warning: Error traversing subtree '{}': {}", entry_path_str, e);
                                }
                            }
                        }
                    }
                }
                
                return Ok(());
            } else {
                // Dacă suntem la o cale non-root, acesta ar putea fi un fișier
                if !prefix.as_os_str().is_empty() {
                    let path_str = prefix.to_string_lossy().to_string();
                    files.insert(path_str, tree_oid.to_string());
                    return Ok(());
                }
            }
        }
        
        // Caz special pentru intrări de top-level care ar putea necesita traversare mai profundă
        if prefix.as_os_str().is_empty() {
            // Verifică toate intrările găsite în root
            for (path, oid) in files.clone() {  // Clonăm pentru a evita probleme de împrumut
                // Doar căutăm intrări de director de top-level (fără separatori de cale)
                if !path.contains('/') {
                    // Încearcă să încarci și să traversezi ca director
                    let dir_path = PathBuf::from(&path);
                    if let Err(e) = Self::collect_files_from_tree(database, &oid, dir_path, files) {
                        println!("Warning: Error traversing entry '{}': {}", path, e);
                        // Continuă cu alte intrări chiar dacă aceasta eșuează
                    }
                }
            }
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
        
        // Dacă calea este în index
        if let Some(entry) = index.get_entry(&path_str) {
            if cached {
                // Compară indexul cu HEAD
                let head_oid = match refs.read_head()? {
                    Some(oid) => oid,
                    None => {
                        // Fără HEAD, arată ca fișier nou
                        let index_obj = database.load(entry.get_oid())?;
                        let content = index_obj.to_bytes();
                        
                        // Verifică dacă fișierul este binar
                        if is_binary_content(&content) {
                            println!("Binary file b/{} created", path_str);
                            return Ok(());
                        }
                        
                        // Generează un hash fictiv pentru formatul git
                        let index_hash = entry.get_oid();
                        let index_hash_short = if index_hash.len() >= 7 { &index_hash[0..7] } else { index_hash };
                        
                        println!("index 0000000..{} 100644", index_hash_short);
                        println!("--- /dev/null");
                        println!("+++ b/{}", path_str);
                        println!("@@ -0,0 +1,{} @@", content.len());
                        
                        let lines = diff::split_lines(&String::from_utf8_lossy(&content));
                        
                        for line in &lines {
                            println!("{}", Color::green(&format!("+{}", line)));
                        }
                        
                        return Ok(());
                    }
                };
                
                // Obține fișierul din commit-ul HEAD
                let commit_obj = database.load(&head_oid)?;
                let commit = match commit_obj.as_any().downcast_ref::<Commit>() {
                    Some(c) => c,
                    None => return Err(Error::Generic("HEAD is not a commit".into())),
                };
                
                let mut head_files: HashMap<String, String> = HashMap::new();
                DiffCommand::collect_files_from_commit(database, commit, &mut head_files)?;
                
                if let Some(head_oid) = head_files.get(&path_str) {
                    // Fișierul există atât în HEAD, cât și în index
                    if head_oid == entry.get_oid() {
                        println!("{}", Color::green(&format!("No changes staged for {}", path_str)));
                        return Ok(());
                    }
                    
                    // Compară versiunile din HEAD și index
                    // Încarcă ambele versiuni
                    let head_obj = database.load(head_oid)?;
                    let index_obj = database.load(entry.get_oid())?;
                    
                    let head_content = head_obj.to_bytes();
                    let index_content = index_obj.to_bytes();
                    
                    // Verifică dacă vreunul dintre fișiere este binar
                    if is_binary_content(&head_content) || is_binary_content(&index_content) {
                        println!("Binary files a/{} and b/{} differ", path_str, path_str);
                        return Ok(());
                    }
                    
                    // Generează hash-uri scurte pentru formatul git
                    let head_hash_short = if head_oid.len() >= 7 { &head_oid[0..7] } else { head_oid };
                    let index_hash_short = if entry.get_oid().len() >= 7 { &entry.get_oid()[0..7] } else { entry.get_oid() };
                    
                    println!("index {}..{} {}", head_hash_short, index_hash_short, entry.mode_octal());
                    println!("--- a/{}", path_str);
                    println!("+++ b/{}", path_str);
                    
                    let head_lines = diff::split_lines(&String::from_utf8_lossy(&head_content));
                    let index_lines = diff::split_lines(&String::from_utf8_lossy(&index_content));
                    
                    // Calculează diff-ul
                    let edits = diff_lines(&head_lines, &index_lines);
                    let diff_text = format_diff(&head_lines, &index_lines, &edits, 3);
                    
                    // Afișează diff-ul colorat
                    print!("{}", DiffCommand::colorize_diff_output(&diff_text));
                } else {
                    // Fișierul este în index, dar nu în HEAD (fișier nou)
                    let index_obj = database.load(entry.get_oid())?;
                    let content = index_obj.to_bytes();
                    
                    // Verifică dacă fișierul este binar
                    if is_binary_content(&content) {
                        println!("Binary file b/{} created", path_str);
                        return Ok(());
                    }
                    
                    // Generează un hash fictiv pentru formatul git
                    let index_hash = entry.get_oid();
                    let index_hash_short = if index_hash.len() >= 7 { &index_hash[0..7] } else { index_hash };
                    
                    println!("index 0000000..{} {}", index_hash_short, entry.mode_octal());
                    println!("--- /dev/null");
                    println!("+++ b/{}", path_str);
                    println!("@@ -0,0 +1,{} @@", content.len());
                    
                    let lines = diff::split_lines(&String::from_utf8_lossy(&content));
                    
                    for line in &lines {
                        println!("{}", Color::green(&format!("+{}", line)));
                    }
                }
            } else {
                // Compară indexul cu arborele de lucru
                if !workspace.path_exists(path)? {
                    let index_obj = database.load(entry.get_oid())?;
                    let content = index_obj.to_bytes();
                    
                    // Verifică dacă fișierul este binar
                    if is_binary_content(&content) {
                        println!("Binary file a/{} has been deleted", path_str);
                        return Ok(());
                    }
                    
                    // Generează un hash fictiv pentru formatul git
                    let index_hash = entry.get_oid();
                    let index_hash_short = if index_hash.len() >= 7 { &index_hash[0..7] } else { index_hash };
                    
                    println!("index {}..0000000 {}", index_hash_short, entry.mode_octal());
                    println!("--- a/{}", path_str);
                    println!("+++ /dev/null");
                    println!("@@ -1,{} +0,0 @@", content.len());
                    
                    let lines = diff::split_lines(&String::from_utf8_lossy(&content));
                    
                    for line in &lines {
                        println!("{}", Color::red(&format!("-{}", line)));
                    }
                    
                    return Ok(());
                }
                
                // Citește copia de lucru
                let file_content = workspace.read_file(path)?;
                
                // Calculează hash-ul pentru conținutul fișierului
                let file_hash = database.hash_file_data(&file_content);
                
                // Dacă hash-ul se potrivește, nu există nicio modificare
                if file_hash == entry.get_oid() {
                    println!("{}", Color::green(&format!("No changes in {}", path_str)));
                    return Ok(());
                }
                
                // Verifică dacă fișierul este binar
                if is_binary_content(&file_content) {
                    println!("index {}..{} {}", 
                            &entry.get_oid()[0..std::cmp::min(7, entry.get_oid().len())], 
                            &file_hash[0..std::cmp::min(7, file_hash.len())], 
                            entry.mode_octal());
                    println!("Binary files a/{} and b/{} differ", path_str, path_str);
                    return Ok(());
                }
                
                // Arată diff-ul între index și copia de lucru
                // Generează hash-uri scurte pentru formatul git
                let index_hash_short = if entry.get_oid().len() >= 7 { &entry.get_oid()[0..7] } else { entry.get_oid() };
                let file_hash_short = if file_hash.len() >= 7 { &file_hash[0..7] } else { &file_hash };
                
                println!("index {}..{} {}", index_hash_short, file_hash_short, entry.mode_octal());
                println!("--- a/{}", path_str);
                println!("+++ b/{}", path_str);
                
                // Folosește diff_with_database din modulul diff pentru a obține conținutul diff-ului
                let raw_diff_output = diff::diff_with_database(workspace, database, path, entry.get_oid(), 3)?;
                
                // Extrage doar partea cu diferențele (fără antetele adăugate de diff_with_database)
                let lines: Vec<&str> = raw_diff_output.lines().collect();
                let diff_content = if lines.len() > 3 {
                    // Sari peste primele 3 linii (antetele) care sunt deja afișate
                    lines[3..].join("\n")
                } else {
                    raw_diff_output
                };
                
                // Colorează și afișează diff-ul
                print!("{}", DiffCommand::colorize_diff_output(&diff_content));
            }
        } else {
            // Calea nu este în index
            if workspace.path_exists(path)? {
                println!("{}", Color::red(&format!("error: path '{}' is untracked", path_str)));
            } else {
                println!("{}", Color::red(&format!("error: path '{}' does not exist", path_str)));
            }
        }
        
        Ok(())
    }
    fn diff_index_vs_head(
        workspace: &Workspace,
        database: &mut Database,
        index: &Index,
        refs: &Refs
    ) -> Result<(), Error> {
        // Obține commit-ul HEAD
        let head_oid = match refs.read_head()? {
            Some(oid) => oid,
            None => {
                println!("{}", Color::yellow("No HEAD commit found. Index contains initial version."));
                return Ok(());
            }
        };
        
        // Încarcă commit-ul HEAD
        let commit_obj = database.load(&head_oid)?;
        let commit = match commit_obj.as_any().downcast_ref::<Commit>() {
            Some(c) => c,
            None => return Err(Error::Generic("HEAD is not a commit".into())),
        };
        
        // Obține fișierele din HEAD
        let mut head_files: HashMap<String, String> = HashMap::new();
        DiffCommand::collect_files_from_commit(database, commit, &mut head_files)?;
        
        let mut has_changes = false;
        
        // Compară fișierele din index cu HEAD
        for entry in index.each_entry() {
            let path = entry.get_path();
            
            if let Some(head_oid) = head_files.get(path) {
                // Fișierul există atât în index, cât și în HEAD
                if head_oid == entry.get_oid() {
                    // Nicio modificare
                    continue;
                }
                
                // Fișierul a fost modificat
                has_changes = true;
                
                // Generează hash-uri scurte pentru antetul git
                let head_hash_short = if head_oid.len() >= 7 { &head_oid[0..7] } else { head_oid };
                let index_hash_short = if entry.get_oid().len() >= 7 { &entry.get_oid()[0..7] } else { entry.get_oid() };
                
                println!("index {}..{} {}", head_hash_short, index_hash_short, entry.mode_octal());
                println!("--- a/{}", path);
                println!("+++ b/{}", path);
                
                // Încarcă ambele versiuni
                let head_obj = database.load(head_oid)?;
                let index_obj = database.load(entry.get_oid())?;
                
                let head_content = head_obj.to_bytes();
                let index_content = index_obj.to_bytes();
                
                // Verifică dacă fișierul este binar
                if is_binary_content(&head_content) || is_binary_content(&index_content) {
                    println!("Binary files a/{} and b/{} differ", path, path);
                    continue;
                }
                
                let head_lines = diff::split_lines(&String::from_utf8_lossy(&head_content));
                let index_lines = diff::split_lines(&String::from_utf8_lossy(&index_content));
                
                // Calculează diff-ul
                let edits = diff_lines(&head_lines, &index_lines);
                let raw_diff = format_diff(&head_lines, &index_lines, &edits, 3);
                
                // Colorează și afișează diff-ul
                let colored_diff = DiffCommand::colorize_diff_output(&raw_diff);
                print!("{}", colored_diff);
            } else {
                // Fișierul există în index, dar nu în HEAD (fișier nou)
                has_changes = true;
                
                // Generează hash-ul pentru antetul git
                let index_hash_short = if entry.get_oid().len() >= 7 { &entry.get_oid()[0..7] } else { entry.get_oid() };
                
                println!("index 0000000..{} {}", index_hash_short, entry.mode_octal());
                println!("--- /dev/null");
                println!("+++ b/{}", path);
                
                // Încarcă versiunea din index
                let index_obj = database.load(entry.get_oid())?;
                let content = index_obj.to_bytes();
                
                // Verifică dacă fișierul este binar
                if is_binary_content(&content) {
                    println!("Binary file b/{} created", path);
                    continue;
                }
                
                let lines = diff::split_lines(&String::from_utf8_lossy(&content));
                
                // Afișează antetul hunk-ului
                println!("@@ -0,0 +1,{} @@", lines.len());
                
                // Arată diff-ul de adăugare
                for line in &lines {
                    println!("{}", Color::green(&format!("+{}", line)));
                }
            }
        }
        
        // Verifică fișierele din HEAD care au fost eliminate din index
        for (path, head_oid) in &head_files {
            if !index.tracked(path) {
                // Fișierul a fost în HEAD, dar a fost eliminat din index
                has_changes = true;
                
                // Generează hash-ul pentru antetul git
                let head_hash_short = if head_oid.len() >= 7 { &head_oid[0..7] } else { head_oid };
                
                println!("index {}..0000000", head_hash_short);
                println!("--- a/{}", path);
                println!("+++ /dev/null");
                
                // Încarcă versiunea din HEAD
                let head_obj = database.load(head_oid)?;
                let content = head_obj.to_bytes();
                
                // Verifică dacă fișierul este binar
                if is_binary_content(&content) {
                    println!("Binary file a/{} deleted", path);
                    continue;
                }
                
                let lines = diff::split_lines(&String::from_utf8_lossy(&content));
                
                // Afișează antetul hunk-ului
                println!("@@ -1,{} +0,0 @@", lines.len());
                
                // Arată diff-ul de ștergere
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
}