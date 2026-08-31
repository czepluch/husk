//! The vdir store: one directory per project, one `.ics` file per task.
//!
//! Writes go to a temp file in the same directory, are fsynced, then renamed
//! over the target, so vdirsyncer never sees a partial file. Every rewrite
//! bumps `SEQUENCE`, `LAST-MODIFIED` and `DTSTAMP`, which is how the phone
//! clients decide which side wins.

use std::fs::{self, File};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::ical::{codec, vtodo};
use crate::model::{NewTask, Project, ProjectId, Task};
use crate::store::Store;

pub struct VdirStore {
    root: PathBuf,
    clock: fn() -> DateTime<Utc>,
}

impl VdirStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            clock: Utc::now,
        }
    }

    /// Replaces the time source, so tests get predictable timestamps.
    pub fn with_clock(mut self, clock: fn() -> DateTime<Utc>) -> Self {
        self.clock = clock;
        self
    }

    fn project_dir(&self, id: &ProjectId) -> PathBuf {
        self.root.join(id.as_str())
    }

    fn project_ids(&self) -> Result<Vec<ProjectId>> {
        let entries = fs::read_dir(&self.root)
            .with_context(|| format!("vdir {} not readable", self.root.display()))?;
        let mut ids: Vec<ProjectId> = entries
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|name| !name.starts_with('.'))
            .map(ProjectId::new)
            .collect();
        ids.sort();
        Ok(ids)
    }

    fn ics_files(&self, id: &ProjectId) -> Result<Vec<PathBuf>> {
        let dir = self.project_dir(id);
        let entries =
            fs::read_dir(&dir).with_context(|| format!("unknown project {}", id.as_str()))?;
        let mut files: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "ics"))
            .collect();
        files.sort();
        Ok(files)
    }

    fn read_task(&self, id: &ProjectId, path: &Path) -> Result<Task> {
        let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        vtodo::parse_task(&text, id.clone()).with_context(|| format!("parse {}", path.display()))
    }

    /// Finds and reads the task holding a UID. `<uid>.ics` is tried first;
    /// file names are not guaranteed to match, so the fallback reads every file.
    fn locate(&self, uid: &str) -> Result<(PathBuf, Task)> {
        let ids = self.project_ids()?;
        for id in &ids {
            let direct = self.project_dir(id).join(format!("{uid}.ics"));
            if direct.is_file()
                && let Ok(task) = self.read_task(id, &direct)
                && task.uid == uid
            {
                return Ok((direct, task));
            }
        }
        for id in &ids {
            for path in self.ics_files(id)? {
                if let Ok(task) = self.read_task(id, &path)
                    && task.uid == uid
                {
                    return Ok((path, task));
                }
            }
        }
        bail!("no task with UID {uid}")
    }

    fn holds_uid(&self, id: &ProjectId, uid: &str) -> Result<bool> {
        Ok(self
            .ics_files(id)?
            .iter()
            .any(|path| self.read_task(id, path).is_ok_and(|t| t.uid == uid)))
    }
}

impl Store for VdirStore {
    fn projects(&self) -> Result<Vec<Project>> {
        self.project_ids()?
            .into_iter()
            .map(|id| {
                let dir = self.project_dir(&id);
                let name = read_trimmed(&dir.join("displayname"))
                    .unwrap_or_else(|| id.as_str().to_string());
                let color = read_trimmed(&dir.join("color"));
                Ok(Project { id, name, color })
            })
            .collect()
    }

    fn tasks(&self, project: Option<&ProjectId>) -> Result<Vec<Task>> {
        let ids = match project {
            Some(id) => vec![id.clone()],
            None => self.project_ids()?,
        };
        let mut tasks = Vec::new();
        for id in &ids {
            for path in self.ics_files(id)? {
                // A file another client wrote badly must not take the whole list down.
                if let Ok(task) = self.read_task(id, &path) {
                    tasks.push(task);
                }
            }
        }
        Ok(tasks)
    }

    fn get(&self, uid: &str) -> Result<Task> {
        self.locate(uid).map(|(_, task)| task)
    }

