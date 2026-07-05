# rugit — Design Document

A standalone reimplementation of Magit in Rust. This document records the
library choices, the architecture, and the shared implementation patterns —
and the reasoning behind them.

## 1. Library selection

| Area | Choice | Rationale |
|---|---|---|
| TUI | **ratatui + crossterm** | The de-facto standard. Immediate-mode rendering fits the Elm-style architecture. crossterm is consumed through the `ratatui::crossterm` re-export to avoid version skew |
| Git | **shelling out to the `git` CLI** | See below. git2/gix rejected |
| File watching | **notify-debouncer-mini** | Auto-refresh on `.git` changes (200 ms debounce) |
| Config | **serde + toml** | Keybindings, colors, options |
| Errors | **thiserror** (git layer) + **anyhow** (app layer) | |
| Concurrency | **std::thread + crossbeam-channel** | No tokio. See below |
| Testing | **tempfile** + a real `git` binary | Integration tests build fixture repositories |

Future candidates: `similar` (word-level intra-hunk diff highlighting),
`syntect` (syntax highlighting, behind a feature flag).

### Why shell out to the git CLI

Magit itself shells out to git. This is not a shortcut — it is the correct
design:

- **libgit2 (git2) has gaps**: interactive rebase, hook execution, GPG
  signing, credential helpers, and more. "Behaves exactly like the user's
  git" is the lifeblood of a Magit clone; depending on libgit2's
  reimplementations guarantees divergence.
- **gitoxide (gix)** is fast for reads but incomplete for writes.
- Reads are fast and stable enough via porcelain formats, whose backward
  compatibility git guarantees.

Every read command carries `--no-optional-locks`. `git status`
opportunistically rewrites the index, which would trigger the fs watcher and
loop the auto-refresh forever.

The git layer is isolated from the app so that reads could later be swapped
to gix without touching anything else.

### Why not tokio

Git operations are process spawns; async buys almost nothing. Worker threads
that post a "done" event to the main loop keep the entire core synchronous —
no lifetime gymnastics, no `Send` contamination (gitui does the same).
Cancellation, if ever needed, is killing the child process.

## 2. Architecture: Elm-style (Model–Update–View)

ratatui is immediate-mode, so all event sources funnel into a single event
loop.

```
┌────────────────┐  ┌──────────────┐  ┌──────────────┐
│ terminal input │  │ git workers  │  │ notify(.git) │
└───────┬────────┘  └──────┬───────┘  └──────┬───────┘
        │   crossbeam-channel<AppEvent>      │
        └────────────┬───────────────────────┘
               ┌─────▼─────┐
               │  update() │ ── thread::spawn workers ──▶ send AppEvent on completion
               ├───────────┤
               │  render() │ (redraw after events; queued events are coalesced)
               └───────────┘
```

```rust
enum AppEvent {
    Key(KeyEvent),
    Resize,
    SnapshotReady { gen: u64, result: Result<StatusSnapshot, String> },
    GitDone { desc: String, entry: ProcessEntry },
    RevisionReady { title: String, header: String, diff: String },
    RepoChanged,   // from notify (debounced)
}
```

- **Fully event-driven**: the main loop blocks on `rx.recv()`; there is no
  tick and no polling. Rendering happens only after handling events (a
  backlog is coalesced into a single redraw). The one exception is *inside*
  the input thread, which uses crossterm's `poll(100ms)` loop — a deliberate
  compromise so stdin reading can be reliably paused during $EDITOR handoff
  (a blocking `read()` cannot be interrupted from outside). It does not
  affect UI latency or redraw frequency.
- **Single-flight refresh + generation counter**: at most one snapshot
  read runs at a time — refreshes requested while one is in flight only
  set a dirty flag, and one follow-up read runs on completion. This keeps
  a burst of watcher events (a fetch updating many refs, `git gc`) from
  piling up concurrent whole-worktree scans on a large repo. Every read
  still carries a `gen` tag and stale results are dropped, as a safety
  net.
- **Mutation completed → auto refresh**: receiving `GitDone` always
  re-reads the snapshot.
