use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use sha1::{Digest, Sha1};
use flate2::read::ZlibDecoder;

use crate::core::database::Database;
use crate::core::index::Index;
use crate::core::workspace::Workspace;
use crate::core::refs::Refs;
use crate::errors::error::Error;

/// Structură pentru raportul de verificare a obiectelor
pub struct ObjectStats {
    pub total: usize,
    pub blobs: usize,
    pub trees: usize,
    pub commits: usize,
    pub unknown: usize,
}

/// Structură pentru o intrare din tree
pub struct TreeEntry {
    pub mode: String,
    pub name: String,
    pub sha: String,
    pub entry_type: String,
    pub exists: bool,
}

/// Structură pentru rezultatul verificării unui commit
pub struct CommitInfo {
    pub oid: String,
    pub tree_oid: String,
    pub parent_oid: Option<String>,
    pub author: String,
    pub committer: String,
    pub message: String,
    pub tree_entries: Vec<TreeEntry>,
}

/// Structură pentru raportul de verificare a repository-ului
pub struct RepositoryReport {
    pub is_valid: bool,
    pub repo_path: PathBuf,
    pub ash_dirs_valid: bool,
    pub index_entries: usize,
    pub index_valid: bool,
    pub head_commit: Option<String>,
    pub object_stats: ObjectStats,
    pub issues: Vec<String>,
}

/// Clasa principală pentru verificarea integrității
pub struct IntegrityChecker {
    root_path: PathBuf,
    git_path: PathBuf,
    database: Database,
    issues: Vec<String>,
}

impl IntegrityChecker {
    /// Creează un nou verificator de integritate
    pub fn new(root_path: &Path) -> Self {
        let git_path = root_path.join(".ash");
        let db_path = git_path.join("objects");
        
        IntegrityChecker {
            root_path: root_path.to_path_buf(),
            git_path,
            database: Database::new(db_path),
            issues: Vec::new(),
        }
    }
    
    /// Verifică integritatea întregului repository
    pub fn check_repository(&mut self) -> Result<RepositoryReport, Error> {
        println!("\n========== VERIFICARE INTEGRITATE REPOSITORY ==========");
        
        // Inițializare raport
        let mut report = RepositoryReport {
            is_valid: true,
            repo_path: self.root_path.clone(),
            ash_dirs_valid: true,
            index_entries: 0,
            index_valid: true,
            head_commit: None,
            object_stats: ObjectStats {
                total: 0,
                blobs: 0,
                trees: 0,
                commits: 0,
                unknown: 0,
            },
            issues: Vec::new(),
        };
        
        // 1. Verifică structura repository-ului
        self.check_repository_structure(&mut report);
        
        // 2. Verifică indexul
        self.check_index(&mut report);
        
        // 3. Verifică HEAD și commit-uri
        self.check_head_and_commits(&mut report);
        
        // 4. Verifică coerența obiectelor
        self.check_objects_storage(&mut report);
        
        // 5. Verifică fișiere neindexate (opțional)
        self.check_untracked_files(&mut report);
        
        // Finalizează raportul
        report.is_valid = report.ash_dirs_valid && report.index_valid && self.issues.is_empty();
        report.issues = self.issues.clone();
        
        println!("\n========== REZULTAT FINAL ==========");
        if report.is_valid {
            println!("✅ Repository-ul este într-o stare validă");
        } else {
            println!("❌ Repository-ul are inconsistențe care necesită rezolvare");
            for issue in &report.issues {
                println!("  - {}", issue);
            }
        }
        
        Ok(report)
    }
    
    /// Verifică existența structurii de directoare necesare
    fn check_repository_structure(&mut self, report: &mut RepositoryReport) {
        println!("1. Verificare structură repository");
        
        if !self.git_path.exists() {
            println!("❌ Directorul .ash nu există");
            report.ash_dirs_valid = false;
            self.issues.push("Repository-ul nu este inițializat (.ash lipsește)".to_string());
            return;
        }
        
        for dir in ["objects", "refs"] {
            let dir_path = self.git_path.join(dir);
            if !dir_path.exists() {
                println!("❌ Directorul {} nu există", dir_path.display());
                report.ash_dirs_valid = false;
                self.issues.push(format!("Directorul {} lipsește", dir_path.display()));
            } else {
                println!("✅ Directorul {} există", dir_path.display());
            }
        }
    }
    
