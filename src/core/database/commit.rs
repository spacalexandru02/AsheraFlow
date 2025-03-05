use std::fs::File;

use crate::errors::error::Error;

use super::{author::Author, database::{Database, GitObject}};

use std::io::Read;

pub struct Commit {
    oid: Option<String>,
    parent: Option<String>,
    tree: String,
    author: Author,
    message: String,
}

pub struct CommitVerificationResult {
    pub commit_oid: String,
    pub tree_oid: String,
    pub parent_oid: Option<String>,
    pub author: String,
    pub committer: String,
    pub message: String,
    pub tree_entries: Vec<TreeEntry>,
}

pub struct TreeEntry {
    pub mode: String,
    pub name: String,
    pub sha: String,
    pub entry_type: String,
    pub exists: bool,
}

impl GitObject for Commit {
    fn get_type(&self) -> &str {
        "commit"
    }

    fn to_bytes(&self) -> Vec<u8> {
        self.to_bytes()
    }

    fn set_oid(&mut self, oid: String) {
        self.oid = Some(oid);
    }
}

impl Commit {
    pub fn new(parent: Option<String>, tree: String, author: Author, message: String) -> Self {
        Commit {
            oid: None,
            parent,
            tree,
            author,
            message,
        }
    }

    pub fn get_oid(&self) -> Option<&String> {
        self.oid.as_ref()
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let timestamp = self.author.timestamp.timestamp();
        let author_line = format!(
            "{} <{}> {} +0000", 
            self.author.name, 
            self.author.email, 
            timestamp
        );
    
        let mut lines = Vec::with_capacity(5);
        
        lines.push(format!("tree {}", self.tree));
        lines.push(format!("author {}", author_line));
        lines.push(format!("committer {}", author_line));
    
        if let Some(parent) = &self.parent {
            lines.push(format!("parent {}", parent));
        }
    
        lines.push(String::new()); // Empty line before message
        lines.push(self.message.clone());
    
        lines.join("\n").into_bytes()
    }
    pub fn verify(database: &Database, oid: &str) -> Result<CommitVerificationResult, Error> {
        println!("\n========== VERIFICARE COMMIT ==========");
        println!("Verificare commit: {}", oid);

        // Verifică dacă obiectul există
        if !database.exists(oid) {
            return Err(Error::Generic(format!("Commit-ul {} nu există", oid)));
        }

        // Citește și decomprimă obiectul
        let object_path = database.pathname.join(&oid[0..2]).join(&oid[2..]);
        let file = File::open(&object_path)?;
        
        let mut decoder = flate2::read::ZlibDecoder::new(file);
        let mut content = Vec::new();
        decoder.read_to_end(&mut content)?;

        // Verifică header-ul
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
        } else {
            println!("✅ Dimensiunea commit-ului este corectă: {} bytes", commit_size);
        }

        // Parsează conținutul commit-ului
        let commit_data = String::from_utf8_lossy(&content[null_pos+1..]).to_string();
        println!("Conținutul commit-ului:\n{}", commit_data);
        
        // Extrage tree, parent, author și message
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
        
        println!("Tree: {}", tree_oid);
        if database.exists(&tree_oid) {
            println!("✅ Tree-ul există în baza de date");
        } else {
            println!("❌ Tree-ul nu există în baza de date");
            return Err(Error::Generic(format!("Tree-ul {} nu există", tree_oid)));
        }
        
        if let Some(parent) = &parent_oid {
            println!("Parent: {}", parent);
            if database.exists(parent) {
                println!("✅ Parent-ul există în baza de date");
            } else {
                println!("❌ Parent-ul nu există în baza de date");
                return Err(Error::Generic(format!("Parent-ul {} nu există", parent)));
            }
        } else {
            println!("Este un commit inițial (fără parent)");
        }
        
        if author.is_empty() {
            println!("❌ Commit-ul nu conține informații despre autor");
            return Err(Error::Generic("Commit-ul nu conține informații despre autor".to_string()));
        } else {
            println!("Autor: {}", author);
        }
        
        if message.is_empty() {
            println!("⚠️ Commit-ul nu conține un mesaj");
        } else {
            println!("Mesaj: {}", message);
        }
        
        // Verifică tree-ul (opțional, poți apela aici o metodă de verificare a tree-ului)
        println!("\nVerificare structură tree:");
        let tree_result = Self::verify_tree(database, &tree_oid)?;
        
        Ok(CommitVerificationResult {
            commit_oid: oid.to_string(),
            tree_oid,
            parent_oid,
            author,
            committer,
            message,
            tree_entries: tree_result
        })
    }
    
    // Metodă helper pentru verificarea tree-ului din commit
    fn verify_tree(database: &Database, tree_oid: &str) -> Result<Vec<TreeEntry>, Error> {
        if !database.exists(tree_oid) {
            return Err(Error::Generic(format!("Tree-ul {} nu există", tree_oid)));
        }
        
        // Citește și decomprimă obiectul tree
        let object_path = database.pathname.join(&tree_oid[0..2]).join(&tree_oid[2..]);
        let file = File::open(&object_path)?;
        
        let mut decoder = flate2::read::ZlibDecoder::new(file);
        let mut content = Vec::new();
        decoder.read_to_end(&mut content)?;
        
        // Verifică header-ul
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
            let entry_exists = database.exists(&sha);
            
            // Adaugă entry în rezultat
            let entry_type = if mode.starts_with("100") { "blob" } else { "tree" };
            entries.push(TreeEntry {
                mode,
                name,
                sha,
                entry_type: entry_type.to_string(),
                exists: entry_exists,
            });
        }
        
        // Afișează rezultatele
        for entry in &entries {
            let status = if entry.exists { "✅" } else { "❌" };
            println!("{} {} {} {} ({})", 
                status, entry.mode, entry.entry_type, entry.name, entry.sha);
        }
        
        Ok(entries)
    }
}