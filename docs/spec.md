# husk: a CalDAV task TUI

Working name: `husk` (Danish "remember"). Rename at will.

## 1. Goals and non-goals

Goals

- Manage VTODO tasks from a terminal with the feature set of Apple Reminders: lists (projects), due dates and times, reminders, priorities, tags, subtasks, notes, recurrence display.
- Round-trip cleanly with Apple Reminders (iOS) and Tasks.org (Android) via a self-hosted Radicale server. A task created or edited on either phone shows up in the TUI and vice versa, including alarms.
- Desktop notifications on Linux for alarms and approaching deadlines, independent of whether the TUI is running.
- One binary. Same core code serves the TUI, a scripting CLI, and the notifier.

Non-goals (v1)

- Talking CalDAV over HTTP. vdirsyncer owns network sync in v1; the storage layer is abstracted so a direct CalDAV backend can replace it later.
- Recurrence completion. Recurring tasks are displayed with their rule; completing them happens on the phone.
- Creating or deleting projects (lists). Done on the phone or in Radicale; the TUI picks them up after `vdirsyncer discover`.
- Calendar events (VEVENT), time tracking, sharing, multi-user.

## 2. Architecture

```
  iPhone (Reminders)     Android (Tasks.org)
          \                     /
           \   CalDAV (HTTPS)  /
            v                 v
        Radicale on DappNode (HTTPS portal, htpasswd auth)
                    ^
                    | CalDAV
                    v
            vdirsyncer (systemd user timer, every 5 min,
                        plus on demand from husk)
                    |
                    v
   ~/.local/share/vdirsyncer/tasks/<collection>/<uid>.ics
                    ^
                    |  read / atomic write
                    v
  +-------------------------------------------------+
  |  husk (single binary, clap subcommands)         |
  |                                                 |
  |   core:  vdir store, VTODO codec, model,        |
  |          quick-add parser, alarm scheduling     |
  |                                                 |
  |   husk          -> ratatui TUI                  |
  |   husk add ...  -> create task (scripting)      |
  |   husk list     -> query, --json for Waybar     |
  |   husk notify   -> fire due alarms (timer)      |
  |   husk sync     -> run vdirsyncer               |
  +-------------------------------------------------+
                    |
                    v
          notify-send / D-Bus -> mako / dunst
```

Data flow: phones write to Radicale; vdirsyncer mirrors to the vdir; husk reads and writes files in the vdir and then triggers a sync. There is exactly one source of truth on the Linux side, the vdir, and husk never keeps state that is not either in an `.ics` file or in the small notifier state file.

Trade-off: depending on vdirsyncer means an external Python dependency and a polling delay. In exchange the network, auth, conflict handling and collection discovery are someone else's tested code, which is the single biggest reason homegrown todo apps die. Revisit when the direct CalDAV backend is written (section 9).

## 3. Data model

One `.ics` file per task, one VTODO per file, filename `<UID>.ics`. This is the vdir convention that vdirsyncer, todoman and khal all share.

Project = vdir collection (a directory). Its human name comes from the `displayname` file vdirsyncer writes alongside, its color from the `color` file. Tag = `CATEGORIES` entry.

Properties husk reads and writes:

