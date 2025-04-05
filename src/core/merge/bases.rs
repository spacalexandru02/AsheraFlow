use std::collections::HashSet;

use crate::core::database::database::Database;
use crate::errors::error::Error;
use crate::core::merge::common_ancestors::{CommonAncestors, Flag};

pub struct Bases<'a> {
    database: &'a mut Database,
    common: CommonAncestors<'a>,
    commits: Vec<String>,
    redundant: HashSet<String>,
}

impl<'a> Bases<'a> {
    pub fn new(database: &'a mut Database, one: &str, two: &str) -> Result<Self, Error> {
        let common = CommonAncestors::new(database, one, &[two])?;
        
        Ok(Self {
            database,
            common,
            commits: Vec::new(),
            redundant: HashSet::new(),
        })
    }

    pub fn find(&mut self) -> Result<Vec<String>, Error> {
        // Find all common ancestors
        self.commits = self.common.find()?;
        
        // If we have 0 or 1 common ancestors, return immediately
        if self.commits.len() <= 1 {
            return Ok(self.commits.clone());
        }

        // Filter out redundant common ancestors
        self.redundant = HashSet::new();

        // We need to process each commit and identify which ones are redundant
        let commits = self.commits.clone();
        for commit in commits {
            self.filter_commit(&commit)?;
        }

        // Return only non-redundant common ancestors
        Ok(self.commits.iter()
            .filter(|commit| !self.redundant.contains(*commit))
            .cloned()
            .collect())
    }

    fn filter_commit(&mut self, commit: &str) -> Result<(), Error> {
        // Skip if already marked redundant
        if self.redundant.contains(commit) {
            return Ok(());
        }

        // Get a list of all other common ancestors to compare against
        let others: Vec<_> = self.commits.iter()
            .filter(|oid| *oid != commit && !self.redundant.contains(*oid))
            .map(|oid| oid.as_str())
            .collect();
        
        // If no other commits to compare against, exit early
        if others.is_empty() {
            return Ok(());
        }

        // Find common ancestors between this commit and all others
        let mut common = CommonAncestors::new(self.database, commit, &others)?;
        common.find()?;

        // If this commit is an ancestor of any other commits, it's redundant
        if common.is_marked(commit, &Flag::Parent2) {
            self.redundant.insert(commit.to_string());
        }

        // If any other commits are ancestors of this one, they're redundant
        for oid in others {
            if common.is_marked(oid, &Flag::Parent1) {
                self.redundant.insert(oid.to_string());
            }
        }

        Ok(())
    }
}