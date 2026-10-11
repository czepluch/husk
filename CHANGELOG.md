# Changelog

Notable changes per release. Versions follow semver; the format follows
Keep a Changelog.

## Unreleased

- The due prompt and the form's due field refuse text with words left
  over after the date (`tomorrow lunch`) instead of silently dropping them.

- `contrib/husk-capture`: a quick-capture script for a launcher keybind
  that prompts with fuzzel, runs `husk add` and reports the outcome
  through the notification daemon.

- Completing a recurring task moves its due date to the rule's next
  occurrence and keeps it pending, moving absolute alarms along in
  wall-clock time and counting a COUNT down; the last occurrence completes the task itself.
  Rules the computation cannot cover are still left to the phone apps.

- The project picker, the form's field focus and its priority and project
  rows wrap past either end, and Space cycles the project row the way it
  cycles priority.

## 0.2.0 - 2026-09-02

- Adding and editing tasks happens in a popup form: title (the quick-add
  grammar still works there and wins over the fields), due, priority,
  tags, project and notes, with `$EDITOR` behind the notes row (`e`
  opens it; Enter always saves). `a` and
  `e` open it; `t`, `T` and `p` stay as single-field shortcuts. A new
  task starts in the visible project, else `default_project`, else the
  first list.

- Prompts and the filter got a movable cursor: arrows (Ctrl jumps a
  word), Home/End, Delete, and typing inserts at the cursor, instead of
  append and backspace only.

- The theme hot reload no longer fires when something merely reads the
  config directory, which the notify timer and a Waybar interval do
  constantly; only an event that can change a file reloads the theme.

## 0.1.0 - 2026-09-01

First usable version.

- Hand-written RFC 5545 codec that preserves every byte it does not model;
  round trips files written by Apple Reminders, Tasks.org and todoman.
- vdir store with atomic writes, conflict detection against the file on
  disk, move between projects, delete with undo.
- TUI: Overdue, Today, Upcoming, All and per-project views, detail pane,
  filter, completed tasks on demand, quick add with a due, priority, tags
  and project syntax, editing of every modelled field, notes and the raw
  file in `$EDITOR`, subtask nesting, project colors, recurrence shown as
  words, fading status messages with an undo hint.
- Background sync through vdirsyncer after each write, flushed on quit;
  `husk sync --discover` for new lists.
- `husk notify` for a systemd timer, with a grace window, retry on failed
  delivery and a lock file; contrib units included.
- `husk add` and `husk list --json` for keybinds and Waybar.
- Theming: phosphor and ansi flavors, Base16 and Base24 scheme files,
  `theme.toml` overrides with hot reload, `husk theme dump` and `check`.
- Timed due dates are written with the local `TZID` and a generated
  `VTIMEZONE`, so Apple Reminders shows them as local time; one alarm at
  the due time by default.