- **$EDITOR handoff**: `git commit` needs the terminal. `update()` only
  queues an `EditorRequest`; the main loop — the only place that controls
  raw mode — suspends the TUI, runs `git commit` with inherited stdio, and
  restores. The input thread is paused via an `AtomicBool` so it cannot
  steal keystrokes destined for the editor (100 ms poll timeout + pause-flag
  check).

### Module layout

```
src/
├ main.rs        terminal setup/teardown, panic hook (restores raw mode),
│                event loop, $EDITOR handoff
├ app/           App state + the update half of the loop, split by concern
│  ├ mod.rs      App struct, AppEvent, update(), command dispatch table
│  ├ keys.rs     key routing (confirm > input > help > transient > keymap)
│  ├ dwim.rs     stage/unstage/discard/visit on the section at point
│  ├ search.rs   buffer-search state, incremental preview, n/p motion
│  ├ workers.rs  worker-thread git calls, refresh generations, process log
│  └ ops/        one module per transient menu (commit / branch / remote /
│                log); a new menu = a new file + one routing arm in ops/mod.rs
├ event.rs       input thread + .git watcher
├ command.rs     Command enum + name/description registry
├ keymap.rs      key-sequence trie, key-notation parser, layering
├ theme.rs       Theme (named color roles). Every UI color goes through here
├ config.rs      ~/.config/rugit/config.toml loading and merging
├ git/           ← TUI-independent. Pure parsers + shell-out
│  ├ client.rs   GitClient (spawn, forced env, read_snapshot)
│  ├ parse.rs    porcelain v2 / unified diff / log / stash parsers (pure)
│  ├ patch.rs    patch builders for hunk- and line-level staging (pure)
│  └ types.rs    StatusSnapshot, FileDiff, Hunk, DiffArea ...
└ ui/
   ├ section.rs  section tree + flattening + SectionId
   ├ pane.rs     cursor/scroll/folding/refresh-survival
   ├ build.rs    snapshot → tree builders (status / revision / log / process log)
   ├ transient.rs data-driven transient engine + menu definitions
   ├ input.rs    minibuffer input + picker (filtering selection)
   └ render.rs   drawing + overlays (input, transient, which-key, help, confirm)
```

`git/parse.rs`, `git/patch.rs`, `ui/section.rs`, and `ui/pane.rs` are pure
modules with no I/O, unit-testable against fixture strings alone. When the
crate is ever split into a workspace, `git/` becomes `rugit-core` as-is.

## 3. The heart: the section tree

The essence of Magit's UX: **every buffer is a tree of sections, and
commands act DWIM on the section at point**. Status, revision, and
process-log buffers all sit on the same foundation.

```rust
struct Section {
    id: SectionId,             // hash of parent_id + kind key; identity across refreshes
    value: SectionValue,       // what commands act on
    heading: Line<'static>,
    body: Vec<Line<'static>>,  // diff lines for hunks
    children: Vec<Section>,
    collapsed: bool,
}

enum SectionValue {
    Root,
    Group(Group),                       // Untracked / Unstaged / Staged / Stashes / Recent ...
    File { area: DiffArea, path: String },
    Hunk { area: DiffArea, path: String, hunk_idx: usize },
    Commit { hash: String },
    Stash { index: usize },
    Text,                               // inert content
}
```

Carrying `DiffArea` (Untracked / Unstaged / Staged / Committed) in the value
lets the same `s` key dispatch to the right git operation in every context.
The actual diff data (`FileDiff`) is owned per-area by the `Pane` and looked
up by `(area, path)` at dispatch time.

### The three key patterns

**(a) DWIM dispatch** — `s` (stage) is a single command that branches on the
`SectionValue` under the cursor:

- `File{Untracked|Unstaged}` → `git add -- <path>`
- `Hunk{Unstaged}` with the cursor on a `+`/`-` line → a partial patch
  containing **only that line**, applied with `git apply --cached`
- `Hunk{Unstaged}` elsewhere → the whole hunk as a patch via
  `git apply --cached`
- the `Group(Unstaged)` heading → `git add -u`
- anything staged → "already staged" message