    fn create(&self, project: &ProjectId, task: NewTask) -> Result<Task> {
        let dir = self.project_dir(project);
        if !dir.is_dir() {
            bail!("unknown project {}", project.as_str());
        }
        let uid = Uuid::new_v4().to_string();
        let path = dir.join(format!("{uid}.ics"));
        if path.exists() {
            bail!("{} already exists", path.display());
        }
        let doc = vtodo::new_document(&task, &uid, (self.clock)());
        write_atomic(&path, &codec::serialize(&doc))?;
        vtodo::from_document(doc, project.clone())
    }

    fn save(&self, task: &mut Task) -> Result<()> {
        let mut doc = vtodo::apply(task)?;
        if doc == task.raw {
            return Ok(());
        }
        let (path, current) = self.locate(&task.uid)?;
        // Compared as parsed documents: the file may be laid out differently
        // (fold width, line endings, trailing newline) and still say the same.
        if current.raw != task.raw {
            bail!(
                "task {} changed on disk since it was read; reload and retry",
                task.uid
            );
        }
        vtodo::bump(&mut doc, (self.clock)())?;
        write_atomic(&path, &codec::serialize(&doc))?;
        task.raw = doc;
        Ok(())
    }

    fn delete(&self, uid: &str) -> Result<()> {
        let (path, _) = self.locate(uid)?;
        fs::remove_file(&path).with_context(|| format!("delete {}", path.display()))
    }

    /// Directory mtimes: every file vdirsyncer or husk writes lands by
    /// rename, which touches the directory, so this moves on every change.
    fn stamp(&self) -> Result<u64> {
        let mut hasher = DefaultHasher::new();
        for id in self.project_ids()? {
            let dir = self.project_dir(&id);
            let modified = fs::metadata(&dir).and_then(|m| m.modified())?;
            id.hash(&mut hasher);
            modified.hash(&mut hasher);
        }
        Ok(hasher.finish())
    }

    fn restore(&self, task: &Task) -> Result<Task> {
        let dir = self.project_dir(&task.project);
        if !dir.is_dir() {
            bail!("unknown project {}", task.project.as_str());
        }
        let (path, on_disk) = match self.locate(&task.uid) {
            Ok((path, current)) => {
                if current.project != task.project {
                    bail!(
                        "task {} has moved to project {}",
                        task.uid,
                        current.project.as_str()
                    );
                }
                (path, vtodo::sequence(&current.raw))
            }
            Err(_) => (dir.join(format!("{}.ics", task.uid)), 0),
        };
        let mut doc = task.raw.clone();
        let base = on_disk.max(vtodo::sequence(&doc));
        vtodo::set_sequence(&mut doc, base)?;
        vtodo::bump(&mut doc, (self.clock)())?;
        write_atomic(&path, &codec::serialize(&doc))?;
        vtodo::from_document(doc, task.project.clone())
    }

    fn move_to(&self, uid: &str, project: &ProjectId) -> Result<()> {
        let (path, task) = self.locate(uid)?;
        if &task.project == project {
            return Ok(());
        }
        let dir = self.project_dir(project);
        if !dir.is_dir() {
            bail!("unknown project {}", project.as_str());
        }
        // vdirsyncer refuses to run when two files claim one UID.
        if self.holds_uid(project, uid)? {
            bail!(
                "project {} already holds a task with UID {uid}",
                project.as_str()
            );
        }
        let target = dir.join(path.file_name().context("file without a name")?);
        if target.exists() {
            bail!("{} already exists", target.display());
        }
        let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        // The old file goes only once the new one is safely on disk, so the
        // UID is never lost and never present twice for longer than a rename.
        write_atomic(&target, &text)?;
        fs::remove_file(&path).with_context(|| format!("delete {}", path.display()))
    }
}

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn write_atomic(path: &Path, text: &str) -> Result<()> {
    let dir = path.parent().context("path without a directory")?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .context("path without a file name")?;
    let tmp = dir.join(format!("{name}.tmp"));
    let written = (|| -> Result<()> {
        let mut file = File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        fs::rename(&tmp, path)
            .with_context(|| format!("rename {} to {}", tmp.display(), path.display()))
    })();
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written?;
    // The rename itself has to reach disk before the write counts as done.
    File::open(dir)?.sync_all()?;
    Ok(())
}
