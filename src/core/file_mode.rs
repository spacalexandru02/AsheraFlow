pub struct FileMode;

impl FileMode {
    /// Mod pentru directoare
    pub const DIRECTORY: u32 = 0o040000;
    
    /// Mod pentru fișiere normale
    pub const REGULAR: u32 = 0o100644;
    
    /// Mod pentru fișiere executabile
    pub const EXECUTABLE: u32 = 0o100755;
    
    /// Mod pentru symlink-uri
    pub const SYMLINK: u32 = 0o120000;
    
    /// Convertește un șir de caractere la un mod numeric
    /// Acceptă atât reprezentări octale cât și zecimale
    pub fn parse(mode_str: &str) -> u32 {
        let trimmed = mode_str.trim();
        
        // Încercăm mai întâi ca număr octal
        if trimmed.starts_with("0") || trimmed.starts_with("0o") {
            let start_idx = if trimmed.starts_with("0o") { 2 } else { 1 };
            if let Ok(mode) = u32::from_str_radix(&trimmed[start_idx..], 8) {
                return mode;
            }
        }
        
        // Încercăm valori octale fără prefix (cum ar fi "100644")
        if trimmed.len() >= 6 && trimmed.starts_with('1') {
            if let Ok(mode) = u32::from_str_radix(trimmed, 8) {
                return mode;
            }
        }
        
        // În final, încercăm ca număr zecimal
        trimmed.parse::<u32>().unwrap_or(Self::REGULAR)
    }
    
    /// Convertește un mod numeric la reprezentarea sa octală
    pub fn to_octal_string(mode: u32) -> String {
        format!("{:o}", mode)
    }
    
    /// Verifică dacă două moduri sunt echivalente, indiferent de reprezentare
    pub fn are_equivalent(mode1: u32, mode2: u32) -> bool {
        // Comparăm doar biții relevanți (permisiunile și tipul)
        // În mod normal, biții 12-15 (tipul) și 0-8 (permisiunile) sunt cei relevanți
        let mask = 0o170000 | 0o777; // Combină masca pentru tip și permisiuni
        (mode1 & mask) == (mode2 & mask)
    }
    
    /// Verifică dacă un mod corespunde unui director
    pub fn is_directory(mode: u32) -> bool {
        (mode & 0o170000) == Self::DIRECTORY
    }
    
    /// Verifică dacă un mod corespunde unui fișier executabil
    pub fn is_executable(mode: u32) -> bool {
        (mode & 0o111) != 0
    }
    
    /// Determină modul corespunzător din metadatele unui fișier
    pub fn from_metadata(metadata: &std::fs::Metadata) -> u32 {
        if metadata.is_dir() {
            return Self::DIRECTORY;
        }
        
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 != 0 {
                return Self::EXECUTABLE;
            }
        }
        
        Self::REGULAR
    }
}