| Property | Notes |
|---|---|
| `UID` | uuid v4, set on create, never changed |
| `SUMMARY` | title |
| `DESCRIPTION` | notes, multi-line |
| `DUE` | `DATE` for all-day. Timed values are read as `DATE-TIME` with `TZID`, as UTC (`Z`), or floating (treated as local). New timed values written by husk are UTC, so husk never has to generate a `VTIMEZONE`; edits keep the form the file already uses (Apple writes `TZID` plus a `VTIMEZONE` component, preserved like any other component) |
| `DTSTART` | Not modelled. Apple writes it equal to `DUE` on every dated task; Tasks.org writes it as a separate start date. When `DUE` changes: if `DTSTART` equals the old `DUE`, move it too; otherwise leave it unless it would end up after the new `DUE`, in which case clamp it to `DUE`. Never add it |
| `STATUS` | `NEEDS-ACTION`, `IN-PROCESS`, `COMPLETED`, `CANCELLED`. Tasks.org omits it on new tasks; missing means `NEEDS-ACTION` |
| `COMPLETED` | UTC timestamp, set together with `STATUS:COMPLETED` and `PERCENT-COMPLETE:100` (Apple writes all three) |
| `PRIORITY` | 1 high, 5 medium, 9 low, 0 none. Both phone apps use these values |
| `CATEGORIES` | tags |
| `RRULE` | read only in v1, shown as text |
| `VALARM` | `ACTION:DISPLAY`, `TRIGGER` absolute (`VALUE=DATE-TIME`, Apple writes these in UTC) or relative: `RELATED=END` or no parameter means relative to `DUE`, `RELATED=START` relative to `DTSTART`. Tasks.org writes three alarms per dated task including a repeating one (`DURATION` plus `REPEAT`); v1 fires each alarm's first trigger only and ignores `REPEAT`. Apple adds `UID` and `X-WR-ALARMUID` inside the alarm; preserve everything |
| `RELATED-TO;RELTYPE=PARENT` | subtask link, read only in v1. Tasks.org writes `RELATED-TO` with no `RELTYPE` at all; missing means `PARENT` |
| `SEQUENCE`, `LAST-MODIFIED`, `DTSTAMP`, `CREATED` | bumped or set on every write. Apple omits `SEQUENCE` on new tasks; a missing value counts as 0 |

Invariant: every property husk does not understand, including `X-APPLE-*` and `X-MOZ-*`, is preserved verbatim on write. The codec parses the component into an ordered property list, the model is a view over it, and saving patches only the properties husk changed. Without this, Apple sort order and Tasks.org metadata get destroyed on the first edit and both phone apps behave strangely.

Rust model, kept deliberately small:

```rust
pub struct Task {
    pub uid: String,
    pub project: ProjectId,       // collection dir name
    pub summary: String,
    pub description: Option<String>,
    pub due: Option<Due>,         // Due::Date(NaiveDate) | Due::DateTime(DateTime<Tz>)
    pub status: Status,
    pub priority: Priority,       // None | High | Medium | Low (+ raw u8 for odd values)
    pub tags: Vec<String>,
    pub alarms: Vec<Alarm>,       // Alarm::Absolute(DateTime<Utc>) | Alarm::Relative(Duration)
    pub rrule: Option<String>,    // raw, plus a derived human string
    pub parent: Option<String>,
    raw: IcalComponent,           // untouched properties, for round-trip
}
```

## 4. Storage layer

```rust
pub trait Store {
    fn projects(&self) -> Result<Vec<Project>>;
    fn tasks(&self, project: Option<&ProjectId>) -> Result<Vec<Task>>;
    fn get(&self, uid: &str) -> Result<Task>;
    fn create(&self, project: &ProjectId, task: NewTask) -> Result<Task>;
    fn save(&self, task: &Task) -> Result<()>;
    fn delete(&self, uid: &str) -> Result<()>;
    fn move_to(&self, uid: &str, project: &ProjectId) -> Result<()>;
}
```

`VdirStore` is the only implementation in v1. Rules:

- Write atomically: serialize to `<uid>.ics.tmp` in the same directory, fsync, rename over the target. vdirsyncer detects the change by etag (file hash), so partial writes must never be visible.
- On every save: `SEQUENCE += 1`, `LAST-MODIFIED` and `DTSTAMP` = now (UTC). Phone clients use these to decide which side wins.
- Move between projects = write into the new directory, delete from the old one, same UID. vdirsyncer handles this as delete plus create on the server, which is what CalDAV expects.
- Delete = remove the file. Offer a session-level undo by keeping the last N removed or overwritten file contents in memory.
- Line folding at 75 octets, escape `,` `;` `\` and newlines in text values. Unfold on read. Get this right once, in one place, with tests.
- Line endings: Radicale stores CRLF but vdirsyncer writes LF into the vdir, while files written locally (todoman, husk) keep whatever their author used, so the vdir is mixed. Accept both on read, write back whatever the file used, and use CRLF for new files. This is what keeps an unmodified save byte-identical.

The UI and CLI never touch the filesystem. Everything goes through `Store`, which is what makes the later CalDAV backend a drop-in.

## 5. Sync

vdirsyncer config (`~/.config/vdirsyncer/config`):

```ini
[general]
status_path = "~/.local/share/vdirsyncer/status/"

