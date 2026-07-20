//! Background plumbing: git mutations and reads on worker threads, refresh
//! generations, and process-log bookkeeping.

use std::thread;

use crate::git::client::{display_cmd, ProcessEntry};
use crate::git::types::{LogEntry, RefEntry, StatusSnapshot};
use crate::keymap::PaneKind;
use crate::ui::build;
use crate::ui::pane::Pane;

use super::{svec, App, AppEvent};

/// Rewrite the index's stat cache after a mutation, still on the worker
/// thread. Reads run with `--no-optional-locks` and can never persist it, so
/// once stat data goes stale (an external checkout, `touch`-ed files) every
/// status/diff would re-hash the same files on every refresh — the dominant
/// cost on a large worktree. This is a deliberate index write; the watcher
/// event it triggers folds into the single-flight refresh. Exit status is
/// meaningless here (1 just means some paths still differ).
pub(super) fn refresh_index_stat_cache(git: &crate::git::client::GitClient) {
    let _ = git.run(&["update-index", "-q", "--refresh", "--unmerged"]);
}

/// Pretty-format for log rows, parsed by `parse::parse_log_entries`.
const LOG_FORMAT: &str = "--format=%h%x1f%D%x1f%s%x1f%an%x1f%ar";
/// How many commits the log buffer fetches (matches Magit's default).
const LOG_LIMIT: &str = "256";
/// Entries kept in the `$` process log. Each keeps its command's full
/// output, so an unbounded log would grow for the whole session.
const PROCESS_LOG_MAX: usize = 200;

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
            refresh_index_stat_cache(&git);
            let _ = tx.send(AppEvent::GitDone { desc, entry });
        });
    }

    /// Run several git commands in sequence on a worker thread, stopping at
    /// the first failure. They land in the process log as one entry (the
    /// commands joined with `&&`) — for compound operations like absorb
    /// (merge, then delete the branch).
    pub(super) fn run_git_seq_bg(&mut self, desc: String, cmds: Vec<Vec<String>>) {
        self.busy = Some(desc.clone());
        let git = self.git.clone();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let mut status = 0;
            let mut output = String::new();
            let mut ran = Vec::new();
            for args in &cmds {
                ran.push(display_cmd(args));
                let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
                match git.run(&arg_refs) {
                    Ok(out) => {
                        output.push_str(&out.stdout);
                        output.push_str(&out.stderr);
                        status = out.status;
                    }
                    Err(e) => {
                        output.push_str(&e.to_string());
                        status = -1;
                    }
                }
                if status != 0 {
                    break;
                }
            }
            let entry = ProcessEntry {
                cmd: ran.join(" && "),
                status,
                output,
            };
            refresh_index_stat_cache(&git);
            let _ = tx.send(AppEvent::GitDone { desc, entry });
        });
    }

    /// Message shown while an explicit refresh runs; the completion
    /// handlers clear it (a mutation's "... done" message replaces it, so
    /// only the literal marker is ever cleared).
    const REFRESHING: &'static str = "refreshing";

    /// Clear the "refreshing" message once the refreshed data arrived.
    fn clear_refreshing(&mut self) {
        if self.message.as_deref() == Some(Self::REFRESHING) {
            self.message = None;
        }
    }

    /// Refresh whatever the active buffer shows: re-run the log for a log
    /// pane, otherwise re-read the status snapshot. This is the explicit
    /// (user-triggered) refresh, so it announces itself.
    pub(super) fn refresh_current(&mut self) {
        self.message = Some(Self::REFRESHING.into());
        if let Some(pane) = self.panes.last() {
            match pane.kind {
                PaneKind::Log => {
                    if let Some(args) = pane.log_args.clone() {
                        let title = pane.title.clone();
                        self.load_log(title, args, true);
                        return;
                    }
                }
                PaneKind::Refs => {
                    self.show_refs();
                    return;
                }
                _ => {}
            }
        }
        self.refresh();
    }

    /// Ref-row fields for `git for-each-ref`, parsed by `parse::parse_refs`.
    const REF_FORMAT: &'static str = "--format=%(refname)\x1f%(objectname:short)\x1f%(HEAD)\x1f%(upstream:short)\x1f%(upstream:track)\x1f%(contents:subject)";

    /// Read every branch/remote/tag on a worker thread and open (or refresh)
    /// the references buffer.
    pub(super) fn show_refs(&mut self) {
        self.busy = Some("references".into());
        let git = self.git.clone();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let entries = git
                .run(&[
                    "for-each-ref",
                    Self::REF_FORMAT,
                    "--sort=-creatordate",
                    "refs/heads",
                    "refs/remotes",
                    "refs/tags",
                ])
                .map(|o| crate::git::parse::parse_refs(&o.stdout))
                .unwrap_or_default();
            let _ = tx.send(AppEvent::RefsReady { entries });
        });
    }

    /// References data arrived: open a refs buffer, or refresh the current one.
    pub(super) fn on_refs_ready(&mut self, entries: Vec<RefEntry>) {
        self.busy = None;
        self.clear_refreshing();
        let root = build::build_refs(&self.theme, &entries);
        if self.panes.last().map(|p| p.kind) == Some(PaneKind::Refs) {
            if let Some(pane) = self.panes.last_mut() {
                pane.replace_tree(root);
            }
        } else {
            self.panes
                .push(Pane::new(PaneKind::Refs, "References".into(), root));
        }
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
    ///
    /// Single-flight: while a read is in flight, further requests only mark
    /// the state dirty and one follow-up read runs when it completes.
    /// Without this, watcher events arriving faster than a large repo can
    /// be scanned pile up concurrent scans that slow each other down.
    pub fn refresh(&mut self) {
        if self.refresh_inflight {
            self.refresh_dirty = true;
            return;
        }
        self.refresh_inflight = true;
        self.refresh_gen += 1;
        let gen = self.refresh_gen;
        let git = self.git.clone();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = git.read_snapshot().map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::SnapshotReady { gen, result });
        });
    }

    /// A background git mutation finished: record it and refresh.
    pub(super) fn on_git_done(&mut self, desc: String, entry: ProcessEntry) {
        self.busy = None;
        if entry.status != 0 {
            let first = entry.output.lines().next().unwrap_or("").to_string();
            self.message = Some(format!("{desc} failed: {first}"));
        } else {
            self.message = Some(format!("{desc} done"));
        }
        self.push_process_entry(entry);
        self.refresh_process_log_pane();
        self.refresh();
    }

    /// `git show` data arrived: open a revision buffer.
    pub(super) fn on_revision_ready(&mut self, title: String, header: String, diff: String) {
        self.busy = None;
        let files = crate::git::parse::parse_diff(&diff);
        let root = build::build_revision(&self.theme, &header, &files);
        let mut pane = Pane::new(PaneKind::Revision, title, root);
        pane.committed = files;
        self.panes.push(pane);
    }

    /// `git log` data arrived: open a log buffer, or refresh the current one.
    pub(super) fn on_log_ready(
        &mut self,
        title: String,
        args: Vec<String>,
        entries: Vec<LogEntry>,
        replace: bool,
    ) {
        self.busy = None;
        self.clear_refreshing();
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

    pub(super) fn on_snapshot(&mut self, gen: u64, result: Result<StatusSnapshot, String>) {
        self.refresh_inflight = false;
        // The gen guard stays as a safety net, though single-flight means a
        // stale snapshot can no longer arrive.
        if gen == self.refresh_gen {
            match result {
                Ok(snapshot) => {
                    self.clear_refreshing();
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
        // A refresh was requested while this one ran; run the follow-up now.
        if self.refresh_dirty {
            self.refresh_dirty = false;
            self.refresh();
        }
    }

    /// The editor ran in the foreground; record the result and refresh.
    pub fn on_editor_done(&mut self, desc: String, args: Vec<String>, status: i32) {
        self.push_process_entry(ProcessEntry {
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
        let root =
            build::build_process_log(&self.theme, &self.process_log, self.process_log_dropped);
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
            pane.replace_tree(build::build_process_log(
                &self.theme,
                &self.process_log,
                self.process_log_dropped,
            ));
        }
    }

    fn push_process_entry(&mut self, entry: ProcessEntry) {
        self.process_log.push(entry);
        if self.process_log.len() > PROCESS_LOG_MAX {
            let excess = self.process_log.len() - PROCESS_LOG_MAX;
            self.process_log.drain(..excess);
            self.process_log_dropped += excess;
        }
    }
}
