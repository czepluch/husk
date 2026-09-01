# husk

CalDAV task TUI in Rust (ratatui). The full design is in `docs/spec.md`; read it before any non-trivial change. This file holds the rules that keep the codebase small.

## Ground rules

- KISS and DRY. No feature that is not in the spec's current milestone. If something seems needed, propose it in one sentence and wait.
- One `Task` model, one iCalendar codec, one `Store` trait. UI and CLI never touch the filesystem or parse `.ics` text directly.
- Colors and styles exist only in `src/theme.rs` and `src/themes/*.toml`. `ratatui::style::Color` and the `Stylize` trait anywhere else fail the build (clippy `disallowed_types` in `clippy.toml`), and `tests/lint.rs` rejects `Color::`, `Stylize` and `prelude::*` in every other source file.
- Preserve every iCalendar property husk does not model. Saving an unmodified task must produce a byte-identical file modulo line folding. This is tested; do not weaken the test.
- Never expand RRULE. Parse, describe, display. Completing a recurring task is refused with a hint.
- All time arithmetic in UTC or in a zoned `DateTime<Tz>`. No `NaiveDateTime` math.
- Writes are atomic: temp file in the same directory, fsync, rename. Bump `SEQUENCE`, `LAST-MODIFIED`, `DTSTAMP` on every save.
- Dependencies: only those listed in the spec unless justified in one sentence. Prefer writing 50 lines over adding a crate.

## Workflow

- Work one milestone at a time, in the order in the spec. Do not start M(n+1) until M(n) tests pass.
- Tests first for `ical/` and `quickadd.rs`: fixtures in `tests/fixtures/` are the spec for interop; add a fixture for every bug found.
- `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test` before declaring anything done.
- Small commits, one concern each, imperative subject line under 60 chars. Commit without asking; always ask before pushing feature work. A fix for a red CI run on an already pushed branch may be pushed without asking.
- Before declaring a milestone done, spawn a fresh review agent with no shared context to review the milestone's code against the spec and this file, then address its findings.
- When a better approach than the spec's turns up, update the spec and say what changed in one sentence. Features outside the current milestone still wait for approval.
- Visual details the user has commented on (colors, weights, symbols, layout) are changed only on request. Propose alternatives, with a rendered preview when possible, and wait.
- Do not touch `~/.local/share/vdirsyncer` during tests; use a temp dir.

## Style

- Explanatory but short comments only where the *why* is not obvious (folding rules, Apple/Tasks.org quirks, DST).
- Prefer iterators and pattern matching over index loops and flags.
- Errors: `anyhow` in binaries and glue, typed errors only if a caller needs to branch on them.
- No em-dashes in strings, docs, or comments.

## Definitions

- **vdir**: one directory per collection, one `.ics` per item, filename `<UID>.ics`.
- **project**: a vdir collection. **tag**: a `CATEGORIES` value.
- **fixture**: a real `.ics` file as written by Apple Reminders, Tasks.org, or Radicale, unmodified.
