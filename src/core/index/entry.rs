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
pub oid: String,
flags: u16,
pub path: String,
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
pub fn parent_directories(&self) -> Vec<PathBuf> {
    let path = PathBuf::from(&self.path);
    let mut dirs = Vec::new();
    
    let mut current = path.clone();
    while let Some(parent) = current.parent() {
        if !parent.as_os_str().is_empty() {
            dirs.push(parent.to_path_buf());
        }
        current = parent.to_path_buf();
    }
    
    // Reverse to get them in ascending order
    dirs.reverse();
    dirs
}

pub fn basename(&self) -> String {
    let path = PathBuf::from(&self.path);
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default()
}

pub fn mode_octal(&self) -> String {
    format!("{:o}", self.mode)
}    pub fn parse(data: &[u8]) -> Result<Self, crate::errors::error::Error> {
    if data.len() < 62 {  // Minimum size without path
        return Err(crate::errors::error::Error::Generic("Entry data too short".to_string()));
    }
    
    // Parse all the fixed-size fields
    let ctime = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    let ctime_nsec = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let mtime = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    let mtime_nsec = u32::from_be_bytes([data[12], data[13], data[14], data[15]]);
    let dev = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
    let ino = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
    let mode = u32::from_be_bytes([data[24], data[25], data[26], data[27]]);
    let uid = u32::from_be_bytes([data[28], data[29], data[30], data[31]]);
    let gid = u32::from_be_bytes([data[32], data[33], data[34], data[35]]);
    let size = u32::from_be_bytes([data[36], data[37], data[38], data[39]]);
    
    // Object ID is 20 bytes (40 hex chars)
    let oid = hex::encode(&data[40..60]);
    
    // Flags are 2 bytes
    let flags = u16::from_be_bytes([data[60], data[61]]);
    
    // Path starts at byte 62 and continues until null byte
    let mut path_end = 62;
    while path_end < data.len() && data[path_end] != 0 {
        path_end += 1;
    }
    
    if path_end == data.len() {
        return Err(crate::errors::error::Error::Generic("No null terminator for path".to_string()));
    }
    
    let path = match std::str::from_utf8(&data[62..path_end]) {
        Ok(s) => s.to_string(),
        Err(_) => return Err(crate::errors::error::Error::Generic("Invalid UTF-8 in path".to_string())),
    };
    
    Ok(Entry {
        ctime,
        ctime_nsec,
        mtime,
        mtime_nsec,
        dev,
        ino,
        mode,
        uid,
        gid,
        size,
        oid,
        flags,
        path,
    })
}    pub fn get_path(&self) -> &str {
    &self.path
}
}