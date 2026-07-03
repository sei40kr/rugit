# rugit

A standalone [Magit](https://magit.vc/)-style Git TUI, written in Rust.

rugit brings Magit's core interaction model to a terminal application that
does not depend on Emacs: a status buffer made of foldable sections, commands
that act on the thing at point, and transient menus with toggleable
command-line switches. Git operations shell out to your real `git` binary, so
hooks, commit signing, credential helpers, and your git config all behave
exactly as they do on the command line.

> **Status**: early (v0.1). The core workflow — stage/unstage/discard,
> commit, branch, push/pull/fetch, revision view, search — works. See
> [Roadmap](#roadmap) for what's missing.

## Features

- **Magit-style status buffer** — untracked/unstaged/staged files with inline
  diffs, stashes, and recent commits as a tree of collapsible sections
- **Stage at any granularity** — `s` stages whatever is at point: a file, a
  hunk, or a **single diff line**; `u` unstages the same way
- **Transient menus** — commit / branch / push / pull / fetch popups with
  Magit-style switches (`-a` → `--all`, `-f` → `--force-with-lease`, ...)
- **Pickers and prompts** — checkout from a filterable branch picker; type a
  name to create branches; unmatched picker input is passed through, so tags
  and SHAs work too
- **Incremental search** — `/` with live highlighting and smart-case; `n`/`p`
  jump between matches
- **Revision & stash view** — `RET` on a commit or stash opens its diff
- **Real git, transparently** — every command rugit runs is logged to a
  process buffer (`$`); commit messages open in your `$GIT_EDITOR`
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

| Key | Action |
|---|---|
| `j` / `k`, arrows | move cursor |
| `n` / `p` | next / previous section (match, while a search is active) |
| `^` | parent section |
| `TAB` | collapse / expand section |
| `C-d` / `C-u`, `PgDn` / `PgUp` | scroll half page |
| `g` | refresh |
| `s` / `u` | stage / unstage the thing at point (file, hunk, or line) |
| `S` / `U` | stage all tracked / unstage all |
| `x` | discard the change at point (with confirmation) |
| `RET` | show the commit / stash at point |
| `c` | commit menu |
| `b` | branch menu (checkout picker, create) |
| `P` / `F` / `f` | push / pull / fetch menus |
| `/` | incremental search (`RET` to confirm, `ESC` to clear) |
| `$` | git process log |
| `?` | help (scrollable) |
| `q` | close buffer / quit |

Inside a transient menu, keys like `-a` toggle switches and highlighted
actions run with the enabled flags.

## Configuration

`~/.config/rugit/config.toml` (`$XDG_CONFIG_HOME` is respected). Everything
is optional; invalid entries produce a startup warning instead of an error.

```toml
scrolloff = 3

[keys.global]
"g"   = "refresh"
"P p" = "push"        # space-separated key sequences are supported

[keys.status]
"s" = "stage"

[colors]               # role names: see src/theme.rs
diff-add     = "green" # color names, "#rrggbb", or 256-color indexes ("42")
cursor-bg    = "#3a3a3a"
search-match = "yellow"
```

Command names for remapping are listed in the help buffer (`?`).

## Design

The architecture — section trees, the event loop, the keymap trie, the
transient engine, and why rugit shells out to git instead of using libgit2 —
is documented in [DESIGN.md](DESIGN.md). A contributor-oriented cheat sheet
lives in [CLAUDE.md](CLAUDE.md).

## Roadmap

- Log buffer
- Rebase / stash / merge transients; branch rename & delete
- Region selection for multi-line staging
- Word-level diff highlighting and syntax highlighting
- Value-taking transient arguments (`--set-upstream=<value>`) and persisted
  switch defaults

## License

MIT
