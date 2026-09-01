use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use chrono::{Local, Utc};
use clap::{Parser, Subcommand, ValueEnum};
use husk::config::Config;
use husk::model::{ProjectId, find_project, project_name};
use husk::notify::State;
use husk::store::{Store, VdirStore};
use husk::sync;
use husk::theme::Theme;
use husk::ui::app::{App, View};
use husk::{cli, ui};

/// CalDAV tasks in the terminal.
#[derive(Parser)]
#[command(name = "husk", version)]
struct Cli {
    /// Config file (default: ~/.config/husk/config.toml)
    #[arg(long, global = true, value_name = "FILE")]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Open the TUI (the default)
    Tui,
    /// Create a task: husk add "Book dentist due:fri 09:00 pri:H +health @life"
    Add {
        /// Quick-add text; words are joined with spaces
        text: Vec<String>,
    },
    /// Print tasks, for scripts and Waybar
    List {
        #[arg(long, value_enum, default_value_t = ViewArg::Today)]
        view: ViewArg,
        /// Only this project (directory or display name)
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Fire desktop notifications for alarms that came due; run it from a timer
    Notify {
        /// Also notify about every overdue task, again
        #[arg(long)]
        nag: bool,
        /// Print what would fire and change nothing
        #[arg(long)]
        dry_run: bool,
        /// State file (default: ~/.local/state/husk/notify.json)
        #[arg(long, value_name = "FILE")]
        state: Option<PathBuf>,
    },
    /// Run the sync command now
    Sync,
}

#[derive(Clone, Copy, ValueEnum)]
enum ViewArg {
    Overdue,
    Today,
    Upcoming,
    All,
}

fn main() -> Result<()> {
    let args = Cli::parse();
    let config = match &args.config {
        Some(path) => Config::from_file(path)?,
        None => Config::load()?,
    };
    let store = VdirStore::new(&config.vdir);
    match args.command.unwrap_or(Command::Tui) {
        Command::Tui => {
            let theme = Theme::load(&config.theme, Config::theme_file().as_deref())?;
            let vdir = config.vdir.clone();
            let app = App::new(config, Box::new(store), Local::now())
                .with_context(|| format!("reading {}", vdir.display()))?;
            ui::run(app, &theme)
        }
        Command::Add { text } => {
            let task = cli::add(&store, &config, &text.join(" "), Local::now())?;
            let project = project_name(&store.projects()?, &task.project);
            println!("Added: {} [{project}]", task.summary);
            // In the foreground, so a keybind can rely on the exit code.
            if !config.sync_command.is_empty()
                && let Err(error) = sync::run(&config.sync_command)
            {
                bail!("task added, but the sync failed: {error}");
            }
            Ok(())
        }
        Command::List {
            view,
            project,
            json,
        } => {
            let projects = store.projects()?;
            let project = match project {
                Some(name) => Some(
                    find_project(&projects, &name)
                        .with_context(|| format!("no project named {name:?}"))?
                        .id
                        .clone(),
                ),
                None => None::<ProjectId>,
            };
            let view = match view {
                ViewArg::Overdue => View::Overdue,
                ViewArg::Today => View::Today,
                ViewArg::Upcoming => View::Upcoming,
                ViewArg::All => View::All,
            };
            let tasks = store.tasks(None)?;
            let rows = cli::rows(
                &tasks,
                &projects,
                &view,
                project.as_ref(),
                Local::now(),
                &config,
            );
            if json {
                println!("{}", cli::json(&rows)?);
            } else {
                print!("{}", cli::text(&rows));
            }
            Ok(())
        }
        Command::Notify {
            nag,
            dry_run,
            state,
        } => {
            let path = match state {
                Some(path) => path,
                None => State::default_path().context("no state directory for this user")?,
            };
            let tasks = store.tasks(None)?;
            let projects = store.projects()?;
            husk::notify::run(&tasks, &projects, &path, Utc::now(), &config, nag, dry_run)?;
            Ok(())
        }
        Command::Sync => {
            if config.sync_command.is_empty() {
                bail!("sync_command is empty in the config");
            }
            sync::run(&config.sync_command).map_err(|e| anyhow::anyhow!(e))
        }
    }
}