[pair tasks]
a = "tasks_local"
b = "tasks_remote"
collections = ["from a", "from b"]
conflict_resolution = "b wins"
metadata = ["displayname", "color"]

[storage tasks_local]
type = "filesystem"
path = "~/.local/share/vdirsyncer/tasks"
fileext = ".ics"

[storage tasks_remote]
type = "caldav"
url = "https://radicale.<your-dyndns>.dappnode.io/"
username = "jacob"
password = "<radicale password>"
item_types = ["VTODO"]
```

`b wins` means the server (and therefore the phone) wins on the rare true conflict. husk syncs immediately after each write, so the window for conflicts is seconds. If you would rather be asked, `["command", "vimdiff"]` works too.

The password sits in the config file itself, `chmod 600`. A `pass` or keyring lookup needs an unlocked gpg-agent or secret service every time the systemd timer fires, which fails quietly after a reboot until something prompts for the key. A mode-600 file in the home directory is the same trust boundary as the vdir it syncs.

husk runs `vdirsyncer sync` (configurable command) in a background thread after every mutation, debounced to one run per two seconds, and on the `s` key. A systemd user timer every five minutes is the backstop and also picks up phone edits. `vdirsyncer discover` followed by `vdirsyncer metasync` must run whenever a list is created or renamed on a phone, because `sync` alone never carries `displayname` and `color`; `husk sync --discover` wraps both. The Arch package ships `vdirsyncer.timer` at 15 minutes; a drop-in sets `OnUnitActiveSec=5m`.

## 6. Notifications

`husk notify` is a stateless-ish subcommand run by a systemd user timer every minute:

1. Load all pending tasks.
2. For each task compute fire times: every `VALARM` (absolute, or `DUE` plus relative trigger), plus configurable deadline lead times for tasks with a timed `DUE` and no alarm (default `["1d", "1h", "0m"]`).
3. Fire any time in `[last_run, now]` that is not in the fired set, via `notify-rust` (D-Bus, works with mako and dunst under Hyprland). Overdue tasks get one notification at the moment they become overdue, then nothing until re-run with `--nag`.
4. Persist fired keys `(uid, fire_time)` and `last_run` to `~/.local/state/husk/notify.json`. Prune entries older than 30 days.

Idempotent by construction: running it twice in a row fires nothing the second time; missing a few runs (laptop asleep) fires everything that came due since `last_run` once, on wake.

Writing alarms: when the TUI creates or sets a timed due date, it adds `VALARM` with `TRIGGER:-PT0M` (at due) by default, plus any lead times from config. This is what makes a task created in the terminal notify on the phone. Apple Reminders fires from the alarm, not the due date, so this is not optional if phone notifications matter. Tasks.org honors the same alarm.

## 7. TUI

Layout, ratatui plus crossterm:

```
 Projects        | Tasks: Today                         3 due
------------------+------------------------------------------------
 > Today       4  | ! 09:00  Book dentist            #health
   Upcoming   11  |   14:00  Call Gustav             #friends
   All        37  |   Fri    Oil balcony             #home  ↻ weekly
------------------+   -      Read Crafting Interp.   #reading
   Personal   19  |
   Home        9  |
   Argot       9  |
------------------+------------------------------------------------
 a add  d done  e edit  t due  p pri  m move  / filter  s sync  ? help
