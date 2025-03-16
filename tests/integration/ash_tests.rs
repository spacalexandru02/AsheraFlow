use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

struct TestRepo {
    dir: TempDir,
    path: PathBuf,
    bin_path: PathBuf,
}

impl TestRepo {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let path = dir.path().to_path_buf();
        
        // Find the path to the executable
        let bin_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("debug")
            .join("AsheraFlow");
        
        // Initialize the repository
        let output = Command::new(&bin_path)
            .arg("init")
            .arg(path.to_str().unwrap())
            .output()
            .expect("Failed to execute init command");
            
        if !output.status.success() {
            panic!("Failed to initialize repository: {}", 
                   String::from_utf8_lossy(&output.stderr));
        }

        TestRepo { dir, path, bin_path }
    }

    fn run_command(&self, command: &str, args: &[&str]) -> String {
        let mut full_args = vec![command];
        full_args.extend_from_slice(args);
        
        let output = Command::new(&self.bin_path)
            .args(&full_args)
            .current_dir(&self.path)
            .output()
            .expect("Failed to execute command");

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        
        println!("Command: {} {:?}", command, args);
        println!("stdout: {}", stdout);
        println!("stderr: {}", stderr);

        assert!(
            output.status.success(),
            "Command failed: {}",
            stderr
        );
        
        stdout
    }
    
    fn read_file(&self, path: &str) -> Option<String> {
        let file_path = self.path.join(path);
        if !file_path.exists() {
            return None;
        }
        
        let mut content = String::new();
        if File::open(file_path).unwrap().read_to_string(&mut content).is_ok() {
            Some(content)
        } else {
            None
        }
    }
    
    fn write_file(&self, path: &str, content: &str) {
        let file_path = self.path.join(path);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(file_path, content).unwrap();
    }
    
    fn dump_file_metadata(&self, path: &str) {
        let file_path = self.path.join(path);
        if !file_path.exists() {
            println!("File doesn't exist: {}", path);
            return;
        }
        
        let metadata = fs::metadata(&file_path).unwrap();
        println!("File metadata for {}:", path);
        println!("  Size: {} bytes", metadata.len());
        println!("  Modified: {:?}", metadata.modified().unwrap());
        println!("  Created: {:?}", metadata.created().unwrap());
        
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            println!("  Mode: {:o}", metadata.mode());
            println!("  UID: {}", metadata.uid());
            println!("  GID: {}", metadata.gid());
        }
    }
    
    fn examine_index(&self) {
        let index_path = self.path.join(".ash/index");
        if !index_path.exists() {
            println!("Index file doesn't exist");
            return;
        }
        
        let metadata = fs::metadata(&index_path).unwrap();
        println!("Index file metadata:");
        println!("  Size: {} bytes", metadata.len());
        println!("  Modified: {:?}", metadata.modified().unwrap());
        
        // Try to print the first few bytes as hex
        if let Ok(mut file) = File::open(&index_path) {
            let mut buffer = [0; 32];
            if let Ok(n) = file.read(&mut buffer) {
                print!("  First {} bytes: ", n);
                for b in &buffer[0..n] {
                    print!("{:02x} ", b);
                }
                println!();
            }
        }
    }
}

// Extract the commit hash from output
fn extract_commit_hash(output: &str) -> Option<String> {
    // Format: [hash] message
    let start = output.find('[');
    let end = output.find(']');
    
    if let (Some(start_idx), Some(end_idx)) = (start, end) {
        if start_idx < end_idx {
            return Some(output[start_idx+1..end_idx].trim().to_string());
        }
    }
    
    // Try finding a 40-character hex string
    for word in output.split_whitespace() {
        if word.len() == 40 && word.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(word.to_string());
        }
    }
    
    None
}

#[test]
fn test_direct_head_updates() {
    let repo = TestRepo::new();
    
    // Create a file
    repo.write_file("test.txt", "test content");
    
    // Check the initial HEAD
    let head_before = repo.read_file(".ash/HEAD");
    println!("HEAD before commit: {:?}", head_before);
    
    // Add and commit
    repo.run_command("add", &["test.txt"]);
    let commit_output = repo.run_command("commit", &["-m", "Initial commit"]);
    
    // Extract the commit hash
    let commit_hash = extract_commit_hash(&commit_output);
    println!("Extracted commit hash: {:?}", commit_hash);
    
    // Check HEAD after commit
    let head_after = repo.read_file(".ash/HEAD");
    println!("HEAD after commit: {:?}", head_after);
    
    // HEAD should contain a non-empty string (the commit hash)
    assert!(head_after.is_some(), "HEAD file doesn't exist after commit");
    assert!(!head_after.unwrap().trim().is_empty(), "HEAD is empty after commit");
}

