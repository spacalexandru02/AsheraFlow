use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use sha1::{Digest, Sha1};
use flate2::write::ZlibEncoder;
use flate2::Compression;
use crate::errors::error::Error;

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

}