**(b) State preservation across refreshes** — every git operation rebuilds
the tree from scratch, but collapse state and the cursor are transplanted by
matching `SectionId`s. The cursor is remembered **not as a raw line number
but as a section-ID chain plus a body offset**, restored with fallbacks in
order: same section same line → same section's heading → nearest surviving
ancestor's heading → clamped line index (same behavior as Magit).

**(c) The flattening cache** — for rendering and cursor movement, the
visible tree is flattened into a cached `Vec<FlatLine>` (each line carries
`path: Vec<usize>`, its child-index route from the root). It is recomputed
only on fold/refresh. Cursor movement, scrolling, and section jumps all
reduce to index arithmetic on this Vec.

**(d) Right margin** — a heading may carry an optional `margin: Line`
(currently only the log's author + relative date). Because right-alignment
needs the viewport width, the margin is *not* baked into the heading at
build time; it rides along on the `FlatLine` and `draw_pane` places it as a
fixed-width block flush to the right (one blank column kept at the far
edge), truncating the *content* — never the margin — so the columns line up
across rows regardless of subject length. The margin itself is built as two
sub-columns (author left-aligned, date right-aligned) sized to the widest of
each across the buffer, so both align vertically. All of this is measured in
**display columns** (`unicode-width`), never char counts, so full-width
(CJK) glyphs stay aligned and truncation never splits a wide glyph. This
keeps `build.rs` width-agnostic. The log also sets `compact` on its root so
its top-level commit sections render as one tight list instead of
blank-line-separated like status groups; ref decorations (`%D`) render as
color-coded tokens (`branch` / `branch-remote` / `tag` roles) rather than a
parenthesized blob.

## 4. Scroll management

With the flattening in place, scrolling reduces to a viewport over
`Vec<FlatLine>`.

- **Cursor-driven** as a rule. Even scroll-style commands (`C-d` etc.) only
  move the cursor; right before drawing, `follow()` clamps the viewport to
  keep the cursor visible with a scrolloff margin. This routes all
  post-refresh position restoration through the section-identity machinery
  (3-b).
- `follow(height, scrolloff)`: clamp `top` into
  `[cursor+margin+1-height, cursor-margin]`, bounded so the viewport never
  runs past the end. The margin is capped at `height/2`.
- Diffs are **not wrapped** — they truncate horizontally (same as Magit).
  Wrapping requires managing a "1 logical line = N display lines" mapping
  and was deliberately deferred.
- Rendering processes only the `flat[top .. top+height]` slice, so huge
  diffs stay O(screen size).

## 5. The keybinding system

Requirements: multi-key sequences (prefixes), per-buffer-kind maps,
remapping via the config file, which-key style hints.

### Commands are data, not closures

```rust
enum Command { Refresh, Stage, Nav(NavCmd), Transient(Menu), ... }
enum NavCmd  { MoveDown, HalfPageUp, NextSection, ... }  // pure cursor motions
enum Menu    { Commit, Branch, Push, Pull, Fetch, Log }  // transient menus
```

A keymap maps `KeyPress → Command` and holds no functions. This buys three
things at once: (1) remapping by name from TOML, (2) a help UI that can
enumerate command descriptions, (3) dispatch concentrated in one `match`,
easy to test. Metadata (name, description) lives in the static `COMMANDS`
table, one entry per leaf (`Nav(MoveDown)` is "move-down").

Command families whose handling is uniform are grouped into sub-enums so
`dispatch` stays a constant-size router: all of `NavCmd` forwards to
`Pane::navigate` in one arm, all of `Menu` resolves through
`transient::menu_def` in one arm. `App::update` follows the same rule —
every event arm is a one-line delegation into the owning submodule
(`app/workers.rs`, `app/keys.rs`, ...). Adding commands, menus, or events
must not grow either match beyond one routing arm per *family*.

### Trie and layering

```rust
struct Keymap(HashMap<KeyPress, Entry>);
enum Entry { Command(Command), Prefix(Keymap) }
```

- Input accumulates into `pending: Vec<KeyPress>` while walking the trie. A
  `Prefix` shows `P-` in the status line plus a which-key candidate panel; a
  `Command` dispatches. Unbound keys and ESC clear the pending state.
- Resolution order: **active transient (captures all keys) > buffer-local >
  global**.
- Key normalization absorbs terminal differences: SHIFT on character keys is
  folded into the character itself, Ctrl-chars are lowercased, Shift-Tab
  becomes `BackTab`.
- One parser for the `"C-x u"` notation serves both the built-in default
  keymap and the config file.

```toml
# ~/.config/rugit/config.toml
scrolloff = 3
[keys.global]
"g"   = "refresh"
[keys.status]
"P p" = "push"   # space-separated sequences

[colors]              # role names: see src/theme.rs
diff-add  = "green"   # color name / "#rrggbb" / 256-color index "42"
cursor-bg = "#3a3a3a"
```

### Color scheme

Every color the UI uses goes through a **named role field on `Theme`**
(`diff-add`, `hunk-header`, `cursor-bg`, `key`, `menu-title`, ...). The rule
that rendering code never hardcodes a color means the `[colors]` section can
restyle the whole application. Parsing is delegated to ratatui's
`Color::FromStr` (names, hex, 256-color indexes). Invalid values and unknown
role names become startup warnings.

## 6. Transients (popup menus)

Fully data-driven. Adding a menu means adding a definition; the engine is
shared.

```rust
struct TransientDef { title, groups: &[GroupDef] }
enum Item {
    Switch { key: "-a", flag: "--all", desc },        // boolean toggle
    Arg    { key: "-n", flag: "--max-count=", desc }, // takes a value
    Action { key: "c", desc, action: TransientAction },
}
```

- While open, a transient is the top layer of key resolution and captures
  every key. Switch/Arg keys like `-a` are two keystrokes, matched by prefix.
- Toggling a `Switch` mutates state and re-renders (enabled switches change
  color).
- Selecting an `Arg` returns `Prompt { flag, desc }`; the app opens a
  minibuffer (`InputPurpose::TransientArg(flag)`) *over* the still-open
  transient, and the submit writes the value back into `state.values`. The
  `flag` ends in `=` so `args()` emits a single `--author=ada` token; an empty
  submit clears it.
- Invoking an `Action` collects enabled switches + value args into a
  `Vec<String>` passed to execution (`git commit --amend --signoff`, ...).
- `TransientAction` is a separate enum from `Command`. Commit-family actions
  route to the editor handoff; push/pull/fetch run in the background; log
  actions append their rev selector and open a log buffer.

Current definitions: Commit (`-a/-e/-n/-s`; commit/amend/extend), Branch
(checkout / create+checkout / create), Push (`-f/-F/-n`; upstream /
set-upstream), Pull (`-r/-a`), Fetch (`-p`; upstream/all), Log
(`-n`/`-A`/`-F` values, `-m/-r/-f` switches; current / other / all
references). The log's `--graph` is intentionally excluded — it prefixes
graph art the `--format` field parser can't read (see §on the log buffer).

Not yet implemented: persisting switch/arg defaults across sessions.

## 6.5 Minibuffer input and the picker

When a transient action needs an argument (a checkout target, a new branch
name), it opens an `InputState`. One component, two modes:

- **plain input**: free text (e.g. a new branch name). Character-wise cursor
  editing (Left/Right/Home/End/Backspace/Delete/C-u). Multibyte-safe — the
  cursor is a char index.
- **picker**: providing `candidates` turns it into a filtering selector.
  The typed text filters with case-insensitive fuzzy matching (the
  `fuzzy-matcher` crate's skim algorithm, best score first), UP/DOWN
  (C-p/C-n) move the selection, TAB completes the selection into the
  input, RET submits the selected candidate — or, with zero matches, the
  raw typed text (which is how checking out a tag or SHA works).

Key-resolution priority: **confirm > input > help > transient > keymap**.
The submitted value is dispatched on `InputPurpose` (CheckoutRev /
CreateCheckoutBranch / CreateBranch) and turned into a git invocation.
Checkout rides `git checkout`'s DWIM (remote-tracking → create local branch,
tag → detach).

Branch candidates come from `git branch --all --format=%(refname:short)`.
Enumerating local refs is fast, so this is the one read done synchronously.

## 6.6 In-buffer search

Incremental, isearch-style. `/` opens a minibuffer (`InputPurpose::Search`).

- **Live preview**: every keystroke updates `app.search` → all matches are
  highlighted and the cursor jumps to the first match at or after the search
  origin (wrapping to the top if none). ESC restores the origin position.
- **Smart-case**: an all-lowercase query matches case-insensitively; any
  uppercase character makes it case-sensitive.
- **Modal switch after confirming**: RET makes the search "active", and
  `n`/`p` dispatch to **match navigation** (with wraparound) instead of
  section navigation. ESC deactivates the search and n/p revert. The keymap
  itself stays static; the switch is a match guard in `dispatch()`
  (`Command::NextSection if self.search.is_some()`) — modal behavior is kept
  out of the keymap layer by design.
- **Highlighting**: only the matched substrings get a background color (the
  `search-match` role). A per-char match mask is computed over the whole
  line text, and existing `Span`s are split at match boundaries and
  `Style::patch`ed (`highlight_query`) — surrounding styling such as diff
  colors survives. Multibyte stays aligned because everything is
  char-indexed.
- Matching is `Pane::find_matches` (an O(lines) scan over the flattened
  lines, no caching). The status bar's right side shows `/query (N)`.
- Search state persists across refreshes: only the query string is kept and
  matches are recomputed every time, so they can never go stale.

## 7. The git execution layer

- Forced flags/env: `git --no-pager --no-optional-locks`,
  `GIT_TERMINAL_PROMPT=0` (never hang on auth), `LC_ALL=C` (stable parsing).
- Every mutation's command line and output is recorded in a ring buffer
  (`Vec<ProcessEntry>`) viewable in the `$` process-log buffer. Transparency
  is a core Magit value.
- **Snapshot reads** happen in one pass: `status --porcelain=v2 --branch
  -z`, `diff`, `diff --cached`, `log -n10`, `stash list`. On an unborn
  branch (before the first commit) `diff --cached` cannot resolve HEAD, so
  it diffs against the **empty-tree constant SHA** (`4b825dc...`).
  Unstaging likewise falls back to `git rm --cached` when unborn.
- **Patch building** (`git/patch.rs`): a whole hunk is transcribed verbatim
  with its header. Line-level staging reconstructs a partial hunk — for the
  unselected lines, `+` is dropped (when staging) or turned into context
  (when unstaging), and `-` the reverse — then recounts and pipes into
  `git apply --recount --cached [-R]`. `\ No newline` markers survive only
  if the line they annotate survived. This is the only code path that can
  corrupt the index, so it carries roundtrip integration tests against real
  git.
- **Destructive operations** (discard, deleting untracked files) go through
  a y/n confirmation. Line-level discard is deliberately not offered (too
  easy to fat-finger); discard works at hunk granularity only.

## 8. Testing strategy

- **Unit**: parsers (porcelain v2 / diff / hunk headers), patch building
  (the line stage/unstage transform rules), keymaps (notation roundtrip,
  trie, shadowing), pane (cursor restoration, follow, toggle), transient
  (switch sequences, flag collection), input (editing, filtering,
  multibyte), search (match mask, span splitting). All pure functions —
  fixture strings suffice.
- **Integration** (`tests/git_integration.rs`): tempfile + the real git
  binary; covers unborn-branch snapshots, hunk stage/unstage roundtrips, and
  single-line staging.
- **Smoke**: drive the real binary in a PTY (`script`), feed keystrokes,
  and assert on captured frames and index state (not yet in CI).

## 9. Future work

- Word-level diff highlighting (`similar`), syntax highlighting (`syntect`,
  feature-flagged)
- Graph rendering in the log buffer (`--graph`), and inline diff expansion of
  a commit under point
- Region selection for multi-line staging; discarding directly from staged
- Transient for stash; rename/delete in the branch transient
- `Arg` transient items and persisted switch defaults
- Workspace split (`rugit-core` / `rugit-tui`); gix for the read side