    /// Verifică indexul și intrările sale
    fn check_index(&mut self, report: &mut RepositoryReport) {
        println!("\n2. Verificare index");
        let index_path = self.git_path.join("index");
        
        if !index_path.exists() {
            println!("❌ Fișierul index nu există");
            report.index_valid = false;
            self.issues.push("Fișierul index nu există".to_string());
            return;
        }
        
        println!("✅ Fișierul index există");
        
        let mut index = Index::new(&index_path);
        
        // Încarcă indexul
        match index.load() {
            Ok(_) => {
                println!("✅ Indexul a fost încărcat cu succes");
                
                // Verifică intrările
                let entry_count = index.entries.len();
                report.index_entries = entry_count;
                println!("📊 Indexul conține {} intrări", entry_count);
                
                if entry_count > 0 {
                    println!("\n3. Verificare intrări în index");
                    
                    for (i, entry) in index.each_entry().enumerate() {
                        println!("\nIntrarea #{}: {}", i+1, entry.path);
                        println!("   OID: {}", entry.oid);
                        println!("   Mod: {} ({})", entry.mode, entry.mode_octal());
                        
                        // Verifică existența obiectului
                        if self.database.exists(&entry.oid) {
                            println!("   ✅ Obiectul există în baza de date");
                            
                            // Verifică integritatea obiectului
                            match self.verify_object_content(&entry.oid) {
                                Ok(_) => println!("   ✅ Conținutul obiectului este valid"),
                                Err(e) => {
                                    println!("   ❌ Eroare la verificarea conținutului: {}", e);
                                    report.index_valid = false;
                                    self.issues.push(format!("Obiectul {} este corupt: {}", entry.oid, e));
                                }
                            }
                        } else {
                            println!("   ❌ Obiectul nu există în baza de date");
                            report.index_valid = false;
                            self.issues.push(format!("Obiectul {} referit în index nu există", entry.oid));
                        }
                        
                        // Verifică că calea fișierului există în workspace
                        let workspace_path = self.root_path.join(&entry.path);
                        if workspace_path.exists() {
                            println!("   ✅ Fișierul există în workspace");
                            
                            // Verifică că hash-ul fișierului actual se potrivește cu cel din index
                            match self.verify_file_hash(&workspace_path, &entry.oid) {
                                Ok(true) => println!("   ✅ Hash-ul fișierului se potrivește cu cel din index"),
                                Ok(false) => {
                                    println!("   ❌ Hash-ul fișierului NU se potrivește cu cel din index");
                                    report.index_valid = false;
                                    self.issues.push(format!("Fișierul {} a fost modificat după adăugarea în index", entry.path));
                                },
                                Err(e) => println!("   ❌ Eroare la calculul hash-ului: {}", e)
                            }
                        } else {
                            println!("   ❌ Fișierul nu există în workspace");
                            self.issues.push(format!("Fișierul {} din index nu există în workspace", entry.path));
                        }
                    }
                }
            },
            Err(e) => {
                println!("❌ Eroare la încărcarea indexului: {}", e);
                report.index_valid = false;
                self.issues.push(format!("Nu s-a putut încărca indexul: {}", e));
            }
        }
    }
    
    /// Verifică HEAD și lanțul de commit-uri
    fn check_head_and_commits(&mut self, report: &mut RepositoryReport) {
        println!("\n4. Verificare HEAD și commit-uri");
        
        let refs = Refs::new(&self.git_path);
        match refs.read_head() {
            Ok(Some(head_oid)) => {
                println!("✅ HEAD există: {}", head_oid);
                report.head_commit = Some(head_oid.clone());
                
                // Verifică obiectul commit
                if self.database.exists(&head_oid) {
                    println!("✅ Commit-ul HEAD există în baza de date");
                    
                    // Verifică conținutul commit-ului
                    match self.verify_commit(&head_oid) {
                        Ok(commit_info) => {
                            println!("✅ Commit-ul este valid");
                            println!("   Tree: {}", commit_info.tree_oid);
                            
                            // Verifică tree-ul din commit
                            if self.database.exists(&commit_info.tree_oid) {
                                println!("✅ Tree-ul din commit există");
                                
                                // Verifică lanțul de commit-uri (opțional)
                                if let Some(parent) = &commit_info.parent_oid {
                                    println!("   Parent: {}", parent);
                                    self.verify_commit_chain(parent, 5); // Verifică până la 5 commit-uri înapoi
                                }
                            } else {
                                println!("❌ Tree-ul din commit nu există");
                                report.index_valid = false;
                                self.issues.push(format!("Tree-ul {} din commit nu există", commit_info.tree_oid));
                            }
                        },
                        Err(e) => {
                            println!("❌ Commit-ul nu este valid: {}", e);
                            report.index_valid = false;
                            self.issues.push(format!("Commit-ul HEAD nu este valid: {}", e));
                        }
                    }
                } else {
                    println!("❌ Commit-ul HEAD nu există în baza de date");
                    report.index_valid = false;
                    self.issues.push(format!("Commit-ul HEAD {} nu există în baza de date", head_oid));
                }
            },
            Ok(None) => println!("ℹ️ Nu există încă niciun commit (HEAD nu există)"),
            Err(e) => {
                println!("❌ Eroare la citirea HEAD: {}", e);
                report.index_valid = false;
                self.issues.push(format!("Eroare la citirea HEAD: {}", e));
            }
        }
    }
    
