// src/core/database/database.rs
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::io::Read;
use std::collections::HashMap;
use sha1::{Digest, Sha1};
use flate2::write::ZlibEncoder;
use flate2::read::ZlibDecoder;
use flate2::Compression;
use crate::errors::error::Error;
use crate::core::database::blob::Blob;
use crate::core::database::tree::Tree;
use crate::core::database::commit::Commit;
use crate::core::database::entry::Entry;
use std::any::Any;

use super::tree::TreeEntry;

pub struct Database {
    pub pathname: PathBuf,
    temp_chars: Vec<char>,
    objects: HashMap<String, Box<dyn GitObject>>,
}

pub trait GitObject {
    fn get_type(&self) -> &str;
    fn to_bytes(&self) -> Vec<u8>;
    fn set_oid(&mut self, oid: String);
    fn as_any(&self) -> &dyn Any;
}

impl Database {
    pub fn new(pathname: PathBuf) -> Self {
        let temp_chars: Vec<char> = ('a'..='z')
            .chain('A'..='Z')
            .chain('0'..='9')
            .collect();

        Database {
            pathname,
            temp_chars,
            objects: HashMap::new(),
        }
    }

    pub fn exists(&self, oid: &str) -> bool {
        self.object_path(oid).exists()
    }

    /// Încarcă un obiect din baza de date folosind OID-ul său
    pub fn load(&mut self, oid: &str) -> Result<Box<dyn GitObject>, Error> {
        // Verifică dacă obiectul e deja în cache
        if let Some(obj) = self.objects.get(oid) {
            // Clone the object to return it
            return Ok(self.clone_object(obj));
        }

        // Citește obiectul și pune-l în cache
        let object = self.read_object(oid)?;
        let result = self.clone_object(&object);
        self.objects.insert(oid.to_string(), object);
        
        Ok(result)
    }

    /// Metodă privată de clonare a unui obiect - implementare de bază
    /// Metodă privată de clonare a unui obiect
fn clone_object(&self, obj: &Box<dyn GitObject>) -> Box<dyn GitObject> {
    match obj.get_type() {
        "blob" => {
            let blob = obj.as_any().downcast_ref::<Blob>().unwrap();
            let mut new_blob = Blob::new(blob.to_bytes());
            if let Some(oid) = blob.get_oid() {
                new_blob.set_oid(oid.clone());
            }
            Box::new(new_blob)
        },
        "tree" => {
            let tree = obj.as_any().downcast_ref::<Tree>().unwrap();
            
            // Creăm un nou tree
            let mut new_tree = Tree::new();
            
            // Copiem intrările folosind metodele publice
            for (name, entry) in tree.get_entries() {
                match entry {
                    TreeEntry::Blob(oid, mode) => {
                        new_tree.insert_entry(name.clone(), TreeEntry::Blob(oid.clone(), *mode));
                    },
                    TreeEntry::Tree(subtree) => {
                        // Pentru simplificare, doar copiem referința OID a subtree-ului
                        let mut new_subtree = Tree::new();
                        if let Some(oid) = subtree.get_oid() {
                            new_subtree.set_oid(oid.clone());
                        }
                        new_tree.insert_entry(name.clone(), TreeEntry::Tree(Box::new(new_subtree)));
                    }
                }
            }
            
            // Setăm OID-ul dacă există
            if let Some(oid) = tree.get_oid() {
                new_tree.set_oid(oid.clone());
            }
            
            Box::new(new_tree)
        },
        "commit" => {
            let commit = obj.as_any().downcast_ref::<Commit>().unwrap();
            
            // Creăm un nou commit cu aceleași date
            let mut new_commit = Commit::new(
                commit.get_parent().cloned(),
                commit.get_tree().to_string(),
                commit.get_author().clone(),
                commit.get_message().to_string()
            );
            
            // Setăm OID-ul dacă există
            if let Some(oid) = commit.get_oid() {
                new_commit.set_oid(oid.clone());
            }
            
            Box::new(new_commit)
        },
        _ => panic!("Unknown object type"),
    }
}

    /// Stochează un obiect git în baza de date
    pub fn store(&mut self, object: &mut impl GitObject) -> Result<String, Error> {
        // Serializează obiectul
        let content = self.serialize_object(object)?;
        
        // Calculează OID-ul (hash)
        let oid = self.hash_content(&content);
        
        // Scrie doar dacă obiectul nu există deja
        if !self.exists(&oid) {
            self.write_object(&oid, &content)?;
        }

        // Setează OID-ul pe obiect
        object.set_oid(oid.clone());

        Ok(oid)
    }
    
