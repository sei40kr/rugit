# rugit

A standalone [Magit](https://magit.vc/)-style Git TUI, written in Rust.

rugit brings Magit's core interaction model to a terminal application that
does not depend on Emacs: a status buffer made of foldable sections, commands
that act on the thing at point, and transient menus with toggleable
command-line switches. Git operations shell out to your real `git` binary, so
hooks, commit signing, credential helpers, and your git config all behave
exactly as they do on the command line.

> **Status**: early (v0.1). The core workflow — stage/unstage/discard,
> commit, branch, merge, rebase (including interactive), stash,
> push/pull/fetch, log and revision views, search — works. See
> [Roadmap](#roadmap) for what's missing.

## Features

- **Magit-style status buffer** — untracked/unstaged/staged files with inline
  diffs, stashes, and recent commits as a tree of collapsible sections
- **Stage at any granularity** — `s` stages whatever is at point: a file, a
  hunk, or a **single diff line**; `u` unstages the same way
- **Transient menus** — commit, branch, merge, rebase, cherry-pick, revert,
  reset, stash, tag, remote, push, pull, fetch, and log popups with
  Magit-style switches (`-a` → `--all`, `-f` → `--force-with-lease`, ...),
  value arguments (`--author=...`), and git-config variables
- **References buffer** — `y` lists branches, remote-tracking branches, and
  tags in foldable groups, each showing its upstream tracking state
- **Pickers and prompts** — checkout from a filterable branch picker; type a
  name to create branches; unmatched picker input is passed through, so tags
  and SHAs work too
- **Incremental search** — `/` with live highlighting and smart-case; `n`/`N`
  jump between matches
- **Log & revision views** — `l` opens the log menu; `RET` on a commit or
  stash opens its diff
- **Copy to clipboard** — `y s` copies the value at point (a commit, ref,
  stash, or file path) and `y b` the buffer's revision; a real clipboard tool
  (wl-copy / xclip / xsel / pbcopy) is used when available so failures are
  reported, else OSC 52 (works over SSH)
- **Real git, transparently** — every command rugit runs is logged to a
  process buffer (`` ` ``); commit messages open in your `$GIT_EDITOR`
- **Auto-refresh** — the status buffer updates when `.git` changes, even from
  other terminals
- **Configurable** — remap any key, restyle every color, all from one TOML
  file

## Installation

Requires a Rust toolchain and a `git` binary on `$PATH`.

```sh
cargo install --git https://github.com/sei40kr/rugit
# or from a checkout:
cargo install --path .
```

## Usage

Run `rugit` anywhere inside a git repository.

### Default key bindings

The bindings are vim-flavored: `j`/`k` move, `g` and `z` are prefixes,
`n`/`N` step through search matches.

| Key | Action |
|---|---|
| `j` / `k`, arrows | move cursor |
| `C-j` / `C-k` (also `g j` / `g k`, `z j` / `z k`) | next / previous section |
| `g h` or `^` | parent section |
| `TAB` (also `z a`) | collapse / expand section |
| `z A` | toggle section recursively |
| `z o` / `z c` | open / close section (`z c` again closes the parent) |
| `z O` / `z C` | open / close section recursively |
| `z r` / `z m` | open / close one fold level |
| `z R` / `z M` | open / close all sections |
| `C-d` / `C-u`, `PgDn` / `PgUp` | scroll half page |
| `C-f` / `C-b` | scroll full page |
| `g g` / `G`, `Home` / `End` | go to top / bottom |
| `g r` | refresh |
| `s` / `u` | stage / unstage the thing at point (file, hunk, or line) |
| `S` / `U` | stage all tracked / unstage all |
| `x` | discard the change at point (with confirmation) |
| `RET` | show the commit / stash at point |
| `y r` | references buffer (branches, remotes, tags) |
| `y s` / `y b` | copy value at point / buffer revision to clipboard |
| `/` | incremental search (`RET` to confirm, `ESC` to clear) |
| `n` / `N` | next / previous search match |
| `` ` `` | git process log |
| `?` | help (scrollable) |
| `q` | close buffer / quit |

Each of these opens a transient menu:

| Key | Menu |
|---|---|
| `c` | commit |
| `b` | branch |
| `m` | merge |
| `r` | rebase |
| `A` | cherry-pick |
| `_` | revert |
| `O` | reset |
| `Z` | stash |
| `t` | tag |
| `M` | remote |
| `p` / `P` | push |
| `F` | pull |
| `f` | fetch |
| `l` | log |
| `o` | submodule |
| `W` | worktree |

Inside a transient menu, keys like `-a` toggle switches and highlighted
actions run with the enabled flags.

During an interactive rebase, the todo buffer has its own keys:
`p` / `r` / `e` / `s` / `f` / `d` set the action for the commit at point
(pick / reword / edit / squash / fixup / drop), `M-j` / `M-k` move it, and
`C-c C-c` / `C-c C-k` confirm or abort the rebase.

## Configuration

`~/.config/rugit/config.toml` (`$XDG_CONFIG_HOME` is respected). Everything
is optional; invalid entries produce a startup warning instead of an error.

```toml
scrolloff = 3

[keys.global]
"g r" = "refresh"     # space-separated key sequences are supported
"P p" = "push"

[keys.status]
"s" = "stage"

[colors]               # role names: see src/theme.rs
diff-add     = "green" # color names, "#rrggbb", or 256-color indexes ("42")
cursor-bg    = "#3a3a3a"
search-match-bg = "magenta"   # bg of search matches
search-match-fg = "lightcyan" # fg of search matches
```

Command names for remapping are listed in the help buffer (`?`).

## Design

The architecture — section trees, the event loop, the keymap trie, the
transient engine, and why rugit shells out to git instead of using libgit2 —
is documented in [DESIGN.md](DESIGN.md). A contributor-oriented cheat sheet
lives in [CLAUDE.md](CLAUDE.md).

## Roadmap

- Region selection for multi-line staging
- Word-level diff highlighting and syntax highlighting
- Persisted transient switch defaults

## License

MIT
