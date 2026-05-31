use std::path::Path;
use uuid::Uuid;

use crate::isolation::{Isolation, InPlaceBranchIsolation};
use crate::isolation::in_place::find_repo_root;

pub struct RunContext {
    pub run_id: String,
    pub task: String,
    isolation: Box<dyn Isolation>,
}

impl RunContext {
    pub fn new(task: String, repo: &Path) -> anyhow::Result<Self> {
        let run_id = Uuid::new_v4().to_string();
        let repo_root = find_repo_root(repo)?;
        let isolation = InPlaceBranchIsolation::create(&repo_root, &run_id, &task)?;

        Ok(Self {
            run_id,
            task,
            isolation: Box::new(isolation),
        })
    }

    pub fn working_path(&self) -> &Path {
        self.isolation.working_path()
    }

    pub fn branch(&self) -> &str {
        self.isolation.branch()
    }
}
