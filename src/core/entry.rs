#[derive(Debug, Clone)]
pub struct Entry {
    name: String,
    oid: String,
    mode: String,
}

impl Entry {
    pub fn new(name: String, oid: String, mode: &str) -> Self {
        Entry {
            name,
            oid,
            mode: mode.to_string(),
        }
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn get_oid(&self) -> &str {
        &self.oid
    }
    pub fn get_mode(&self) -> &str {
        &self.mode
    }
}
