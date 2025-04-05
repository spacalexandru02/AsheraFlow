use std::collections::HashSet;

use crate::core::database::database::Database;
use crate::errors::error::Error;
use crate::core::merge::common_ancestors::CommonAncestors;

pub struct Bases<'a> {
    database: &'a mut Database,
    commits: Vec<String>,
    redundant: HashSet<String>,
}

impl<'a> Bases<'a> {
    pub fn new(database: &'a mut Database, one: &str, two: &str) -> Result<Self, Error> {
        // Initialize with empty commits list - we'll populate it in find()
        Ok(Self {
            database,
            commits: Vec::new(),
            redundant: HashSet::new(),
        })
    }

    pub fn find(&mut self) -> Result<Vec<String>, Error> {
        // Create a new CommonAncestors instance and find common ancestors
        let mut common = CommonAncestors::new(self.database, one, &[two])?;
        self.commits = common.find()?;
        
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
        let common_results = common.find()?;

        // If this commit is an ancestor of any other commits, it's redundant
        for oid in others {
            if common_results.contains(&oid.to_string()) {
                self.redundant.insert(commit.to_string());
                break;
            }
        }

        // If any other commits are ancestors of this one, they're redundant
        for oid in others {
            if common_results.contains(&commit.to_string()) {
                self.redundant.insert(oid.to_string());
            }
        }

        Ok(())
    }
}