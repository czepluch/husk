use anyhow::{Context, Result};
use chrono::Local;
use husk::config::Config;
use husk::store::VdirStore;
use husk::theme::Theme;
use husk::ui::{self, app::App};

fn main() -> Result<()> {
    let config = Config::load()?;
    let theme = Theme::load(&config.theme, Config::theme_file().as_deref())?;
    let store = VdirStore::new(&config.vdir);
    let vdir = config.vdir.clone();
    let app = App::new(config, Box::new(store), Local::now())
        .with_context(|| format!("reading {}", vdir.display()))?;
    ui::run(app, &theme)
}
