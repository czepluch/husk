//! Storage behind one trait, so the UI and CLI never touch the filesystem
//! and a CalDAV backend can replace the vdir one later.

pub mod vdir;

pub use vdir::VdirStore;

use anyhow::Result;

use crate::model::{NewTask, Project, ProjectId, Task};

pub trait Store {
    fn projects(&self) -> Result<Vec<Project>>;
    /// Tasks of one project, or of all projects when `project` is `None`.
    fn tasks(&self, project: Option<&ProjectId>) -> Result<Vec<Task>>;
    fn get(&self, uid: &str) -> Result<Task>;
    fn create(&self, project: &ProjectId, task: NewTask) -> Result<Task>;
    /// Writes the task's changes. A task that was not changed is not written.
    fn save(&self, task: &Task) -> Result<()>;
    fn delete(&self, uid: &str) -> Result<()>;
    fn move_to(&self, uid: &str, project: &ProjectId) -> Result<()>;
}