```

Views: `Today` (due today or overdue, all projects), `Upcoming` (next 7 days), `All`, one per project, `Completed` (toggle with `c`). Sort: overdue first, then due time, then priority, then created. Recurring tasks show `↻` and a human rule ("weekly", "every 2nd Tuesday"); `d` on one is refused with a one-line hint. Subtasks render indented under their parent.

Keys, vim-flavored: `j`/`k` move, `Tab` switch pane, `Enter` detail, `a` quick add, `d` done, `x` delete (confirm), `u` undo, `e` edit summary, `n` edit notes in `$EDITOR`, `t` due, `p` priority, `T` tags, `m` move project, `/` filter, `s` sync, `c` show completed, `?` help, `q` quit.

Quick add grammar, one line, Taskwarrior-shaped, parsed by a small tokenizer rather than NLP:

```
Book dentist due:tomorrow 09:00 pri:H +health @personal
```

- `due:` accepts `today`, `tomorrow`, weekday names, `next mon`, `+2d`, `2026-09-03`, optional `HH:MM` after any of them; a bare `HH:MM` means today.
- `+tag` adds to `CATEGORIES`, `@project` overrides the selected project, `pri:H|M|L`.
- Everything else is the summary. Unknown tokens stay in the summary; the parser never errors.

Same grammar drives `husk add`, so you can bind a Hyprland key to `fuzzel --dmenu | xargs husk add` for capture without opening the TUI.

Detail view shows every field including alarms and raw RRULE, and `o` opens the `.ics` in `$EDITOR` as an escape hatch.

## 7b. Theming

Requirement: the look is a first-class feature, not an afterthought. Default feel is cypherpunk: dark, high contrast, phosphor accents, sparse chrome, no rounded boxes. Everything about it must be overridable without touching code, and existing theme ecosystems must work out of the box.

Design, in three layers, all resolved into one `Theme` struct at startup:

1. Terminal palette default. With no config, husk uses only the 16 ANSI colors plus default fg/bg, the way lazygit does. Whatever your terminal and pywal or matugen already decided is what husk looks like. Zero-config consistency with the rest of the Hyprland setup.

2. Scheme files. `theme = "~/.config/husk/themes/<name>.yaml"` loads any Base16 or Base24 scheme file from tinted-theming unmodified. That is several hundred maintained schemes (Catppuccin, Gruvbox, Tokyo Night, Nord, and so on) with no porting work, and the same files btop, yazi flavors and many others are generated from. husk maps the 16 or 24 base slots to its semantic slots with a fixed table.

3. Semantic overrides. `~/.config/husk/theme.toml` sets individual semantic slots, on top of either layer above. This is the yazi and btop model: a flat file of named slots, each a style (`fg`, `bg`, `bold`, `italic`, `underline`, `dim`), colors as hex, ANSI names, or `base0X` references.

Semantic slots, kept small and stable so themes do not rot when the UI changes:

```toml
[colors]
bg = "default"          # or hex / ansi / base00
fg = "base05"
muted = "base03"
accent = "base0B"       # selection, active pane, key hints
border = "base02"
border_active = "base0B"
overdue = "base08"
due_today = "base0A"
due_soon = "base0D"
done = "base03"
pri_high = "base08"
pri_medium = "base0A"
pri_low = "base0C"
tag = "base0E"
project = "base0D"
recurring = "base0C"

[styles]                # optional per-element overrides
selected = { bg = "base02", bold = true }
title = { fg = "accent", bold = true }
status_bar = { fg = "muted" }
help_key = { fg = "accent", bold = true }

[symbols]
set = "nerd"            # "nerd" | "ascii" | custom table below
recurring = ""
overdue = "!"
done = "✓"
subtask = "└"
alarm = ""

[borders]
style = "plain"         # "plain" | "rounded" | "double" | "thick" | "none"
```

Shipped built-in flavors, selectable by name, all written as ordinary `theme.toml` files embedded in the binary so they double as documentation and templates:

- `phosphor` (default): near-black bg, green accent, amber for due today, red for overdue, dim gray chrome. The cypherpunk one.
- `ansi`: the layer-1 terminal palette, for people who theme their terminal once.
- `mono`: no color, weights and dim only, for tmux over ssh on a bad day.

Implementation notes:

- `theme.rs` owns parsing, the Base16 mapping table, resolution into a `Theme` of `ratatui::style::Style` values, and nothing else. The UI code references `theme.overdue`, never a color literal. Enforce this with a clippy deny on `Color::` outside `theme.rs`.
- Hot reload: watch `theme.toml` with the `notify` crate and re-resolve on change. Iterating on a theme without restarting is what makes people actually make themes.
- `husk theme dump` prints the fully resolved theme as TOML, so a user can start from the current look instead of from scratch. `husk theme check <file>` validates a file and reports unknown slots.
- Symbols must have an ASCII fallback so the app works without a Nerd Font; `set = "ascii"` swaps the whole table.
- Base16 mapping is the one piece of policy: `base00` bg, `base05` fg, `base03` muted, `base02` selection bg, `base08` red for overdue and high priority, `base0A` yellow for due today and medium, `base0B` green for accent, `base0D` blue for soon and projects, `base0E` magenta for tags, `base0C` cyan for recurring and low. Document it in the README so scheme authors know what to expect.

Add to M2 (read-only TUI): the theme loader with the `phosphor` and `ansi` flavors, since layout and colors are settled together. Add to M5: Base16 loading, hot reload, `husk theme dump` and `check`.

## 8. Crate layout and dependencies

Single crate: a library target holding the modules below and a thin binary on top, so the tests in `tests/` can reach the modules. Dependencies are added by the milestone that first uses them.

```
src/
  lib.rs          module declarations only
  main.rs         clap: tui (default) | add | list | notify | sync
  config.rs       ~/.config/husk/config.toml via serde + toml
  model.rs        Task, Project, Due, Alarm, Priority, Status
  ical/
    codec.rs      unfold/fold, escape, parse to IcalComponent, serialize
    vtodo.rs      IcalComponent <-> Task, property-level patching
  store/
    mod.rs        Store trait
    vdir.rs       VdirStore
  quickadd.rs     tokenizer + date parser
  alarms.rs       fire-time computation (shared by TUI and notify)
  sync.rs         run vdirsyncer, debounce
  notify.rs       husk notify
  theme.rs        theme files, Base16 mapping, hot reload; only place colors live
  themes/         embedded flavors: phosphor.toml, ansi.toml, mono.toml
  ui/
    app.rs        state machine, key handling
    views.rs      project pane, task list, detail
    widgets.rs
