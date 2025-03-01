use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const REGULAR_MODE: u32 = 0o100644;
const EXECUTABLE_MODE: u32 = 0o100755;
const MAX_PATH_SIZE: u16 = 0xfff;

#[derive(Debug, Clone)]
pub struct Entry {
    ctime: u32,
    ctime_nsec: u32,
    mtime: u32,
    mtime_nsec: u32,
    dev: u32,
    ino: u32,
    mode: u32,
    uid: u32,
    gid: u32,
    size: u32,
    oid: String,
    flags: u16,
    path: String,
}

impl Entry {
    pub fn create(pathname: &Path, oid: &str, stat: &fs::Metadata) -> Self {
        let path = pathname.to_string_lossy().to_string();
        
        // Determine if file is executable (mode 755) or regular (mode 644)
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            if stat.permissions().mode() & 0o111 != 0 {
                EXECUTABLE_MODE
            } else {
                REGULAR_MODE
            }
        };
        
        #[cfg(not(unix))]
        let mode = REGULAR_MODE;
        
        let flags = path.len().min(MAX_PATH_SIZE as usize) as u16;
        
        // Get ctime and mtime
        let ctime = stat.created().unwrap_or(SystemTime::now());
        let mtime = stat.modified().unwrap_or(SystemTime::now());
        
        let ctime_duration = ctime.duration_since(UNIX_EPOCH).unwrap_or_default();
        let mtime_duration = mtime.duration_since(UNIX_EPOCH).unwrap_or_default();
        
        Entry {
            ctime: ctime_duration.as_secs() as u32,
            ctime_nsec: ctime_duration.subsec_nanos(),
            mtime: mtime_duration.as_secs() as u32,
            mtime_nsec: mtime_duration.subsec_nanos(),
            dev: 0,  // These might not be directly available in Rust's fs::Metadata
            ino: 0,  // We could use libc to get these on Unix systems if needed
            mode,
            uid: 0,  // Same here, might need platform-specific code
            gid: 0,  // Same here, might need platform-specific code
            size: stat.len() as u32,
            oid: oid.to_string(),
            flags,
            path,
        }
    }
    
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut result = Vec::new();
        
        // Pack all the fixed-size fields
        result.extend_from_slice(&self.ctime.to_be_bytes());
        result.extend_from_slice(&self.ctime_nsec.to_be_bytes());
        result.extend_from_slice(&self.mtime.to_be_bytes());
        result.extend_from_slice(&self.mtime_nsec.to_be_bytes());
        result.extend_from_slice(&self.dev.to_be_bytes());
        result.extend_from_slice(&self.ino.to_be_bytes());
        result.extend_from_slice(&self.mode.to_be_bytes());
        result.extend_from_slice(&self.uid.to_be_bytes());
        result.extend_from_slice(&self.gid.to_be_bytes());
        result.extend_from_slice(&self.size.to_be_bytes());
        
        // Add OID (assuming hex format, 40 chars)
        result.extend_from_slice(self.oid.as_bytes());
        
        // Add flags
        result.extend_from_slice(&self.flags.to_be_bytes());
        
        // Add path
        result.extend_from_slice(self.path.as_bytes());
        result.push(0); // Null terminator
        
        // Pad to 8-byte boundary
        while result.len() % 8 != 0 {
            result.push(0);
        }
        
        result
    }
}