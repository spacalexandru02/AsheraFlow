use std::fs;
use std::path::{Path, PathBuf};
use globset::{Glob, GlobSet, GlobSetBuilder};
use crate::errors::error::Error;

pub struct Workspace {
    root_path: PathBuf,
}

impl Workspace {
    pub fn new(root_path: &Path) -> Self {
        Workspace {
            root_path: root_path.to_path_buf(),
        }
    }

    // Încarcă regulile din .ashignore
    fn load_ignore_rules(&self) -> Result<GlobSet, Error> {
        let ignore_path = self.root_path.join(".ashignore");
        let mut builder = GlobSetBuilder::new();
    
        if ignore_path.exists() {
            let content = fs::read_to_string(&ignore_path)?;
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let glob = Glob::new(line)?; // Folosește `?` pentru a converti automat
                builder.add(glob);
            }
        }
    
        Ok(builder.build()?) // Conversie automată a `globset::Error` în `Error`
    }
    // Listare recursivă cu ignorare
    pub fn list_files(&self) -> Result<Vec<PathBuf>, Error> {
        let ignore_set = self.load_ignore_rules()?;
        let mut files = Vec::new();
        self.list_files_recursive(&self.root_path, &mut files, &ignore_set)?;
        Ok(files)
    }

    fn list_files_recursive(
        &self,
        path: &Path,
        files: &mut Vec<PathBuf>,
        ignore_set: &GlobSet,
    ) -> Result<(), Error> {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let entry_path = entry.path();
            let relative_path = entry_path.strip_prefix(&self.root_path)?;
    
            // Ignoră explicit directorul `.ash` și conținutul său
            if relative_path.starts_with(".ash") {
                continue;
            }
    
            // Verifică regulile din .ashignore
            if !ignore_set.matches(relative_path).is_empty() {
                continue;
            }
    
            if entry_path.is_dir() {
                self.list_files_recursive(&entry_path, files, ignore_set)?;
            } else {
                println!("Including file: {:?}", relative_path);
                files.push(relative_path.to_path_buf());
            }
        }
        Ok(())
    }

    pub fn read_file(&self, path: &Path) -> Result<Vec<u8>, Error> {
        let file_path = self.root_path.join(path);
        fs::read(&file_path).map_err(Into::into)
    }
}