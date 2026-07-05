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

use crate::command::{Command, NavCmd};
use crate::git::client::{GitClient, ProcessEntry};
use crate::git::types::{LogEntry, StatusSnapshot};
use crate::keymap::{KeyPress, Keymaps, PaneKind};
use crate::theme::Theme;
use crate::ui::input::InputState;
use crate::ui::pane::Pane;
use crate::ui::transient::TransientState;

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
    /// Extra environment for the git process (e.g. `GIT_SEQUENCE_EDITOR`
    /// when launching a rebase whose todo the app already wrote).
    pub envs: Vec<(String, String)>,
}

impl EditorRequest {
    pub fn new(desc: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            desc: desc.into(),
            args,
            envs: Vec::new(),
        }
    }
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

    /// Pure router: every arm is a one-line delegation. Event bodies live in
    /// the submodule that owns the concern (see the module docs).
    pub fn update(&mut self, event: AppEvent) {
        match event {
            AppEvent::Key(ev) => self.on_key(ev),
            AppEvent::Resize => {}
            AppEvent::SnapshotReady { gen, result } => self.on_snapshot(gen, result),
            AppEvent::GitDone { desc, entry } => self.on_git_done(desc, entry),
            AppEvent::RevisionReady {
                title,
                header,
                diff,
            } => self.on_revision_ready(title, header, diff),
            AppEvent::LogReady {
                title,
                args,
                entries,
                replace,
            } => self.on_log_ready(title, args, entries, replace),
            AppEvent::RepoChanged => self.refresh(),
        }
    }

    // ---- command dispatch --------------------------------------------------

    /// Pure router, like `update`: an arm that grows a body gets extracted
    /// into a submodule method.
    fn dispatch(&mut self, cmd: Command) {
        let height = 40; // page motions use a nominal height; follow() clamps
        match cmd {
            Command::Quit => self.quit_or_pop(),
            Command::Refresh => {
                self.refresh_current();
                self.message = Some("refreshing".into());
            }
            // While a search is active, n/p walk matches instead of sections.
            Command::Nav(NavCmd::NextSection) if self.search.query.is_some() => self.search_move(1),
            Command::Nav(NavCmd::PrevSection) if self.search.query.is_some() => {
                self.search_move(-1)
            }
            Command::Nav(nav) => self.pane_mut(|p| p.navigate(nav, height)),
            Command::ToggleSection => self.pane_mut(|p| p.toggle_at_cursor()),
            Command::Stage => self.stage_at_point(),
            Command::Unstage => self.unstage_at_point(),
            Command::StageAll => self.run_git_bg("stage all".into(), svec(&["add", "-u"]), None),
            Command::UnstageAll => self.unstage_all(),
            Command::Discard => self.discard_at_point(),
            Command::Visit => self.visit_at_point(),
            Command::Search => self.start_search(),
            Command::Transient(menu) => self.open_transient(menu),
            Command::Todo(cmd) => self.todo_command(cmd),
            Command::Help => {
                self.show_help = true;
                self.help_scroll = 0;
            }
            Command::ProcessLog => self.open_process_log(),
        }
    }

    fn quit_or_pop(&mut self) {
        if self.panes.len() > 1 {
            self.panes.pop();
        } else {
            self.should_quit = true;
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
