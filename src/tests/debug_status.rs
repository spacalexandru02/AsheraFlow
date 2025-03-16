use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

struct TestRepo {
    dir: TempDir,
    path: PathBuf,
}

impl TestRepo {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let path = dir.path().to_path_buf();
        
        // Inițializează structura .ash
        fs::create_dir_all(path.join(".ash/objects")).unwrap();
        fs::write(path.join(".ash/HEAD"), "ref: refs/heads/main").unwrap();
        File::create(path.join(".ash/index")).unwrap();

        TestRepo { dir, path }
    }

    fn run_command(&self, command: &str, args: &[&str]) {
        let output = Command::new("cargo")
            .args(&["run", "--", command])
            .args(args)
            .current_dir(&self.path)
            .output()
            .expect("Failed to execute command");

        assert!(
            output.status.success(),
            "Command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn read_head(&self) -> String {
        let head_path = self.path.join(".ash/HEAD");
        let mut head_content = String::new();
        File::open(head_path)
            .unwrap()
            .read_to_string(&mut head_content)
            .unwrap();
        head_content.trim().to_string()
    }

    fn get_index_entries(&self) -> Vec<String> {
        let index_path = self.path.join(".ash/index");
        let mut index_content = String::new();
        File::open(index_path)
            .unwrap()
            .read_to_string(&mut index_content)
            .unwrap();
        index_content.lines().map(|s| s.to_string()).collect()
    }

    fn get_objects(&self) -> Vec<PathBuf> {
        walkdir::WalkDir::new(self.path.join(".ash/objects"))
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.path().to_path_buf())
            .collect()
    }
}

#[test]
fn test_add_command_basic() {
    let repo = TestRepo::new();
    
    // Creează fișier și director
    fs::write(repo.path.join("test.txt"), "content").unwrap();
    fs::create_dir_all(repo.path.join("src/tests")).unwrap();
    fs::write(repo.path.join("src/tests/mod.rs"), "test content").unwrap();

    // Rulează comanda add
    repo.run_command("add", &["test.txt", "src/tests"]);

    // Verifică index
    let entries = repo.get_index_entries();
    assert!(entries.contains(&"test.txt".to_string()));
    assert!(entries.contains(&"src/tests/mod.rs".to_string()));
}

#[test]
fn test_commit_command_basic() {
    let repo = TestRepo::new();
    
    // Setup initial
    fs::write(repo.path.join("file.txt"), "content").unwrap();
    repo.run_command("add", &["file.txt"]);
    
    // Comite
    repo.run_command("commit", &["--message", "Initial commit"]);
    
    // Verifică HEAD
    let head_content = repo.read_head();
    assert!(head_content.starts_with("ref: refs/heads/main"));
    
    // Verifică existența obiectelor
    let objects = repo.get_objects();
    assert_eq!(objects.len(), 2); // Un obiect pentru conținut și un commit
}

#[test]
fn test_commit_message_format() {
    let repo = TestRepo::new();
    
    fs::write(repo.path.join("test.txt"), "content").unwrap();
    repo.run_command("add", &["test.txt"]);
    repo.run_command("commit", &["--message", "i"]);
    
    // Verifică mesajul commit-ului în obiect
    let objects = repo.get_objects();
    let commit_object = objects.iter().find(|p| p.to_str().unwrap().contains("commit")).unwrap();
    
    let mut commit_content = String::new();
    File::open(commit_object)
        .unwrap()
        .read_to_string(&mut commit_content)
        .unwrap();
    
    assert!(commit_content.contains("i"));
}

// Adaugă în Cargo.toml:
// [dependencies]
// walkdir = "2.3.2"
// tempfile = "3.3.0"
