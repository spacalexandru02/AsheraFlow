use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use sha1::{Digest, Sha1};
use flate2::write::ZlibEncoder;
use flate2::Compression;
use crate::core::index::index::Index;
use crate::core::workspace::Workspace;
use crate::errors::error::Error;
use std::io::Read;

pub struct Database {
    pub pathname: PathBuf,
    temp_chars: Vec<char>,
}

pub trait GitObject {
    fn get_type(&self) -> &str;
    fn to_bytes(&self) -> Vec<u8>;
    fn set_oid(&mut self, oid: String);
}

impl Database {
    pub fn verify_repository_integrity(self, root_path: &Path) -> Result<(), Error> {
        println!("\n========== VERIFICARE INTEGRITATE REPOSITORY ==========");
        
        let git_path = root_path.join(".ash");
        let db_path = git_path.join("objects");
        let index_path = git_path.join("index");
        
        // Verifică existența structurii de directoare
        if !git_path.exists() {
            return Err(Error::Generic(format!("Repository path does not exist: {}", git_path.display())));
        }
        
        println!("1. Verificare structură repository");
        for dir in ["objects", "refs"] {
            let dir_path = git_path.join(dir);
            if !dir_path.exists() {
                println!("❌ Directorul {} nu există", dir_path.display());
            } else {
                println!("✅ Directorul {} există", dir_path.display());
            }
        }
        
        // Inițializează componentele
        let database = Database::new(db_path);
        let mut index = Index::new(&index_path);
        
        // Verifică indexul
        println!("\n2. Verificare index");
        match File::open(&index_path) {
            Ok(_) => println!("✅ Fișierul index există"),
            Err(_) => {
                println!("❌ Fișierul index nu există");
                return Err(Error::Generic("Index file does not exist".to_string()));
            }
        }
        
        let mut index_valid = true;
        
        // Încarcă indexul
        match index.load() {
            Ok(_) => println!("✅ Indexul a fost încărcat cu succes"),
            Err(e) => {
                println!("❌ Eroare la încărcarea indexului: {}", e);
                index_valid = false;
            }
        }
        
        // Verifică numărul de intrări
        if index_valid {
            let entry_count = index.entries.len();
            println!("📊 Indexul conține {} intrări", entry_count);
            
            // Verifică fiecare intrare
            println!("\n3. Verificare intrări în index");
            
            for (i, entry) in index.each_entry().enumerate() {
                println!("\nIntrarea #{}: {}", i+1, entry.path);
                println!("   OID: {}", entry.oid);
                println!("   Mod: {} ({})", entry.mode, entry.mode_octal());
                
                // Verifică existența obiectului
                if database.exists(&entry.oid) {
                    println!("   ✅ Obiectul există în baza de date");
                    
                    // Verifică integritatea obiectului
                    match verify_object_content(&database, &entry.oid) {
                        Ok(_) => println!("   ✅ Conținutul obiectului este valid"),
                        Err(e) => println!("   ❌ Eroare la verificarea conținutului: {}", e)
                    }
                } else {
                    println!("   ❌ Obiectul nu există în baza de date");
                    index_valid = false;
                }
                
                // Verifică că calea fișierului există în workspace
                let workspace_path = root_path.join(&entry.path);
                if workspace_path.exists() {
                    println!("   ✅ Fișierul există în workspace");
                    
                    // Verifică că hash-ul fișierului actual se potrivește cu cel din index
                    match verify_file_hash(&workspace_path, &entry.oid) {
                        Ok(true) => println!("   ✅ Hash-ul fișierului se potrivește cu cel din index"),
                        Ok(false) => {
                            println!("   ❌ Hash-ul fișierului NU se potrivește cu cel din index");
                            index_valid = false;
                        },
                        Err(e) => println!("   ❌ Eroare la calculul hash-ului: {}", e)
                    }
                } else {
                    println!("   ❌ Fișierul nu există în workspace");
                    index_valid = false;
                }
            }
        }
        
        // Verifică trecerea inversă: fișiere din workspace care ar trebui să fie în index
        println!("\n4. Căutare fișiere neindexate");
        let workspace = Workspace::new(root_path);
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
            Err(e) => println!("❌ Eroare la listarea fișierelor din workspace: {}", e)
        }
        
        // Verifică coherența obiectelor din storage
        println!("\n5. Verificare coherență obiecte");
        match audit_objects_storage(&database) {
            Ok(stats) => {
                println!("✅ Verificare completă a bazei de date de obiecte");
                println!("   - Obiecte totale: {}", stats.0);
                println!("   - Obiecte blob: {}", stats.1);
                println!("   - Obiecte tree: {}", stats.2);
                println!("   - Obiecte commit: {}", stats.3);
            },
            Err(e) => println!("❌ Eroare la verificarea obiectelor: {}", e)
        }
        
