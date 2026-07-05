//! Background plumbing: git mutations and reads on worker threads, refresh
//! generations, and process-log bookkeeping.

use std::thread;

use crate::git::client::{display_cmd, ProcessEntry};
use crate::git::types::StatusSnapshot;
use crate::keymap::PaneKind;
use crate::ui::build;
use crate::ui::pane::Pane;

use super::{svec, App, AppEvent};

/// Pretty-format for log rows, parsed by `parse::parse_log_entries`.
const LOG_FORMAT: &str = "--format=%h%x1f%D%x1f%s%x1f%an%x1f%ar";
/// How many commits the log buffer fetches (matches Magit's default).
const LOG_LIMIT: &str = "256";

impl App {
    /// Run a git mutation on a worker thread; completion triggers a refresh.
    pub(super) fn run_git_bg(&mut self, desc: String, args: Vec<String>, stdin: Option<String>) {
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

    /// Refresh whatever the active buffer shows: re-run the log for a log
    /// pane, otherwise re-read the status snapshot.
    pub(super) fn refresh_current(&mut self) {
        if let Some(pane) = self.panes.last() {
            if pane.kind == PaneKind::Log {
                if let Some(args) = pane.log_args.clone() {
                    let title = pane.title.clone();
                    self.load_log(title, args, true);
                    return;
                }
            }
        }
        self.refresh();
    }

    /// Read a log on a worker thread. `rev_args` are everything after the
    /// format+limit: the transient options (e.g. `--no-merges`,
    /// `--author=ada`) followed by the revision selector (`HEAD`, `--all`, a
    /// branch). Stored on the pane so `g` reproduces the same query. `replace`
    /// refreshes the current log pane instead of opening a new one.
    pub(super) fn load_log(&mut self, title: String, rev_args: Vec<String>, replace: bool) {
        self.busy = Some(title.clone());
        let git = self.git.clone();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let mut args = svec(&["log", LOG_FORMAT, "-n", LOG_LIMIT]);
            args.extend(rev_args.iter().cloned());
            let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let entries = git
                .run(&arg_refs)
                .map(|o| crate::git::parse::parse_log_entries(&o.stdout))
                .unwrap_or_default();
            let _ = tx.send(AppEvent::LogReady {
                title,
                args: rev_args,
                entries,
                replace,
            });
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

    pub(super) fn on_snapshot(&mut self, gen: u64, result: Result<StatusSnapshot, String>) {
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

    pub(super) fn open_process_log(&mut self) {
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

    pub(super) fn refresh_process_log_pane(&mut self) {
        if let Some(pane) = self
            .panes
            .iter_mut()
            .find(|p| p.kind == PaneKind::ProcessLog)
        {
            pane.replace_tree(build::build_process_log(&self.theme, &self.process_log));
        }
    }
}
