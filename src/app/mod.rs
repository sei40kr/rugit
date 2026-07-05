//! Application state and the update half of the Elm-style loop. All git
//! mutations run on worker threads; results come back as `AppEvent`s.
//!
//! `App`'s behavior is split across submodules by concern: key routing in
//! `keys`, cursor DWIM in `dwim`, buffer search in `search`, background
//! plumbing in `workers`, and one module per transient menu under `ops`.
//! This file owns the state, the event loop entry (`update`) and the
//! command dispatch table.

mod dwim;
mod keys;
mod ops;
mod search;
mod workers;

pub use search::SearchState;

use crossbeam_channel::Sender;
use ratatui::crossterm::event::KeyEvent;

use crate::command::Command;
use crate::git::client::{GitClient, ProcessEntry};
use crate::git::types::{LogEntry, StatusSnapshot};
use crate::keymap::{KeyPress, Keymaps, PaneKind};
use crate::theme::Theme;
use crate::ui::build;
use crate::ui::input::InputState;
use crate::ui::pane::Pane;
use crate::ui::transient::{TransientState, BRANCH, COMMIT, FETCH, LOG, PULL, PUSH};

#[derive(Debug)]
#[allow(clippy::large_enum_variant)] // events are few and short-lived
pub enum AppEvent {
    Key(KeyEvent),
    Resize,
    /// A background snapshot read finished.
    SnapshotReady {
        gen: u64,
        result: Result<StatusSnapshot, String>,
    },
    /// A background git mutation finished.
    GitDone {
        desc: String,
        entry: ProcessEntry,
    },
    /// `git show` data for a revision buffer arrived.
    RevisionReady {
        title: String,
        header: String,
        diff: String,
    },
    /// `git log` data for a log buffer arrived. `replace` re-uses the current
    /// log pane (a refresh) instead of pushing a new one.
    LogReady {
        title: String,
        args: Vec<String>,
        entries: Vec<LogEntry>,
        replace: bool,
    },
    /// The fs watcher saw `.git` change.
    RepoChanged,
}

/// A destructive action awaiting y/n confirmation.
pub struct Confirm {
    pub prompt: String,
    pub action: PendingAction,
}

pub enum PendingAction {
    Git {
        desc: String,
        args: Vec<String>,
        stdin: Option<String>,
    },
    DeletePaths(Vec<String>),
}

/// `git commit` must run with the terminal handed over to $EDITOR; the main
/// loop performs this outside of raw mode.
pub struct EditorRequest {
    pub desc: String,
    pub args: Vec<String>,
}

pub struct App {
    pub git: GitClient,
    pub tx: Sender<AppEvent>,
    pub panes: Vec<Pane>,
    pub keymaps: Keymaps,
    pub theme: Theme,
    pub scrolloff: usize,
    pub pending: Vec<KeyPress>,
    pub transient: Option<TransientState>,
    pub input: Option<InputState>,
    pub confirm: Option<Confirm>,
    pub show_help: bool,
    /// Scroll offset of the help overlay; clamped to the content by render.
    pub help_scroll: usize,
    pub message: Option<String>,
    pub busy: Option<String>,
    pub process_log: Vec<ProcessEntry>,
    pub snapshot: Option<StatusSnapshot>,
    pub search: SearchState,
    pub should_quit: bool,
    editor_request: Option<EditorRequest>,
    refresh_gen: u64,
}

impl App {
    pub fn new(
        git: GitClient,
        tx: Sender<AppEvent>,
        keymaps: Keymaps,
        theme: Theme,
        scrolloff: usize,
    ) -> Self {
        let title = format!(
            "rugit: {}",
            git.repo_root
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        );
        let status = Pane::new(PaneKind::Status, title, crate::ui::section::Section::root());
        Self {
            git,
            tx,
            panes: vec![status],
            keymaps,
            theme,
            scrolloff,
            pending: Vec::new(),
            transient: None,
            input: None,
            confirm: None,
            show_help: false,
            help_scroll: 0,
            message: None,
            busy: None,
            process_log: Vec::new(),
            snapshot: None,
            search: SearchState::default(),
            should_quit: false,
            editor_request: None,
            refresh_gen: 0,
        }
    }

    pub fn take_editor_request(&mut self) -> Option<EditorRequest> {
        self.editor_request.take()
    }

    pub fn which_key_candidates(&self) -> Vec<(String, String)> {
        let kind = self
            .panes
            .last()
            .map(|p| p.kind)
            .unwrap_or(PaneKind::Status);
        self.keymaps.candidates(kind, &self.pending)
    }

