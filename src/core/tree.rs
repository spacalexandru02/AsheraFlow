use crate::core::entry::Entry;

use super::database::GitObject;

pub struct Tree {
    oid: Option<String>,
    entries: Vec<Entry>,
}

impl GitObject for Tree {
    fn get_type(&self) -> &str {
        "tree"
    }

    fn to_bytes(&self) -> Vec<u8> {
        self.to_bytes()
    }

    fn set_oid(&mut self, oid: String) {
        self.oid = Some(oid);
    }
}

impl Tree {
    const MODE: &'static str = "100644";

    pub fn new(entries: Vec<Entry>) -> Self {
        Tree { oid: None, entries }
    }

    pub fn set_oid(&mut self, oid: String) {
        self.oid = Some(oid);
    }

    pub fn get_oid(&self) -> Option<&String> {
        self.oid.as_ref()
    }

    pub fn get_type(&self) -> &str {
        "tree"
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut entries = self.entries.clone();
        entries.sort_by(|a, b| a.get_name().cmp(b.get_name()));

        let mut result = Vec::new();
        for entry in entries {
            let line = format!("{} {}\0{}", Self::MODE, entry.get_name(), entry.get_oid());
            result.extend_from_slice(line.as_bytes());
        }
        result
    }
}