```

Dependencies: `ratatui`, `crossterm`, `clap`, `serde` + `toml`, `chrono` + `chrono-tz`, `uuid`, `notify-rust` (desktop notifications), `notify` (file watching for theme hot reload), `anyhow`, `directories`. For RRULE display, `rrule` for parsing and validation only; it has no human-readable describer, so the "weekly" / "every 2nd Tuesday" text is a small function of ours, and occurrences are never expanded in v1. For Base16 scheme files (M5), `serde_yaml` is archived upstream; a scheme file is a flat `palette:` map, so either a maintained YAML crate or a few dozen lines of hand parsing, decided when M5 starts. Write the iCalendar codec yourself; the existing crates either do not preserve unknown properties or do not round-trip, and the format is small enough that a few hundred lines with fixtures is less risk than a dependency you have to fight.

Config:

```toml
vdir = "~/.local/share/vdirsyncer/tasks"
default_project = "personal"
sync_command = ["vdirsyncer", "sync"]
date_format = "%Y-%m-%d"
time_format = "%H:%M"
default_alarm_leads = ["1d", "1h", "0m"]
```

## 9. Implementation plan

M0, half a day: infrastructure. Radicale on the DappNode behind the HTTPS portal, `rights = owner_only`, bcrypt htpasswd. Add the account on iPhone (Settings, Calendar accounts, CalDAV, Reminders on) and Tasks.org on Android. vdirsyncer config and timer. Verify the round trip by hand with todoman. Before writing any Rust, save a handful of `.ics` files as produced by each phone app into `tests/fixtures/`; these are the real spec.

M0 without the DappNode: until a DappNode package exists, run Radicale on the laptop (`radicale` and `python-bcrypt` from the Arch repos, `hosts = 0.0.0.0:5232`, otherwise the same config) and open port 5232 to the LAN in ufw. Both phone apps refuse Basic auth over cleartext HTTP (iOS silently ignores the 401 challenge, Tasks.org reports "not permitted by network config"), so Radicale needs TLS: make a local CA and a server certificate with openssl, with the laptop's LAN IP in the SAN, SHA-256, a 2048-bit key, `extendedKeyUsage = serverAuth` and at most 825 days validity (what iOS requires of user-installed certificates), and `keyUsage = critical, keyCertSign, cRLSign` on the CA (Python 3.13+ verifies strictly and rejects a CA without it; the phones do not care). Set `ssl = True`, `certificate` and `key` under `[server]`, and install the CA with full trust on each phone. Phones point at `https://<laptop-lan-ip>:5232/`; vdirsyncer uses `https://localhost:5232/` (put `localhost` in the SAN too) with `verify = "<path to ca.crt>"`, or `verify_fingerprint = "<sha256 of server.crt>"` to pin the server certificate instead of validating the chain, so a DHCP change only affects the phones. Radicale stores one `.ics` per item under `collections/`, so moving to the DappNode later is copying that directory into the package volume, re-pointing the phones and vdirsyncer at the HTTPS URL, and clearing the pair status under `~/.local/share/vdirsyncer/status/` so vdirsyncer re-pairs items by UID instead of treating the new server as empty. husk never talks to the server, so nothing in it changes; during M1 and M2 `sync_command` can be `["true"]`.

