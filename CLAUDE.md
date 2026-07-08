# rugit

A standalone Magit-style Git TUI written in Rust (ratatui). The architecture
and its rationale live in `DESIGN.md` — read it before making structural
changes.

## Commands

```sh
cargo build                 # binary: target/debug/rugit
cargo test                  # unit tests + integration tests (needs a real `git` binary)
cargo clippy --all-targets  # must stay at 0 warnings
```

Manual smoke test: run `cargo run` inside any git repository (`q` quits). To
drive the TUI headlessly, run it in a PTY and pipe keystrokes:

```sh
{ sleep 1.5; printf 'q'; } | script -qec "stty rows 30 cols 100; target/debug/rugit" /dev/null
```

## Architecture in one minute

- **Event-driven Elm-style loop**: everything (terminal input, git worker
  completions, `.git` fs notifications) funnels into one
  `crossbeam_channel<AppEvent>`; the main loop blocks on `recv()`, updates
  `App`, redraws. No tokio, no tick — git mutations run on plain worker
  threads.
- **Every buffer is a section tree** (`ui/section.rs`): status, revision, and
  process-log buffers are all trees of `Section`s flattened into `FlatLine`s.
  Commands dispatch DWIM on the `SectionValue` under the cursor (`s` stages a
  file, a hunk, or a single diff line depending on where point is).
- **Commands are data** (`command.rs`): keymaps (`keymap.rs`, a key-sequence
  trie) map keys to `Command` values, which makes TOML remapping and the help
  UI free.
- **Overlays are data-driven**: transient menus (`ui/transient.rs`),
  minibuffer/picker (`ui/input.rs`). Key-resolution priority:
  confirm > input > help > transient > keymap (buffer-local > global).
- **Git = shell out to the `git` CLI** (`git/client.rs`): parsers
  (`git/parse.rs`) and patch builders (`git/patch.rs`) are pure functions.

## Invariants — do not break these

- **No hardcoded colors** in `ui/` rendering or tree-building code. Every
  color goes through a named role on `Theme` (`src/theme.rs`) so the
  `[colors]` config section can restyle everything.
- **Keep the pure modules pure**: `git/parse.rs`, `git/patch.rs`,
  `ui/section.rs`, `ui/pane.rs` must stay free of I/O so they remain
  unit-testable against fixture strings.
- **All git reads go through `GitClient`**, which forces
  `--no-optional-locks`. A read that writes `.git/index` re-triggers the fs
  watcher and causes a refresh loop.
- **`git/patch.rs` is the one place that can corrupt the index.** Any change
  there needs a roundtrip test against real git in
  `tests/git_integration.rs`, not just unit tests.
- **Trees are always rebuilt, never mutated in place**, and must be swapped
  in via `Pane::replace_tree` — that is what preserves fold state and the
  cursor (section-identity based, not line-number based) across refreshes.
- Refresh results carry a generation counter; stale snapshots are dropped.
  Don't bypass `App::refresh`.
- **`App::update` and `App::dispatch` are pure routers**: every match arm is
  a one-line delegation. If an arm needs a body, extract it into the
  submodule that owns the concern (`app/keys.rs`, `app/dwim.rs`,
  `app/workers.rs`, `app/search.rs`, `app/ops/*`). Grouped commands
  (`Command::Nav`, `Command::Transient`) must not be un-grouped back into
  per-variant arms.

## Adding things

- **New command**: add a `Command` variant + `COMMANDS` entry
  (`command.rs`), handle it in `App::dispatch` (one-line delegation; pure
  cursor motions go in `NavCmd` instead and cost no dispatch arm), bind it
  in `keymap::default_keymaps`. Help and which-key pick it up automatically.
- **New transient menu**: add a `Menu` variant (`command.rs`), define a
  `TransientDef` + `menu_def` arm in `ui/transient.rs`, map its
  `TransientAction`s in `App::invoke_transient`. Actions that need an
  argument call `App::open_input` / `open_picker` with a continuation
  closure; multi-step flows chain by opening the next input inside it.
- **New buffer kind**: add a `PaneKind`, a tree builder in `ui/build.rs`,
  and (optionally) a buffer-local keymap. Navigation, scrolling, folding,
  search, and rendering come for free from `Pane`.

## User config

`~/.config/rugit/config.toml` (`$XDG_CONFIG_HOME` respected): `scrolloff`,
`[keys.global]` / `[keys.status]` (Emacs-style key notation, space-separated
sequences), `[colors]` (role name → color name / `#rrggbb` / 256-color
index). Invalid entries become startup warnings, never errors.
