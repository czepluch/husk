use anyhow::{Context, Result};
use chrono::Local;
use husk::config::Config;
use husk::store::{Store, VdirStore};
use husk::theme::Theme;
use husk::ui::{self, app::App};

fn main() -> Result<()> {
    let config = Config::load()?;
    let theme = Theme::load(&config.theme, Config::theme_file().as_deref())?;
    let store = VdirStore::new(&config.vdir);
    let projects = store
        .projects()
        .with_context(|| format!("reading projects from {}", config.vdir.display()))?;
    let tasks = store.tasks(None)?;
    let app = App::new(config, projects, tasks, Local::now());
    ui::run(app, &theme)
}
