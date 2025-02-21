#[derive(Debug, Clone)]
pub struct Entry {
    name: String,
    oid: String,
}

impl Entry {
    pub fn new(name: String, oid: String) -> Self {
        Entry { name, oid }
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn get_oid(&self) -> &str {
        &self.oid
    }
}