    /// Calculează hash-ul unui obiect git fără a-l stoca
    pub fn hash_object(&self, object: &impl GitObject) -> Result<String, Error> {
        let content = self.serialize_object(object)?;
        Ok(self.hash_content(&content))
    }
    
    /// Serializează un obiect git în reprezentarea sa binară
    pub fn serialize_object(&self, object: &impl GitObject) -> Result<Vec<u8>, Error> {
        let content = object.to_bytes();
        let header = format!("{} {}\0", object.get_type(), content.len());
        let mut full_content = header.as_bytes().to_vec();
        full_content.extend(content);
        
        Ok(full_content)
    }
    
    /// Calculează hash-ul SHA-1 al conținutului
    pub fn hash_content(&self, content: &[u8]) -> String {
        let mut hasher = Sha1::new();
        hasher.update(content);
        let result = hasher.finalize();
        format!("{:x}", result)
    }

    /// Scrie un obiect în baza de date
    fn write_object(&self, oid: &str, content: &[u8]) -> Result<(), Error> {
        let object_path = self.object_path(oid);
        
        // Ieși devreme dacă obiectul există
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

        // Comprimă și scrie
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
        encoder.write_all(content)?;
        let compressed = encoder.finish()?;

        file.write_all(&compressed)?;
        fs::rename(temp_path, object_path)?;

        Ok(())
    }

    /// Obține calea către un obiect bazat pe OID
    fn object_path(&self, oid: &str) -> PathBuf {
        self.pathname.join(&oid[0..2]).join(&oid[2..])
    }

    /// Citește un obiect din baza de date și îl parsează
    fn read_object(&self, oid: &str) -> Result<Box<dyn GitObject>, Error> {
        let path = self.object_path(oid);
        
        if !path.exists() {
            return Err(Error::Generic(format!("Object not found: {}", oid)));
        }
        
        // Citește fișierul
        let mut file = File::open(&path)?;
        let mut compressed_data = Vec::new();
        file.read_to_end(&mut compressed_data)?;
        
        // Decomprimă datele
        let mut decoder = ZlibDecoder::new(&compressed_data[..]);
        let mut data = Vec::new();
        decoder.read_to_end(&mut data)?;
        
        // Parsează header-ul
        let null_pos = data.iter().position(|&b| b == 0)
            .ok_or_else(|| Error::Generic("Invalid object format: missing null byte".to_string()))?;
        
        let header = std::str::from_utf8(&data[0..null_pos])
            .map_err(|_| Error::Generic("Invalid header encoding".to_string()))?;
        
        let parts: Vec<&str> = header.split(' ').collect();
        if parts.len() != 2 {
            return Err(Error::Generic(format!("Invalid header format: {}", header)));
        }
        
        let obj_type = parts[0];
        let obj_size: usize = parts[1].parse()
            .map_err(|_| Error::Generic(format!("Invalid size in header: {}", parts[1])))?;
        
        // Verifică dimensiunea
        if obj_size != data.len() - null_pos - 1 {
            return Err(Error::Generic(format!(
                "Size mismatch: header claims {} bytes, actual content is {} bytes",
                obj_size, data.len() - null_pos - 1
            )));
        }
        
        // Extrage conținutul (după octetul null)
        let content = &data[null_pos + 1..];
        
        // Parsează obiectul în funcție de tip
        let mut object: Box<dyn GitObject> = match obj_type {
            "blob" => Box::new(Blob::parse(content)),
            "tree" => Box::new(Tree::parse(content)?),
            "commit" => Box::new(Commit::parse(content)?),
            _ => return Err(Error::Generic(format!("Unknown object type: {}", obj_type))),
        };
        
        // Setează OID-ul
        object.set_oid(oid.to_string());
        
        Ok(object)
    }

    fn generate_temp_name(&self) -> String {
        use rand::seq::SliceRandom;
        let mut rng = rand::thread_rng();
        let name: String = (0..6)
            .map(|_| self.temp_chars.choose(&mut rng).unwrap())
            .collect();
        format!("tmp_obj_{}", name)
    }
    
    /// Helper method to calculate hash for raw data (useful for status command)
    pub fn hash_file_data(&self, data: &[u8]) -> String {
        let header = format!("blob {}\0", data.len());
        let mut full_content = header.as_bytes().to_vec();
        full_content.extend(data);
        
        self.hash_content(&full_content)
    }
}