        println!("\n========== REZULTAT FINAL ==========");
        if index_valid {
            println!("✅ Repository-ul este într-o stare validă");
            Ok(())
        } else {
            println!("❌ Repository-ul are inconsistențe care necesită rezolvare");
            Err(Error::Generic("Repository integrity check failed".to_string()))
        }

    }
    
    // Funcție helper pentru a verifica conținutul unui obiect
    
    
    pub fn new(pathname: PathBuf) -> Self {
        let temp_chars: Vec<char> = ('a'..='z')
            .chain('A'..='Z')
            .chain('0'..='9')
            .collect();

        Database {
            pathname,
            temp_chars,
        }
    }

    pub fn exists(&self, oid: &str) -> bool {
        self.pathname.join(&oid[0..2]).join(&oid[2..]).exists()
    }

    pub fn store(&mut self, object: &mut impl GitObject) -> Result<(), Error> {
        let content = object.to_bytes();
        let header = format!("{} {}\0", object.get_type(), content.len());
        let mut full_content = header.as_bytes().to_vec();
        full_content.extend(content);

        let oid = Self::calculate_oid(&full_content);
        
        if !self.exists(&oid) {
            self.write_object(&oid, &full_content)?;
        }

        // Asigurați-vă că `set_oid` este apelat întotdeauna
        object.set_oid(oid.clone());

        Ok(())
    }

    fn calculate_oid(content: &[u8]) -> String {
        let mut hasher = Sha1::new();
        hasher.update(content);
        let result = hasher.finalize();
        format!("{:x}", result)
    }

    fn write_object(&self, oid: &str, content: &[u8]) -> Result<(), Error> {
        let object_path = self.pathname.join(&oid[0..2]).join(&oid[2..]);
        
        // Return early if object exists
        if object_path.exists() {
            return Ok(());
        }
        
        let dirname = object_path.parent().ok_or_else(|| {
            Error::Generic(format!("Invalid object path: {}", object_path.display()))
        })?;

        if !dirname.exists() {
            fs::create_dir_all(dirname)?;
        }

        let temp_name = self.generate_temp_name();
        let temp_path = dirname.join(temp_name);

        let mut file = File::create(&temp_path)?;

        // Compress and write
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
        encoder.write_all(content)?;
        let compressed = encoder.finish()?;

        file.write_all(&compressed)?;
        fs::rename(temp_path, object_path)?;

        Ok(())
    }

    fn generate_temp_name(&self) -> String {
        use rand::seq::SliceRandom;
        let mut rng = rand::thread_rng();
        let name: String = (0..6)
            .map(|_| self.temp_chars.choose(&mut rng).unwrap())
            .collect();
        format!("tmp_obj_{}", name)
    }

    pub fn get_object_path(&self, oid: &str) -> PathBuf {
        self.pathname.join(&oid[0..2]).join(&oid[2..])
    }

    pub fn verify_object(&self, oid: &str) -> Result<(), Error> {
        println!("Verificare obiect: {}", oid);
        
        // Verifică dacă obiectul există
        if !self.exists(oid) {
            return Err(Error::Generic(format!("Obiectul {} nu există", oid)));
        }
        
        // Încearcă să citești obiectul
        let object_path = self.get_object_path(oid);
        let file = File::open(&object_path)?;
        
        // Decomprima obiectul
        let mut decoder = flate2::read::ZlibDecoder::new(file);
        let mut content = Vec::new();
        
        match decoder.read_to_end(&mut content) {
            Ok(size) => {
                println!("Obiect citit: {} bytes", size);
                
                // Verifică formatul obiectului
                let null_pos = content.iter().position(|&b| b == 0);
                if let Some(pos) = null_pos {
                    let header = String::from_utf8_lossy(&content[0..pos]);
                    println!("Header: {}", header);
                    
                    // Verifică dacă header-ul are formatul corect "<type> <size>"
                    let parts: Vec<&str> = header.split(' ').collect();
                    if parts.len() == 2 {
                        let obj_type = parts[0];
                        let obj_size: usize = parts[1].parse().unwrap_or(0);
                        
                        println!("Tip: {}, Dimensiune declarată: {}", obj_type, obj_size);
                        println!("Dimensiune reală: {}", content.len() - pos - 1);
                        
                        // Verifică dacă dimensiunea declarată se potrivește cu cea reală
                        if obj_size == content.len() - pos - 1 {
                            println!("✓ Dimensiunea se potrivește");
                        } else {
                            println!("✗ Dimensiunea nu se potrivește");
                        }
                    } else {
                        println!("✗ Format header invalid");
                    }
                } else {
                    println!("✗ Nu s-a găsit byte-ul null separator în header");
                }
                
                Ok(())
            },
            Err(e) => {
                Err(Error::Generic(format!("Eroare la decomprimarea obiectului: {}", e)))
            }
        }
    }

    
// Funcție pentru a verifica că hash-ul unui fișier se potrivește cu cel din index

}

pub fn verify_object_content(database: &Database, oid: &str) -> Result<(), Error> {
    let object_path = database.pathname.join(&oid[0..2]).join(&oid[2..]);
    let file = File::open(&object_path)?;
    
    let mut decoder = flate2::read::ZlibDecoder::new(file);
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

pub fn verify_file_hash(file_path: &Path, expected_oid: &str) -> Result<bool, Error> {
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

// Funcție pentru a audita întreaga bază de date de obiecte
// Returnează (total_obiecte, blob_count, tree_count, commit_count)
pub fn audit_objects_storage(database: &Database) -> Result<(usize, usize, usize, usize), Error> {
    let mut total_objects = 0;
    let mut blob_count = 0;
    let mut tree_count = 0;
    let mut commit_count = 0;
    
    let objects_dir = &database.pathname;
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
                    let mut decoder = flate2::read::ZlibDecoder::new(file);
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
                                }
                            }
                        },
                        Err(e) => {
                            println!("   ⚠️ Nu s-a putut citi obiectul {}: {}", oid, e);
                        }
                    }
                }
            }
        }
    }
    
    Ok((total_objects, blob_count, tree_count, commit_count))
}