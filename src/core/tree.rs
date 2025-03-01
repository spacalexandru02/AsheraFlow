use crate::core::entry::Entry;
use super::database::GitObject;
use crate::errors::error::Error;
use itertools::Itertools;

#[derive(Debug)]
pub struct Tree {
    oid: Option<String>,
    entries: Vec<Entry>,
}

impl GitObject for Tree {
    fn get_type(&self) -> &str {
        "tree"
    }

    fn to_bytes(&self) -> Vec<u8> {
        self.entries
            .iter()
            .sorted_by_key(|e| e.get_name())
            .flat_map(|entry| {
                format!("{} {}\0{}", entry.get_mode(), entry.get_name(), entry.get_oid()).into_bytes()
            })
            .collect()
    }

    fn set_oid(&mut self, oid: String) {
        self.oid = Some(oid);
    }
}

impl Tree {
    pub fn new(entries: Vec<Entry>) -> Result<Self, Error> {
        let mut tree = Tree { oid: None, entries };
        tree.sort_entries();
        Ok(tree)
    }

    fn sort_entries(&mut self) {
        self.entries.sort_by(|a, b| a.get_name().cmp(b.get_name()));
    }


    pub fn get_oid(&self) -> Option<&String> {
        self.oid.as_ref()
    }

}