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
    /// Writes the task's changes and updates `task` to match the file, so it
    /// can be edited and saved again. A task that was not changed is not
    /// written.
    fn save(&self, task: &mut Task) -> Result<()>;
    fn delete(&self, uid: &str) -> Result<()>;
    /// Writes a task back from memory, for undo: after a delete the file is
    /// recreated, after an edit it is overwritten. `SEQUENCE` ends up above
    /// whatever the file held, so phone clients accept the older content.
    fn restore(&self, task: &Task) -> Result<Task>;
    fn move_to(&self, uid: &str, project: &ProjectId) -> Result<()>;
}