M1, weekend 1: codec and store. Fold/unfold, escaping, parse, serialize. Round-trip test: for every fixture, parse then serialize must equal the input modulo folding. `VdirStore` with atomic writes, sequence bump, move, delete. Test against a temp dir.

M2, weekend 2: read-only TUI. Projects pane, task list, Today and Upcoming views, detail view, filter. No writes yet. This is where the layout and keybindings get settled cheaply.

M3, weekend 3: mutations. Quick add and its tests, done, delete with undo, edit due, priority, tags, notes via `$EDITOR`, move. Sync trigger after writes. Alarm written on create. From here on you can live on it.

M4, one evening: `husk notify`, state file, systemd timer, `husk list --json` for Waybar, `husk add` for the Hyprland capture keybind.

M5, as needed: RRULE description, subtask rendering, colors from the vdir `color` file, `c` completed view, `husk sync --discover`.

Later, optional: `CaldavStore` implementing `Store` over HTTPS with `reqwest` (PROPFIND, REPORT, sync-token, If-Match on PUT). At that point vdirsyncer becomes optional and husk can run on a machine with no sync setup. Also: recurrence completion (advance `DUE` by the rule, bump `SEQUENCE`), creating projects (MKCALENDAR), attachments never.

## 10. Known pitfalls, collected up front

- Apple Reminders polls CalDAV on a fetch interval (Settings, Accounts, Fetch New Data). Set it to 15 minutes or open the app to force a pull; there is no push for third-party CalDAV.
- Apple writes `DUE` as `DATE-TIME` with a `TZID`, marks completion with three properties, and adds `X-APPLE-SORT-ORDER` and `X-APPLE-*` alarm metadata. Preserve all of it.
- Tasks.org writes relative alarm triggers and `X-APPLE-SORT-ORDER` of its own; same rule. It also omits `STATUS` and `CALSCALE`, uses numeric UIDs, and writes timed `DUE` values one second past the minute (`T130001`) to mark "has a time". Display minutes only. A `:00` value written by another client is still shown as timed: a UTC `DUE` with `:00` seconds and no `VTIMEZONE` (as written by todoman) displays at the right local time on both phones, verified 2026-08-31.
- Apple Reminders on a CalDAV account has no tags, subtasks or flags (iCloud only). A `#tag` typed on the iPhone stays literal text in `SUMMARY`. Tags and subtasks therefore only come from Tasks.org or husk; whether Apple preserves `CATEGORIES` and `RELATED-TO` on a task it edits needs to be checked with a fixture before M3 writes them.
- Apple keeps properties sorted alphabetically and inserts new ones in order (`COMPLETED` lands before `CREATED`). Order is preserved, never imposed.
- Radicale stores completed tasks forever. Show them under `Completed` and add `husk purge --older-than 90d` later if the vdir gets big.
- A floating `DUE` (no `TZID`, no `Z`) is legal and shows up from some clients. Treat as local time.
- vdirsyncer refuses to run if two files claim the same UID. The store must never create that state; move is write-new-then-delete-old, never copy-then-forget.
- DST: compute all fire times in UTC from the zoned due time; never do arithmetic on naive local times.
- Do not implement RRULE expansion in v1. The phone does it, and it is the single largest source of bugs in this class of software.

## 11. Bonus ideas, cheap once the core exists

- Waybar module: `husk list --view today --json | jq length` for a due-today badge; click opens the TUI in a terminal.
- `husk add` from a Hyprland keybind with a fuzzel prompt, as above.
- Share a list with your partner: create it in Radicale under a shared collection with `rights` allowing both users; Apple Reminders and Tasks.org both handle it, husk sees it as one more project.
- Since Radicale is now running, point khal and Apple Calendar at it too; `Upcoming` could show today's events read-only for context, later.
- Tag-based views (`#health` across all projects) fall out of the filter for free.