    /// Verifică coerența obiectelor în storage
    fn check_objects_storage(&mut self, report: &mut RepositoryReport) -> Result<(), Error> {
        println!("\n5. Verificare coherență obiecte");
        
        let mut total_objects = 0;
        let mut blob_count = 0;
        let mut tree_count = 0;
        let mut commit_count = 0;
        let mut unknown_count = 0;
        
        let objects_dir = &self.database.pathname;
        for prefix_entry in fs::read_dir(objects_dir)? {
            let prefix_entry = prefix_entry?;
            let prefix_path = prefix_entry.path();
            
            if prefix_path.is_dir() && prefix_path.file_name().unwrap().len() == 2 {
                for obj_entry in fs::read_dir(&prefix_path)? {
                    let obj_entry = obj_entry?;
                    let obj_path = obj_entry.path();
                    
                    if obj_path.is_file() {
                        total_objects += 1;
                        
                        // Determină tipul obiectului
                        let prefix = prefix_path.file_name().unwrap().to_string_lossy();
                        let suffix = obj_path.file_name().unwrap().to_string_lossy();
                        let oid = format!("{}{}", prefix, suffix);
                        
                        let file = File::open(&obj_path)?;
                        let mut decoder = ZlibDecoder::new(file);
                        let mut content = Vec::new();
                        
                        match decoder.read_to_end(&mut content) {
                            Ok(_) => {
                                if let Some(null_pos) = content.iter().position(|&b| b == 0) {
                                    let header = String::from_utf8_lossy(&content[0..null_pos]);
                                    if header.starts_with("blob") {
                                        blob_count += 1;
                                    } else if header.starts_with("tree") {
                                        tree_count += 1;
                                    } else if header.starts_with("commit") {
                                        commit_count += 1;
                                    } else {
                                        unknown_count += 1;
                                        println!("   ⚠️ Obiect cu header necunoscut: {}", header);
                                    }
                                } else {
                                    unknown_count += 1;
                                    println!("   ⚠️ Obiect fără separator null: {}", oid);
                                }
                            },
                            Err(e) => {
                                unknown_count += 1;
                                println!("   ⚠️ Nu s-a putut citi obiectul {}: {}", oid, e);
                                self.issues.push(format!("Obiectul {} este corupt: {}", oid, e));
                            }
                        }
                    }
                }
            }
        }
        
        report.object_stats = ObjectStats {
            total: total_objects,
            blobs: blob_count,
            trees: tree_count,
            commits: commit_count,
            unknown: unknown_count,
        };
        
        println!("✅ Verificare completă a bazei de date de obiecte");
        println!("   - Obiecte totale: {}", total_objects);
        println!("   - Obiecte blob: {}", blob_count);
        println!("   - Obiecte tree: {}", tree_count);
        println!("   - Obiecte commit: {}", commit_count);
        if unknown_count > 0 {
            println!("   - Obiecte necunoscute: {}", unknown_count);
        }
        
        Ok(())
    }
    
