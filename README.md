# husk

Tasks in the terminal, synced with your phone. husk is a CalDAV task (VTODO)
client that round-trips cleanly with Apple Reminders and Tasks.org through
vdirsyncer and a CalDAV server. One binary gives you a TUI, a scripting CLI
for keybinds and Waybar, and a notifier for a systemd timer.

```
iPhone Reminders / Tasks.org  <-- CalDAV -->  CalDAV server (Radicale, Nextcloud, Baikal, Fastmail, iCloud)
                                                 ^
                                                 | vdirsyncer (timer, and after every husk write)
                                                 v
                              ~/.local/share/vdirsyncer/tasks/<list>/<uid>.ics
                                                 ^
                                                 | atomic read / write
                                                 v
                                               husk
```

husk reads and writes `.ics` files in the vdir and never talks to the server
itself. Every property it does not understand is preserved byte for byte, so
Apple and Tasks.org metadata survives an edit here, and their edits survive a
sync. The full design is in `docs/spec.md`.

## Install

Each release carries Linux x86_64 binaries: a `linux-gnu` build and a static
`linux-musl` one for distributions with an older glibc.

```
gh release download v0.1.0 --repo czepluch/husk --pattern '*linux-gnu.tar.gz'
tar xzf husk-v0.1.0-x86_64-unknown-linux-gnu.tar.gz
install -Dm755 husk-v0.1.0-x86_64-unknown-linux-gnu/husk ~/.local/bin/husk
```

Or build from source with Rust 1.89 or newer:

```
cargo install --git https://github.com/czepluch/husk    # or --path . in a checkout
```

husk is developed and tested on Linux. The TUI and CLI have nothing
Linux-specific in them, but `husk notify` shells out to `notify-send`, and
there are no macOS builds yet.

You need vdirsyncer with a `tasks` pair pointing at your CalDAV server, and
a notification daemon such as mako or dunst if you want alarms on the desktop.
Any server the phone apps accept works: Radicale (self-hosted, what husk was
built against; section 5 of the spec has a working config), Nextcloud,
Baikal, Fastmail or iCloud. Apple Reminders and Tasks.org each need the
server added as a CalDAV account on the phone.

## Configuration

`~/.config/husk/config.toml`, every key optional:

```toml
vdir = "~/.local/share/vdirsyncer/tasks"
default_project = "Life"                # for quick add outside a project view
sync_command = ["vdirsyncer", "sync"]   # an empty list turns syncing off
date_format = "%Y-%m-%d"
time_format = "%H:%M"
default_alarm_leads = ["0m"]                # one alarm, at the due time
theme = "phosphor"                      # phosphor, ansi, or a Base16 scheme file
```

Any command takes `--config <file>` to use another config.

## The TUI

`husk` opens it. Left pane: Overdue, Today, Upcoming, All, then one entry per
list. Right pane: the tasks of that view, overdue first, then by due time,
priority and creation time; subtasks sit under their parent.

| Key | Action |
|---|---|
| `j` `k` `g` `G` | move, first, last (arrows work too) |
| `Tab` | switch pane |
| `Enter` | open the task; in the left pane, focus the list |
| `a` | add: `Book dentist due:fri 09:00 pri:H +health @life` |
| `d` | done, or reopen (recurring tasks are completed on the phone) |
| `x` | delete, after `y/n` |
| `u` | undo the last change (the bar says `(u undo)` when it applies) |
| `e` `t` `p` `T` | edit the title, the due date, cycle priority, edit tags |
| `m` | move to another list |
| `n` `o` | edit the notes, or the raw `.ics`, in `$EDITOR` |
| `/` | filter by text; `Enter` keeps it, `Esc` clears it |
| `c` | show or hide completed tasks |
| `s` | sync now |
| `?` `q` | help, quit |

Quick add reads `due:` (`today`, `tomorrow`, `fri`, `next mon`, `+2d`,
`2026-09-03`, each with an optional `HH:MM`; a bare `HH:MM` means today),
`pri:H|M|L`, `+tag` and `@list`. Anything else is the title. The due prompt
(`t`) takes the same date words; an empty answer clears the date.

Setting a time on a task that has no alarms adds the alarm from
`default_alarm_leads` (at the due time by default), which is what makes the
phones notify too; existing alarms are left alone. Apple Reminders shows a
task's first alarm as its reminder time, so keep it to one.

## The CLI

```
husk add "Call bank due:tomorrow 09:00 +money"   # waits for the sync
husk list [--view overdue|today|upcoming|all] [--project X] [--json]
husk notify [--nag] [--dry-run]                  # see Notifications
husk sync [--discover]                           # --discover after a new list on a phone
husk theme dump | check <file>
```

Hyprland capture keybind: `fuzzel --dmenu | xargs -r husk add`.

Waybar badge: `husk list --json | jq length` counts tasks due today or
overdue; `--view overdue` for the overdue count alone. `list --json` rows carry
`uid`, `summary`, `project`, `due` (RFC 3339 or `YYYY-MM-DD`), `due_label`,
`overdue`, `priority`, `tags`, `recurring`, `parent` and `project_id`.

## Notifications

`husk notify` fires a desktop notification for every alarm that came due
since its last run, once each, and remembers what it fired in
`~/.local/state/husk/notify.json`. Run it from a timer:

```
cp contrib/husk-notify.service contrib/husk-notify.timer ~/.config/systemd/user/
systemctl --user enable --now husk-notify.timer
```

`husk notify --dry-run` shows what would fire. `--nag` also repeats every
overdue task.

## Theming

Three layers, all resolved into one theme at startup and reloaded while husk
runs whenever a file under `~/.config/husk` changes:

1. A built-in flavor, `theme = "phosphor"` (dark, phosphor green accent,
   amber for today, red for overdue) or `theme = "ansi"` (only the terminal's
   own sixteen colors, so pywal or matugen decide the look).
2. A Base16 or Base24 scheme file, `theme = "~/.config/husk/themes/gruvbox.yaml"`,
   used unmodified from tinted-theming. Slots map as below.
3. `~/.config/husk/theme.toml`, setting individual slots on top of either:

```toml
[colors]
accent = "#ff8800"       # hex, an ANSI name, a 0-255 index, or base0X with a scheme
overdue = "bright_red"

[styles]
selected = { bg = "#243524", bold = true }
pri_high = { fg = "#ff6ec7", bold = true }   # priority is a weight by default

[symbols]
set = "ascii"            # unicode (default) or ascii; single keys override

[borders]
style = "rounded"        # plain, rounded, double, thick, none
```

`husk theme dump` prints the theme in use, fully resolved, as a starting
point. `husk theme check file.toml` validates a file and names unknown slots
or colors.

Base16 mapping, for scheme authors:

| Slot | Base16 |
|---|---|
| bg, fg | base00, base05 |
| muted, done | base03 |
| border, selection background | base02 |
| accent, border_active | base0B |
| overdue | base08 |
| due_today | base0A |
| due_soon, project | base0D |
| tag | base0E |
| recurring | base0C |

Priority is not a color: hue means time, weight means importance (bold for
high, dim for low), and a `!!!` marker column repeats it.

## Development

`cargo test` runs the codec round trip over `tests/fixtures/`, real `.ics`
files written by Apple Reminders, Tasks.org and todoman; a fixture is never
edited. `cargo clippy --all-targets -- -D warnings` must be clean; colors may
only appear in `src/theme.rs`, which `tests/lint.rs` enforces. Tests run under
two time zones in CI.
