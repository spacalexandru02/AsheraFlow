use std::env;
use std::fs;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use regex::Regex;
use crate::errors::error::Error;

pub struct Editor {
    path: PathBuf,
    command: String,
    closed: bool,
    file: Option<File>,
}

impl Editor {
    pub const DEFAULT_EDITOR: &'static str = "vi";
    
    pub fn new(path: PathBuf, command: Option<String>) -> Result<Self, Error> {
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .map_err(|e| Error::IO(e))?;
            
        Ok(Self {
            path,
            command: command.unwrap_or_else(|| Self::DEFAULT_EDITOR.to_string()),
            closed: false,
            file: Some(file),
        })
    }
    
    pub fn edit<F>(path: PathBuf, command: Option<String>, f: F) -> Result<Option<String>, Error>
    where
        F: FnOnce(&mut Editor) -> Result<(), Error>,
    {
        let mut editor = Editor::new(path, command)?;
        f(&mut editor)?;
        editor.edit_file()
    }
    
    pub fn puts(&mut self, string: &str) -> Result<(), Error> {
        if self.closed {
            return Ok(());
        }
        
        if let Some(file) = &mut self.file {
            file.write_all(string.as_bytes()).map_err(|e| Error::IO(e))?;
            file.write_all(b"\n").map_err(|e| Error::IO(e))?;
        }
        
        Ok(())
    }
    
    pub fn note(&mut self, string: &str) -> Result<(), Error> {
        if self.closed {
            return Ok(());
        }
        
        if let Some(file) = &mut self.file {
            for line in string.lines() {
                writeln!(file, "# {}", line).map_err(|e| Error::IO(e))?;
            }
        }
        
        Ok(())
    }
    
    pub fn close(&mut self) {
        self.closed = true;
    }
    
    pub fn edit_file(&mut self) -> Result<Option<String>, Error> {
        // Close the file handle before launching editor
        self.file.take();
        
        // Don't launch editor if closed flag is set
        if self.closed {
            return self.read_result();
        }
        
        // Parse command into arguments using shlex for better handling of quotes
        let args = match shlex::split(&self.command) {
            Some(args) => args,
            None => return Err(Error::Generic("Invalid editor command".to_string())),
        };
        
        if args.is_empty() {
            return Err(Error::Generic("Empty editor command".to_string()));
        }
        
        // Extract command and arguments
        let cmd = &args[0];
        let mut cmd_args = args[1..].to_vec();
        
        // Add file path to arguments
        cmd_args.push(self.path.to_string_lossy().to_string());
        
        // Execute editor
        let status = Command::new(cmd)
            .args(cmd_args)
            .status()
            .map_err(|e| Error::Generic(format!("Failed to execute editor '{}': {}", self.command, e)))?;
        
        if !status.success() {
            return Err(Error::Generic(format!("Editor '{}' exited with non-zero status", self.command)));
        }
        
        self.read_result()
    }
    
    fn read_result(&self) -> Result<Option<String>, Error> {
        let content = fs::read_to_string(&self.path)
            .map_err(|e| Error::IO(e))?;
        
        Ok(self.remove_notes(&content))
    }
    
    fn remove_notes(&self, content: &str) -> Option<String> {
        // Filter out comments
        let lines: Vec<&str> = content.lines()
            .filter(|line| !line.starts_with('#'))
            .collect();
        
        // Check if all remaining lines are whitespace
        let re = Regex::new(r"^\s*$").unwrap();
        if lines.iter().all(|line| re.is_match(line)) {
            None
        } else {
            Some(format!("{}\n", lines.join("\n").trim()))
        }
    }
    
    pub fn get_editor_command() -> String {
        env::var("GIT_EDITOR")
            .or_else(|_| env::var("VISUAL"))
            .or_else(|_| env::var("EDITOR"))
            .unwrap_or_else(|_| Self::DEFAULT_EDITOR.to_string())
    }
}