    /// Verifică fișiere neindexate (opțional)
    fn check_untracked_files(&self, report: &mut RepositoryReport) {
        println!("\n6. Căutare fișiere neindexate");
        
        let workspace = Workspace::new(&self.root_path);
        let index_path = self.git_path.join("index");
        let mut index = Index::new(index_path);
        
        // Încarcă indexul
        if index.load().is_err() {
            println!("⚠️ Nu s-a putut încărca indexul pentru verificarea fișierelor neindexate");
            return;
        }
        
        // Listează fișierele din workspace
        match workspace.list_files() {
            Ok(files) => {
                let mut untracked_files = Vec::new();
                
                for file in files {
                    let file_path = file.to_string_lossy().to_string();
                    if !index.entries.contains_key(&file_path) {
                        untracked_files.push(file_path);
                    }
                }
                
                if untracked_files.is_empty() {
                    println!("✅ Toate fișierele din workspace sunt în index");
                } else {
                    println!("ℹ️ Fișiere neindexate ({}):", untracked_files.len());
                    for file in untracked_files.iter().take(10) {
                        println!("   - {}", file);
                    }
                    if untracked_files.len() > 10 {
                        println!("   ... și încă {} fișiere", untracked_files.len() - 10);
                    }
                }
            },
            Err(e) => println!("⚠️ Eroare la listarea fișierelor din workspace: {}", e)
        }
    }
    
    /// Verifică un lanț de commit-uri până la o anumită adâncime
    fn verify_commit_chain(&self, start_oid: &str, depth: usize) {
        if depth == 0 {
            println!("   ℹ️ Adâncime maximă atinsă în verificarea lanțului de commit-uri");
            return;
        }
        
        match self.verify_commit(start_oid) {
            Ok(commit_info) => {
                println!("   ✅ Commit-ul parent {} este valid", start_oid);
                
                if let Some(parent) = &commit_info.parent_oid {
                    self.verify_commit_chain(parent, depth - 1);
                }
            },
            Err(e) => {
                println!("   ❌ Commit-ul parent {} nu este valid: {}", start_oid, e);
                self.issues.push(format!("Commit-ul {} din lanțul istoric nu este valid: {}", start_oid, e));
            }
        }
    }
    
    /// Verifică conținutul unui obiect
    fn verify_object_content(&self, oid: &str) -> Result<(), Error> {
        let object_path = self.database.pathname.join(&oid[0..2]).join(&oid[2..]);
        let file = File::open(&object_path)?;
        
        let mut decoder = ZlibDecoder::new(file);
        let mut content = Vec::new();
        decoder.read_to_end(&mut content)?;
        
        let null_pos = content.iter().position(|&b| b == 0)
            .ok_or_else(|| Error::Generic("Invalid object format: missing null byte".to_string()))?;
        
        let header = String::from_utf8_lossy(&content[0..null_pos]).to_string();
        let parts: Vec<&str> = header.split(' ').collect();
        
        if parts.len() != 2 {
            return Err(Error::Generic(format!("Invalid header format: {}", header)));
        }
        
        let obj_type = parts[0];
        if !["blob", "tree", "commit"].contains(&obj_type) {
            return Err(Error::Generic(format!("Invalid object type: {}", obj_type)));
        }
        
        let obj_size: usize = parts[1].parse()
            .map_err(|_| Error::Generic(format!("Invalid size in header: {}", parts[1])))?;
        
        if obj_size != content.len() - null_pos - 1 {
            return Err(Error::Generic(format!(
                "Size mismatch: header claims {} bytes, actual content is {} bytes",
                obj_size, content.len() - null_pos - 1
            )));
        }
        
        Ok(())
    }
    
    /// Verifică hash-ul unui fișier
    fn verify_file_hash(&self, file_path: &Path, expected_oid: &str) -> Result<bool, Error> {
        let data = fs::read(file_path)?;
        
        let header = format!("blob {}\0", data.len());
        let mut full_content = header.as_bytes().to_vec();
        full_content.extend(&data);
        
        let mut hasher = Sha1::new();
        hasher.update(&full_content);
        let result = hasher.finalize();
        let actual_oid = format!("{:x}", result);
        
        Ok(actual_oid == expected_oid)
    }
    
