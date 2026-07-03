//! Application state and the update half of the Elm-style loop. All git
//! mutations run on worker threads; results come back as `AppEvent`s.

use std::thread;

use crossbeam_channel::Sender;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind};

use crate::command::Command;
use crate::git::client::{display_cmd, GitClient, ProcessEntry};
use crate::git::patch::{self, LineOp};
use crate::git::types::{DiffArea, StatusSnapshot};
use crate::keymap::{normalize, KeyPress, Keymaps, Lookup, PaneKind};
use crate::theme::Theme;
use crate::ui::build;
use crate::ui::input::{InputPurpose, InputResult, InputState};
use crate::ui::pane::Pane;
use crate::ui::section::{Group, SectionValue};
use crate::ui::transient::{
    TransientAction, TransientResult, TransientState, BRANCH, COMMIT, FETCH, PULL, PUSH,
};

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
    DeletePath(String),
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
    /// Active buffer-search query. While set, matches are highlighted and
    /// n/p navigate matches instead of sections; ESC clears it.
    pub search: Option<String>,
    pub should_quit: bool,
    /// Cursor position when `/` was pressed, for restoring on cancel.
    search_origin: usize,
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
            search: None,
            should_quit: false,
            search_origin: 0,
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
            AppEvent::RepoChanged => self.refresh(),
        }
    }

    fn on_key(&mut self, ev: KeyEvent) {
        if ev.kind != KeyEventKind::Press {
            return;
        }
        let kp = normalize(&ev);
        self.message = None;

        if self.confirm.is_some() {
            self.on_confirm_key(&kp);
            return;
        }
        if let Some(input) = self.input.as_mut() {
            let purpose = input.purpose;
            match input.on_key(&kp) {
                InputResult::Consumed => {
                    // Incremental search reacts to every edit.
                    if purpose == InputPurpose::Search {
                        let query = input.text.clone();
                        self.search_preview(query);
                    }
                }
                InputResult::Cancel => {
                    self.input = None;
                    if purpose == InputPurpose::Search {
                        self.search = None;
                        let origin = self.search_origin;
                        self.pane_mut(|p| p.cursor = origin.min(p.line_count().saturating_sub(1)));
                    }
                    self.message = Some("aborted".into());
                }
                InputResult::Submit(value) => {
                    self.input = None;
                    self.on_input_submit(purpose, value);
                }
            }
            return;
        }
        if self.show_help {
            let ctrl = kp
                .mods
                .contains(ratatui::crossterm::event::KeyModifiers::CONTROL);
            match kp.code {
                KeyCode::Char('q') | KeyCode::Esc | KeyCode::Char('?') => self.show_help = false,
                KeyCode::Char('j') | KeyCode::Down => self.help_scroll += 1,
                KeyCode::Char('k') | KeyCode::Up => {
                    self.help_scroll = self.help_scroll.saturating_sub(1)
                }
                KeyCode::Char('d') if ctrl => self.help_scroll += 10,
                KeyCode::Char('u') if ctrl => {
                    self.help_scroll = self.help_scroll.saturating_sub(10)
                }
                KeyCode::PageDown => self.help_scroll += 10,
                KeyCode::PageUp => self.help_scroll = self.help_scroll.saturating_sub(10),
                KeyCode::Home | KeyCode::Char('g') => self.help_scroll = 0,
                // Clamped down to the last line by render.
                KeyCode::End | KeyCode::Char('G') => self.help_scroll = usize::MAX,
                _ => {}
            }
            return;
        }
        if let Some(transient) = self.transient.as_mut() {
            match transient.on_key(&kp) {
                TransientResult::Consumed => {}
                TransientResult::Cancel => self.transient = None,
                TransientResult::Unbound => {
                    self.message = Some("key not bound in this menu".into());
                }
                TransientResult::Invoke(action, args) => {
                    self.transient = None;
                    self.invoke_transient(action, args);
                }
            }
            return;
        }

        if kp.is_esc() {
            if !self.pending.is_empty() {
                self.pending.clear();
            } else if self.search.take().is_some() {
                self.message = Some("search cleared".into());
            }
            return;
        }
        self.pending.push(kp);
        let kind = self
            .panes
            .last()
            .map(|p| p.kind)
            .unwrap_or(PaneKind::Status);
        match self.keymaps.lookup(kind, &self.pending) {
            Lookup::Command(cmd) => {
                self.pending.clear();
                self.dispatch(cmd);
            }
            Lookup::Pending => {}
            Lookup::Unbound => {
                if self.pending.len() > 1 {
                    self.message = Some(format!(
                        "{} is undefined",
                        crate::keymap::format_keys(&self.pending)
                    ));
                }
                self.pending.clear();
            }
        }
    }

    fn on_confirm_key(&mut self, kp: &KeyPress) {
        let Some(confirm) = self.confirm.take() else {
            return;
        };
        if matches!(
            kp.code,
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter
        ) {
            match confirm.action {
                PendingAction::Git { desc, args, stdin } => self.run_git_bg(desc, args, stdin),
                PendingAction::DeletePath(path) => {
                    let full = self.git.repo_root.join(&path);
                    let result = if full.is_dir() {
                        std::fs::remove_dir_all(&full)
                    } else {
                        std::fs::remove_file(&full)
                    };
                    match result {
                        Ok(()) => self.message = Some(format!("deleted {path}")),
                        Err(e) => self.message = Some(format!("delete failed: {e}")),
                    }
                    self.refresh();
                }
            }
        } else {
            self.message = Some("aborted".into());
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
                self.refresh();
                self.message = Some("refreshing".into());
            }
            Command::MoveDown => self.pane_mut(|p| p.move_cursor(1)),
            Command::MoveUp => self.pane_mut(|p| p.move_cursor(-1)),
            Command::HalfPageDown => self.pane_mut(|p| p.move_cursor(height / 2)),
            Command::HalfPageUp => self.pane_mut(|p| p.move_cursor(-(height / 2))),
            Command::GotoTop => self.pane_mut(|p| p.goto_top()),
            Command::GotoBottom => self.pane_mut(|p| p.goto_bottom()),
            // While a search is active, n/p walk matches instead of sections.
            Command::NextSection if self.search.is_some() => self.search_move(1),
            Command::PrevSection if self.search.is_some() => self.search_move(-1),
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

    // ---- DWIM: stage / unstage / discard ------------------------------------

    fn stage_at_point(&mut self) {
        let Some(pane) = self.panes.last() else {
            return;
        };
        match pane.value_at_cursor() {
            SectionValue::File {
                area: DiffArea::Untracked | DiffArea::Unstaged,
                path,
            } => {
                self.run_git_bg(format!("stage {path}"), svec(&["add", "--", &path]), None);
            }
            SectionValue::Hunk {
                area: DiffArea::Unstaged,
                path,
                hunk_idx,
            } => {
                if let Some(patch) =
                    self.patch_at_point(DiffArea::Unstaged, &path, hunk_idx, LineOp::Stage)
                {
                    self.run_git_bg(
                        format!("stage hunk in {path}"),
                        svec(&["apply", "--cached", "--recount", "--whitespace=nowarn"]),
                        Some(patch),
                    );
                }
            }
            SectionValue::Group(Group::Untracked) => {
                let mut args = svec(&["add", "--"]);
                if let Some(s) = &self.snapshot {
                    args.extend(s.untracked.iter().cloned());
                }
                self.run_git_bg("stage untracked files".into(), args, None);
            }
            SectionValue::Group(Group::Unstaged) => {
                self.run_git_bg("stage all tracked".into(), svec(&["add", "-u"]), None);
            }
            SectionValue::File {
                area: DiffArea::Staged,
                ..
            }
            | SectionValue::Hunk {
                area: DiffArea::Staged,
                ..
            }
            | SectionValue::Group(Group::Staged) => {
                self.message = Some("already staged".into());
            }
            _ => self.message = Some("nothing to stage here".into()),
        }
    }

    fn unstage_at_point(&mut self) {
        let Some(pane) = self.panes.last() else {
            return;
        };
        match pane.value_at_cursor() {
            SectionValue::File {
                area: DiffArea::Staged,
                path,
            } => {
                let args = if self.head_exists() {
                    svec(&["restore", "--staged", "--", &path])
                } else {
                    svec(&["rm", "--cached", "-r", "-q", "--", &path])
                };
                self.run_git_bg(format!("unstage {path}"), args, None);
            }
            SectionValue::Hunk {
                area: DiffArea::Staged,
                path,
                hunk_idx,
            } => {
                if let Some(patch) =
                    self.patch_at_point(DiffArea::Staged, &path, hunk_idx, LineOp::Unstage)
                {
                    self.run_git_bg(
                        format!("unstage hunk in {path}"),
                        svec(&[
                            "apply",
                            "-R",
                            "--cached",
                            "--recount",
                            "--whitespace=nowarn",
                        ]),
                        Some(patch),
                    );
                }
            }
            SectionValue::Group(Group::Staged) => self.unstage_all(),
            SectionValue::File {
                area: DiffArea::Untracked | DiffArea::Unstaged,
                ..
            }
            | SectionValue::Hunk {
                area: DiffArea::Unstaged,
                ..
            } => {
                self.message = Some("not staged".into());
            }
            _ => self.message = Some("nothing to unstage here".into()),
        }
    }

    fn unstage_all(&mut self) {
        let args = if self.head_exists() {
            svec(&["restore", "--staged", "--", "."])
        } else {
            svec(&["rm", "--cached", "-r", "-q", "--", "."])
        };
        self.run_git_bg("unstage all".into(), args, None);
    }

    fn discard_at_point(&mut self) {
        let Some(pane) = self.panes.last() else {
            return;
        };
        match pane.value_at_cursor() {
            SectionValue::File {
                area: DiffArea::Untracked,
                path,
            } => {
                self.confirm = Some(Confirm {
                    prompt: format!("Delete untracked {path}?"),
                    action: PendingAction::DeletePath(path),
                });
            }
            SectionValue::File {
                area: DiffArea::Unstaged,
                path,
            } => {
                self.confirm = Some(Confirm {
                    prompt: format!("Discard changes to {path}?"),
                    action: PendingAction::Git {
                        desc: format!("discard {path}"),
                        args: svec(&["restore", "--", &path]),
                        stdin: None,
                    },
                });
            }
            SectionValue::Hunk {
                area: DiffArea::Unstaged,
                path,
                hunk_idx,
            } => {
                // Whole-hunk only: line-level discard is easy to fat-finger.
                let Some(pane) = self.panes.last() else {
                    return;
                };
                let Some(fd) = pane.find_file(DiffArea::Unstaged, &path) else {
                    return;
                };
                let Some(hunk) = fd.hunks.get(hunk_idx) else {
                    return;
                };
                let patch = patch::hunk_patch(fd, hunk);
                self.confirm = Some(Confirm {
                    prompt: format!("Discard this hunk in {path}?"),
                    action: PendingAction::Git {
                        desc: format!("discard hunk in {path}"),
                        args: svec(&["apply", "-R", "--recount", "--whitespace=nowarn"]),
                        stdin: Some(patch),
                    },
                });
            }
            SectionValue::File {
                area: DiffArea::Staged,
                ..
            }
            | SectionValue::Hunk {
                area: DiffArea::Staged,
                ..
            } => {
                self.message = Some("unstage first, then discard".into());
            }
            _ => self.message = Some("nothing to discard here".into()),
        }
    }

    /// Build the patch for the hunk at point: single-line when the cursor is
    /// on a `+`/`-` body line, the whole hunk otherwise.
    fn patch_at_point(
        &self,
        area: DiffArea,
        path: &str,
        hunk_idx: usize,
        op: LineOp,
    ) -> Option<String> {
        let pane = self.panes.last()?;
        let fd = pane.find_file(area, path)?;
        let hunk = fd.hunks.get(hunk_idx)?;
        let cur = pane.current()?;
        if let Some(line_idx) = cur.body_idx.filter(|_| !cur.is_heading) {
            if let Some(p) = patch::line_patch(fd, hunk, line_idx, op) {
                return Some(p);
            }
        }
        Some(patch::hunk_patch(fd, hunk))
    }

    fn visit_at_point(&mut self) {
        let Some(pane) = self.panes.last() else {
            return;
        };
        let rev = match pane.value_at_cursor() {
            SectionValue::Commit { hash } => hash,
            SectionValue::Stash { index } => format!("stash@{{{index}}}"),
            _ => {
                self.message = Some("nothing to visit here".into());
                return;
            }
        };
        self.busy = Some(format!("loading {rev}"));
        let git = self.git.clone();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let header = git
                .run(&["show", "--no-patch", "--format=medium", &rev])
                .map(|o| o.stdout)
                .unwrap_or_default();
            let diff = git
                .run(&["show", "--format=", "--patch", "--no-ext-diff", &rev])
                .map(|o| o.stdout)
                .unwrap_or_default();
            let _ = tx.send(AppEvent::RevisionReady {
                title: rev,
                header,
                diff,
            });
        });
    }

    // ---- transients ----------------------------------------------------------

    fn invoke_transient(&mut self, action: TransientAction, mut args: Vec<String>) {
        match action {
            TransientAction::Commit => {
                let mut a = svec(&["commit"]);
                a.append(&mut args);
                self.editor_request = Some(EditorRequest {
                    desc: "commit".into(),
                    args: a,
                });
            }
            TransientAction::CommitAmend => {
                let mut a = svec(&["commit", "--amend"]);
                a.append(&mut args);
                self.editor_request = Some(EditorRequest {
                    desc: "amend".into(),
                    args: a,
                });
            }
            TransientAction::CommitExtend => {
                let mut a = svec(&["commit", "--amend", "--no-edit"]);
                a.append(&mut args);
                self.run_git_bg("extend commit".into(), a, None);
            }
            TransientAction::Push => {
                let mut a = svec(&["push"]);
                a.append(&mut args);
                self.run_git_bg("push".into(), a, None);
            }
            TransientAction::PushSetUpstream => {
                let mut a = svec(&["push", "-u", "origin", "HEAD"]);
                a.append(&mut args);
                self.run_git_bg("push (set upstream)".into(), a, None);
            }
            TransientAction::Pull => {
                let mut a = svec(&["pull"]);
                a.append(&mut args);
                self.run_git_bg("pull".into(), a, None);
            }
            TransientAction::Fetch => {
                let mut a = svec(&["fetch"]);
                a.append(&mut args);
                self.run_git_bg("fetch".into(), a, None);
            }
            TransientAction::FetchAll => {
                let mut a = svec(&["fetch", "--all"]);
                a.append(&mut args);
                self.run_git_bg("fetch --all".into(), a, None);
            }
            TransientAction::Checkout => {
                self.input = Some(InputState::picker(
                    "Checkout",
                    InputPurpose::CheckoutRev,
                    self.list_branches(),
                ));
            }
            TransientAction::CreateCheckoutBranch => {
                self.input = Some(InputState::plain(
                    "Create and checkout branch",
                    InputPurpose::CreateCheckoutBranch,
                ));
            }
            TransientAction::CreateBranch => {
                self.input = Some(InputState::plain(
                    "Create branch",
                    InputPurpose::CreateBranch,
                ));
            }
        }
    }

    // ---- buffer search ---------------------------------------------------------

    fn start_search(&mut self) {
        self.search_origin = self.panes.last().map(|p| p.cursor).unwrap_or(0);
        self.input = Some(InputState::plain("Search", InputPurpose::Search));
    }

    /// Live update while the search input is open: highlight matches and jump
    /// to the first one at or after where the search started.
    fn search_preview(&mut self, query: String) {
        if query.is_empty() {
            self.search = None;
            let origin = self.search_origin;
            self.pane_mut(|p| p.cursor = origin.min(p.line_count().saturating_sub(1)));
            return;
        }
        self.search = Some(query.clone());
        let origin = self.search_origin;
        let Some(pane) = self.panes.last_mut() else {
            return;
        };
        let matches = pane.find_matches(&query);
        pane.cursor = matches
            .iter()
            .copied()
            .find(|&i| i >= origin)
            .or(matches.first().copied())
            .unwrap_or(origin);
    }

    /// n/p while a search is active: jump to the next/previous match,
    /// wrapping around the buffer.
    fn search_move(&mut self, dir: isize) {
        let Some(query) = self.search.clone() else {
            return;
        };
        let Some(pane) = self.panes.last_mut() else {
            return;
        };
        let matches = pane.find_matches(&query);
        if matches.is_empty() {
            self.message = Some(format!("no matches for \"{query}\""));
            return;
        }
        let cur = pane.cursor;
        let (next, wrapped) = if dir > 0 {
            match matches.iter().copied().find(|&i| i > cur) {
                Some(i) => (i, false),
                None => (matches[0], true),
            }
        } else {
            match matches.iter().rev().copied().find(|&i| i < cur) {
                Some(i) => (i, false),
                None => (*matches.last().unwrap(), true),
            }
        };
        pane.cursor = next;
        if wrapped {
            self.message = Some(if dir > 0 {
                "wrapped to top".into()
            } else {
                "wrapped to bottom".into()
            });
        }
    }

    // ---- minibuffer input ------------------------------------------------------

    fn on_input_submit(&mut self, purpose: InputPurpose, value: String) {
        if purpose == InputPurpose::Search {
            if value.is_empty() {
                self.search = None;
            } else if let Some(pane) = self.panes.last() {
                let n = pane.find_matches(&value).len();
                self.message = Some(format!("{n} match(es) — n/p to navigate, ESC to clear"));
            }
            return;
        }
        if value.is_empty() {
            self.message = Some("empty input".into());
            return;
        }
        match purpose {
            // `git checkout` DWIMs: local branch, remote-tracking branch
            // (creates a local branch), tag or raw revision (detaches).
            InputPurpose::CheckoutRev => {
                self.run_git_bg(
                    format!("checkout {value}"),
                    svec(&["checkout", &value]),
                    None,
                );
            }
            InputPurpose::CreateCheckoutBranch => {
                self.run_git_bg(
                    format!("create+checkout {value}"),
                    svec(&["checkout", "-b", &value]),
                    None,
                );
            }
            InputPurpose::CreateBranch => {
                self.run_git_bg(
                    format!("create branch {value}"),
                    svec(&["branch", &value]),
                    None,
                );
            }
            InputPurpose::Search => unreachable!("handled by the early return above"),
        }
    }

    /// Local and remote-tracking branch names for the checkout picker.
    /// Listing refs is a fast local read, so this runs synchronously.
    fn list_branches(&self) -> Vec<String> {
        let out = self
            .git
            .run(&["branch", "--all", "--format=%(refname:short)"])
            .ok();
        let mut seen = std::collections::BTreeSet::new();
        out.map(|o| o.stdout)
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.ends_with("HEAD") && !l.contains("HEAD detached"))
            .filter(|l| seen.insert(l.to_string()))
            .map(str::to_string)
            .collect()
    }

    // ---- git plumbing ----------------------------------------------------------

    fn head_exists(&self) -> bool {
        self.snapshot
            .as_ref()
            .map(|s| s.branch.oid.is_some())
            .unwrap_or(true)
    }

    /// Run a git mutation on a worker thread; completion triggers a refresh.
    fn run_git_bg(&mut self, desc: String, args: Vec<String>, stdin: Option<String>) {
        self.busy = Some(desc.clone());
        let git = self.git.clone();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let result = match &stdin {
                Some(input) => git.run_with_input(&arg_refs, input),
                None => git.run(&arg_refs),
            };
            let entry = match result {
                Ok(out) => ProcessEntry {
                    cmd: display_cmd(&args),
                    status: out.status,
                    output: format!("{}{}", out.stdout, out.stderr),
                },
                Err(e) => ProcessEntry {
                    cmd: display_cmd(&args),
                    status: -1,
                    output: e.to_string(),
                },
            };
            let _ = tx.send(AppEvent::GitDone { desc, entry });
        });
    }

    /// Kick off a background status snapshot read.
    pub fn refresh(&mut self) {
        self.refresh_gen += 1;
        let gen = self.refresh_gen;
        let git = self.git.clone();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = git.read_snapshot().map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::SnapshotReady { gen, result });
        });
    }

    fn on_snapshot(&mut self, gen: u64, result: Result<StatusSnapshot, String>) {
        if gen != self.refresh_gen {
            return; // stale — a newer refresh is already in flight
        }
        match result {
            Ok(snapshot) => {
                let root = build::build_status(&self.theme, &snapshot);
                if let Some(pane) = self.panes.iter_mut().find(|p| p.kind == PaneKind::Status) {
                    pane.replace_tree(root);
                    pane.unstaged = snapshot.unstaged.clone();
                    pane.staged = snapshot.staged.clone();
                }
                self.snapshot = Some(snapshot);
            }
            Err(e) => self.message = Some(format!("refresh failed: {e}")),
        }
    }

    /// The editor ran in the foreground; record the result and refresh.
    pub fn on_editor_done(&mut self, desc: String, args: Vec<String>, status: i32) {
        self.process_log.push(ProcessEntry {
            cmd: display_cmd(&args),
            status,
            output: String::new(), // stdio was inherited by the editor
        });
        self.message = Some(if status == 0 {
            format!("{desc} done")
        } else {
            format!("{desc} exited with {status}")
        });
        self.refresh_process_log_pane();
        self.refresh();
    }

    fn open_process_log(&mut self) {
        if self.panes.last().map(|p| p.kind) == Some(PaneKind::ProcessLog) {
            return;
        }
        let root = build::build_process_log(&self.theme, &self.process_log);
        self.panes.push(Pane::new(
            PaneKind::ProcessLog,
            "git process log".into(),
            root,
        ));
    }

    fn refresh_process_log_pane(&mut self) {
        if let Some(pane) = self
            .panes
            .iter_mut()
            .find(|p| p.kind == PaneKind::ProcessLog)
        {
            pane.replace_tree(build::build_process_log(&self.theme, &self.process_log));
        }
    }
}

fn svec(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}