    // ---- event handling ----------------------------------------------------

    pub fn update(&mut self, event: AppEvent) {
        match event {
            AppEvent::Key(ev) => self.on_key(ev),
            AppEvent::Resize => {}
            AppEvent::SnapshotReady { gen, result } => self.on_snapshot(gen, result),
            AppEvent::GitDone { desc, entry } => {
                self.busy = None;
                if entry.status != 0 {
                    let first = entry.output.lines().next().unwrap_or("").to_string();
                    self.message = Some(format!("{desc} failed: {first}"));
                } else {
                    self.message = Some(format!("{desc} done"));
                }
                self.process_log.push(entry);
                self.refresh_process_log_pane();
                self.refresh();
            }
            AppEvent::RevisionReady {
                title,
                header,
                diff,
            } => {
                self.busy = None;
                let files = crate::git::parse::parse_diff(&diff);
                let root = build::build_revision(&self.theme, &header, &files);
                let mut pane = Pane::new(PaneKind::Revision, title, root);
                pane.committed = files;
                self.panes.push(pane);
            }
            AppEvent::LogReady {
                title,
                args,
                entries,
                replace,
            } => {
                self.busy = None;
                let root = build::build_log(&self.theme, &title, &entries);
                let top_is_log = self.panes.last().map(|p| p.kind) == Some(PaneKind::Log);
                if replace && top_is_log {
                    if let Some(pane) = self.panes.last_mut() {
                        pane.title = title;
                        pane.log_args = Some(args);
                        pane.replace_tree(root);
                    }
                } else {
                    let mut pane = Pane::new(PaneKind::Log, title, root);
                    pane.log_args = Some(args);
                    self.panes.push(pane);
                }
            }
            AppEvent::RepoChanged => self.refresh(),
        }
    }

    // ---- command dispatch --------------------------------------------------

    fn dispatch(&mut self, cmd: Command) {
        let height = 40; // page motions use a nominal height; follow() clamps
        match cmd {
            Command::Quit => {
                if self.panes.len() > 1 {
                    self.panes.pop();
                } else {
                    self.should_quit = true;
                }
            }
            Command::Refresh => {
                self.refresh_current();
                self.message = Some("refreshing".into());
            }
            Command::MoveDown => self.pane_mut(|p| p.move_cursor(1)),
            Command::MoveUp => self.pane_mut(|p| p.move_cursor(-1)),
            Command::HalfPageDown => self.pane_mut(|p| p.move_cursor(height / 2)),
            Command::HalfPageUp => self.pane_mut(|p| p.move_cursor(-(height / 2))),
            Command::GotoTop => self.pane_mut(|p| p.goto_top()),
            Command::GotoBottom => self.pane_mut(|p| p.goto_bottom()),
            // While a search is active, n/p walk matches instead of sections.
            Command::NextSection if self.search.query.is_some() => self.search_move(1),
            Command::PrevSection if self.search.query.is_some() => self.search_move(-1),
            Command::NextSection => self.pane_mut(|p| p.next_section()),
            Command::PrevSection => self.pane_mut(|p| p.prev_section()),
            Command::ParentSection => self.pane_mut(|p| p.parent_section()),
            Command::ToggleSection => self.pane_mut(|p| p.toggle_at_cursor()),
            Command::Stage => self.stage_at_point(),
            Command::Unstage => self.unstage_at_point(),
            Command::StageAll => self.run_git_bg("stage all".into(), svec(&["add", "-u"]), None),
            Command::UnstageAll => self.unstage_all(),
            Command::Discard => self.discard_at_point(),
            Command::Visit => self.visit_at_point(),
            Command::Search => self.start_search(),
            Command::TransientCommit => self.transient = Some(TransientState::new(&COMMIT)),
            Command::TransientBranch => self.transient = Some(TransientState::new(&BRANCH)),
            Command::TransientPush => self.transient = Some(TransientState::new(&PUSH)),
            Command::TransientPull => self.transient = Some(TransientState::new(&PULL)),
            Command::TransientFetch => self.transient = Some(TransientState::new(&FETCH)),
            Command::TransientLog => self.transient = Some(TransientState::new(&LOG)),
            Command::Help => {
                self.show_help = true;
                self.help_scroll = 0;
            }
            Command::ProcessLog => self.open_process_log(),
        }
    }

    fn pane_mut(&mut self, f: impl FnOnce(&mut Pane)) {
        if let Some(p) = self.panes.last_mut() {
            f(p);
        }
    }
}

fn svec(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}