    /// Verifică un commit
    fn verify_commit(&self, oid: &str) -> Result<CommitInfo, Error> {
        let object_path = self.database.pathname.join(&oid[0..2]).join(&oid[2..]);
        let file = File::open(&object_path)?;
        
        let mut decoder = ZlibDecoder::new(file);
        let mut content = Vec::new();
        decoder.read_to_end(&mut content)?;
        
        let null_pos = content.iter().position(|&b| b == 0)
            .ok_or_else(|| Error::Generic("Invalid object format: missing null byte".to_string()))?;
        
        let header = String::from_utf8_lossy(&content[0..null_pos]).to_string();
        if !header.starts_with("commit ") {
            return Err(Error::Generic(format!("Invalid commit header: {}", header)));
        }
        
        let commit_size: usize = header[7..].parse()
            .map_err(|_| Error::Generic("Invalid commit size in header".to_string()))?;
        
        if commit_size != content.len() - null_pos - 1 {
            println!("⚠️ Dimensiunea din header ({}) nu se potrivește cu dimensiunea reală ({}) a commit-ului",
                commit_size, content.len() - null_pos - 1);
        }
        
        let commit_data = String::from_utf8_lossy(&content[null_pos+1..]).to_string();
        
        // Parsează conținutul commit-ului
        let mut tree_oid = String::new();
        let mut parent_oid = None;
        let mut author = String::new();
        let mut committer = String::new();
        let mut message = String::new();
        let mut in_message = false;
        
        for line in commit_data.lines() {
            if in_message {
                message.push_str(line);
                message.push('\n');
                continue;
            }
            
            if line.is_empty() {
                in_message = true;
                continue;
            }
            
            if line.starts_with("tree ") {
                tree_oid = line[5..].to_string();
            } else if line.starts_with("parent ") {
                parent_oid = Some(line[7..].to_string());
            } else if line.starts_with("author ") {
                author = line[7..].to_string();
            } else if line.starts_with("committer ") {
                committer = line[10..].to_string();
            }
        }
        
        message = message.trim().to_string();
        
        // Verifică componentele esențiale
        if tree_oid.is_empty() {
            return Err(Error::Generic("Commit-ul nu conține o referință la tree".to_string()));
        }
        
        // Verifică tree-ul
        let tree_entries = self.verify_tree(&tree_oid)?;
        
        Ok(CommitInfo {
            oid: oid.to_string(),
            tree_oid,
            parent_oid,
            author,
            committer,
            message,
            tree_entries,
        })
    }
    
    /// Verifică un tree
    fn verify_tree(&self, tree_oid: &str) -> Result<Vec<TreeEntry>, Error> {
        if !self.database.exists(tree_oid) {
            return Err(Error::Generic(format!("Tree-ul {} nu există", tree_oid)));
        }
        
        let object_path = self.database.pathname.join(&tree_oid[0..2]).join(&tree_oid[2..]);
        let file = File::open(&object_path)?;
        
        let mut decoder = ZlibDecoder::new(file);
        let mut content = Vec::new();
        decoder.read_to_end(&mut content)?;
        
        let null_pos = content.iter().position(|&b| b == 0)
            .ok_or_else(|| Error::Generic("Invalid tree format: missing null byte".to_string()))?;
        
        let header = String::from_utf8_lossy(&content[0..null_pos]).to_string();
        if !header.starts_with("tree ") {
            return Err(Error::Generic(format!("Invalid tree header: {}", header)));
        }
        
        // Parsează tree entries
        let mut entries = Vec::new();
        let mut pos = null_pos + 1;
        
        while pos < content.len() {
            // Format: "<mode> <name>\0<sha1>"
            let space_pos = content[pos..].iter().position(|&b| b == b' ')
                .ok_or_else(|| Error::Generic("Invalid tree entry: missing space".to_string()))?;
            
            let mode = String::from_utf8_lossy(&content[pos..pos+space_pos]).to_string();
            pos += space_pos + 1;
            
            let null_pos = content[pos..].iter().position(|&b| b == 0)
                .ok_or_else(|| Error::Generic("Invalid tree entry: missing null".to_string()))?;
            
            let name = String::from_utf8_lossy(&content[pos..pos+null_pos]).to_string();
            pos += null_pos + 1;
            
            // SHA-1 is always 20 bytes
            if pos + 20 > content.len() {
                return Err(Error::Generic("Invalid tree entry: truncated SHA-1".to_string()));
            }
            
            let sha = hex::encode(&content[pos..pos+20]);
            pos += 20;
            
            // Verifică obiectul
            let entry_exists = self.database.exists(&sha);
            
            // Adaugă entry în rezultat
            let entry_type = if mode.starts_with("100") { "blob" } else { "tree" };
            
            let status = if entry_exists { "✅" } else { "❌" };
            println!("{} {} {} {} ({})", 
                status, mode, entry_type, name, sha);
            
            entries.push(TreeEntry {
                mode,
                name,
                sha,
                entry_type: entry_type.to_string(),
                exists: entry_exists,
            });
        }
        
        Ok(entries)
    }
}