#[test]
fn test_status_debugging() {
    let repo = TestRepo::new();
    
    // Create and add a file
    repo.write_file("status_test.txt", "test content");
    
    println!("File metadata before add:");
    repo.dump_file_metadata("status_test.txt");
    
    repo.run_command("add", &["status_test.txt"]);
    
    println!("File metadata after add:");
    repo.dump_file_metadata("status_test.txt");
    println!("Index after add:");
    repo.examine_index();
    
    // Commit the file
    repo.run_command("commit", &["-m", "Add status test file"]);
    
    println!("File metadata after commit:");
    repo.dump_file_metadata("status_test.txt");
    println!("Index after commit:");
    repo.examine_index();
    
    // Run status and check output
    let status_output = repo.run_command("status", &[]);
    println!("Status after commit: {}", status_output);
    
    // Run status with porcelain flag
    let porcelain_output = repo.run_command("status", &["--porcelain"]);
    println!("Porcelain status after commit: {}", porcelain_output);
    
    // Write to the file again with the exact same content
    repo.write_file("status_test.txt", "test content");
    
    println!("File metadata after rewrite (same content):");
    repo.dump_file_metadata("status_test.txt");
    
    // Run status again
    let status_after_rewrite = repo.run_command("status", &[]);
    println!("Status after rewrite (same content): {}", status_after_rewrite);
    
    // Run status with porcelain flag again
    let porcelain_after_rewrite = repo.run_command("status", &["--porcelain"]);
    println!("Porcelain status after rewrite (same content): {}", porcelain_after_rewrite);
}

#[test]
fn test_timestamp_issue() {
    let repo = TestRepo::new();
    
    // Create and add a file
    repo.write_file("time_test.txt", "test content");
    repo.run_command("add", &["time_test.txt"]);
    
    // Sleep a bit to ensure timestamps will be different
    std::thread::sleep(std::time::Duration::from_secs(2));
    
    // Commit the file
    repo.run_command("commit", &["-m", "Add time test file"]);
    
    // Run status to see if it's clean
    let status_output = repo.run_command("status", &[]);
    println!("Status after commit: {}", status_output);
    
    // Sleep again to ensure timestamps will be different
    std::thread::sleep(std::time::Duration::from_secs(2));
    
    // Write the exact same content to the file
    // This will update the timestamp but not the content
    repo.write_file("time_test.txt", "test content");
    
    // Run status again
    let status_after_rewrite = repo.run_command("status", &[]);
    println!("Status after rewrite (same content): {}", status_after_rewrite);
    
    // Add the file again (should not change content, but update timestamps in index)
    repo.run_command("add", &["time_test.txt"]);
    
    // Run status one more time
    let status_after_add = repo.run_command("status", &[]);
    println!("Status after re-adding (same content): {}", status_after_add);
    
    // Try another commit
    repo.run_command("commit", &["-m", "Re-add time test file"]);
    
    // Final status check
    let final_status = repo.run_command("status", &[]);
    println!("Final status: {}", final_status);
}

#[test]
fn test_binary_content_comparison() {
    let repo = TestRepo::new();
    
    // Create a text file with varied content
    let content = "Hello\r\nWorld\nThis is a test.\nWith different line endings.\r\n";
    repo.write_file("line_endings.txt", content);
    
    // Add and commit
    repo.run_command("add", &["line_endings.txt"]);
    repo.run_command("commit", &["-m", "Add file with mixed line endings"]);
    
    // Check status
    let status_output = repo.run_command("status", &[]);
    println!("Status after commit: {}", status_output);
    
    // Write the exact same content to the file
    repo.write_file("line_endings.txt", content);
    
    // Check status again
    let status_after_rewrite = repo.run_command("status", &[]);
    println!("Status after rewrite (same content): {}", status_after_rewrite);
}