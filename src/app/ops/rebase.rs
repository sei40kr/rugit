//! The rebase transient and the in-app todo editor (Magit's
//! git-rebase-mode). Plain rebases run in the background; interactive ones
//! open a `RebaseTodo` pane where the plan is edited with p/r/e/s/f/d and
//! M-j/M-k, then confirmed with C-c C-c. The app writes the todo itself and
//! points `GIT_SEQUENCE_EDITOR` at a `cp` of that file, so git's own
//! sequencer runs the plan; per-commit message edits still open $EDITOR via
//! the foreground editor handoff. Abort confirms first.

use crate::app::{svec, App, Confirm, EditorRequest, PendingAction};
use crate::command::TodoCmd;
use crate::git::todo::{self, TodoAction, TodoEntry};
use crate::keymap::PaneKind;
use crate::ui::build;
use crate::ui::pane::{Pane, RebaseTodoState};
use crate::ui::section::SectionValue;
use crate::ui::transient::TransientAction;

impl App {
    /// Whether a rebase is in progress, per the latest status snapshot.
    /// Decides both the rebase menu variant and whether the in-progress
    /// actions are allowed.
    pub(super) fn rebasing(&self) -> bool {
        self.snapshot.as_ref().and_then(|s| s.state.as_deref()) == Some("rebasing")
    }

    pub(super) fn rebase_action(&mut self, action: TransientAction, args: Vec<String>) {
        // The in-progress actions operate on rebase state that must exist;
        // gate on an actual rebase because the in-progress menu is chosen
        // from the snapshot, which can lag.
        if matches!(
            action,
            TransientAction::RebaseContinue
                | TransientAction::RebaseSkip
                | TransientAction::RebaseEditTodo
                | TransientAction::RebaseAbort
        ) && !self.rebasing()
        {
            self.message = Some("no rebase in progress".into());
            return;
        }
        match action {
            TransientAction::RebaseUpstream => {
                let Some(upstream) = self
                    .snapshot
                    .as_ref()
                    .and_then(|s| s.branch.upstream.clone())
                else {
                    self.message = Some("no upstream configured".into());
                    return;
                };
                // The -i switch turns any rebase into an interactive one.
                if args.iter().any(|f| f == "--interactive") {
                    self.open_todo_editor(upstream, args);
                    return;
                }
                let mut cli = svec(&["rebase"]);
                cli.extend(args);
                cli.push(upstream.clone());
                self.run_git_bg(format!("rebase onto {upstream}"), cli, None);
            }
            TransientAction::RebaseElsewhere => {
                let revs = self.list_revs_at_point();
                // The -i switch routes into the todo editor too.
                self.open_picker("Rebase onto", revs, move |app, rev| {
                    if args.iter().any(|f| f == "--interactive") {
                        app.open_todo_editor(rev, args);
                        return;
                    }
                    let mut cli = svec(&["rebase"]);
                    cli.extend(args);
                    cli.push(rev.clone());
                    app.run_git_bg(format!("rebase onto {rev}"), cli, None);
                });
            }
            TransientAction::RebaseInteractive => {
                let revs = self.list_revs_at_point();
                self.open_picker("Interactive rebase onto", revs, move |app, rev| {
                    app.open_todo_editor(rev, args)
                });
            }
            // Continuing commits the resolved conflict, which can open
            // $EDITOR for the message; hand the terminal over.
            TransientAction::RebaseContinue => {
                self.editor_request = Some(EditorRequest::new(
                    "rebase continue",
                    svec(&["rebase", "--continue"]),
                ));
            }
            TransientAction::RebaseSkip => {
                self.run_git_bg("rebase skip".into(), svec(&["rebase", "--skip"]), None);
            }
            TransientAction::RebaseEditTodo => self.open_edit_todo(),
            TransientAction::RebaseAbort => {
                // Aborting throws away everything rebased so far plus any
                // conflict resolutions in progress; confirm like discard.
                self.confirm = Some(Confirm {
                    prompt: "Abort the rebase in progress?".into(),
                    action: PendingAction::Git {
                        desc: "abort rebase".into(),
                        args: svec(&["rebase", "--abort"]),
                        stdin: None,
                    },
                });
            }
            _ => unreachable!("not a rebase action"),
        }
    }

    // ---- todo editor -------------------------------------------------------

    /// Start an interactive rebase: list `base..HEAD`, seed the plan with
    /// picks (applying `--autosquash` ourselves, since we, not git, write the
    /// todo), and open the editor pane.
    fn open_todo_editor(&mut self, base: String, mut flags: Vec<String>) {
        // `--interactive` is what this editor *is*; confirm adds it back.
        flags.retain(|f| f != "--interactive");
        // Listing the span is a fast local read, like `list_branches`.
        let range = format!("{base}..HEAD");
        let out = match self
            .git
            .run(&["log", "--reverse", "--format=%h\u{1f}%s", &range])
        {
            Ok(out) if out.ok() => out,
            Ok(out) => {
                let first = out.stderr.lines().next().unwrap_or("").to_string();
                self.message = Some(format!("rebase onto {base} failed: {first}"));
                return;
            }
            Err(e) => {
                self.message = Some(format!("rebase onto {base} failed: {e}"));
                return;
            }
        };
        let mut entries: Vec<TodoEntry> = out
            .stdout
            .lines()
            .filter_map(|l| {
                let (hash, subject) = l.split_once('\u{1f}')?;
                Some(TodoEntry {
                    action: TodoAction::Pick,
                    hash: hash.to_string(),
                    subject: subject.to_string(),
                })
            })
            .collect();
        if entries.is_empty() {
            self.message = Some(format!("no commits in {range}"));
            return;
        }
        // --autosquash acts on the todo, which git never generates here; the
        // remaining flags are passed through to `git rebase` at confirm.
        if let Some(i) = flags.iter().position(|f| f == "--autosquash") {
            flags.remove(i);
            entries = todo::autosquash(entries);
        }
        self.push_todo_pane(
            format!("Interactive rebase onto {base}"),
            RebaseTodoState {
                entries,
                flags,
                base: Some(base),
            },
        );
    }

    /// Edit the remaining todo of the rebase in progress. Plans containing
    /// instructions the editor cannot represent (exec, break, ...) fall back
    /// to $EDITOR, as does a non-interactive (rebase-apply) rebase.
    fn open_edit_todo(&mut self) {
        let path = self
            .git
            .git_dir
            .join("rebase-merge")
            .join("git-rebase-todo");
        let parsed = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| todo::parse_todo(&text).ok());
        match parsed {
            Some(entries) if !entries.is_empty() => self.push_todo_pane(
                "Editing the rebase in progress".into(),
                RebaseTodoState {
                    entries,
                    flags: Vec::new(),
                    base: None,
                },
            ),
            _ => {
                self.editor_request = Some(EditorRequest::new(
                    "rebase edit-todo",
                    svec(&["rebase", "--edit-todo"]),
                ));
            }
        }
    }

    fn push_todo_pane(&mut self, title: String, state: RebaseTodoState) {
        let root = build::build_rebase_todo(&self.theme, &title, &state.entries);
        let mut pane = Pane::new(PaneKind::RebaseTodo, title, root);
        pane.todo = Some(state);
        self.panes.push(pane);
    }

    /// Route a `TodoCmd` (only bound in the rebase-todo buffer). Called
    /// from `App::dispatch`, hence the wider visibility than the transient
    /// handlers above.
    pub(in crate::app) fn todo_command(&mut self, cmd: TodoCmd) {
        match cmd {
            TodoCmd::Pick => self.todo_set_action(TodoAction::Pick),
            TodoCmd::Reword => self.todo_set_action(TodoAction::Reword),
            TodoCmd::Edit => self.todo_set_action(TodoAction::Edit),
            TodoCmd::Squash => self.todo_set_action(TodoAction::Squash),
            TodoCmd::Fixup => self.todo_set_action(TodoAction::Fixup),
            TodoCmd::Drop => self.todo_set_action(TodoAction::Drop),
            TodoCmd::MoveUp => self.todo_move(-1),
            TodoCmd::MoveDown => self.todo_move(1),
            TodoCmd::Confirm => self.todo_confirm(),
            TodoCmd::Abort => {
                self.panes.pop();
                self.message = Some("rebase todo discarded".into());
            }
        }
    }

    /// Index into the plan of the entry under the cursor.
    fn todo_index_at_point(&self) -> Option<usize> {
        let pane = self.panes.last()?;
        let hash = match pane.value_at_cursor() {
            SectionValue::Commit { hash } => hash,
            _ => return None,
        };
        pane.todo
            .as_ref()?
            .entries
            .iter()
            .position(|e| e.hash == hash)
    }

    fn todo_set_action(&mut self, action: TodoAction) {
        let Some(i) = self.todo_index_at_point() else {
            self.message = Some("not on a commit".into());
            return;
        };
        // Melding into the previous commit needs a previous commit.
        if matches!(action, TodoAction::Squash | TodoAction::Fixup) && i == 0 {
            self.message = Some("cannot meld into the commit before the rebase".into());
            return;
        }
        if let Some(state) = self.panes.last_mut().and_then(|p| p.todo.as_mut()) {
            state.entries[i].action = action;
        }
        self.rebuild_todo_pane();
        // Move on to the next line, like git-rebase-mode.
        self.pane_mut(|p| p.move_cursor(1));
    }

    fn todo_move(&mut self, delta: isize) {
        let Some(i) = self.todo_index_at_point() else {
            self.message = Some("not on a commit".into());
            return;
        };
        let Some(state) = self.panes.last_mut().and_then(|p| p.todo.as_mut()) else {
            return;
        };
        let Some(j) = i
            .checked_add_signed(delta)
            .filter(|j| *j < state.entries.len())
        else {
            return;
        };
        state.entries.swap(i, j);
        // The cursor follows the moved commit via section identity.
        self.rebuild_todo_pane();
    }

    fn rebuild_todo_pane(&mut self) {
        let Some(pane) = self.panes.last_mut() else {
            return;
        };
        let Some(state) = pane.todo.as_ref() else {
            return;
        };
        let root = build::build_rebase_todo(&self.theme, &pane.title, &state.entries);
        pane.replace_tree(root);
    }

    /// C-c C-c: write the plan where `GIT_SEQUENCE_EDITOR` (a `cp` of it)
    /// will install it as the todo, then run git in the foreground so any
    /// reword/squash message prompts can open $EDITOR.
    fn todo_confirm(&mut self) {
        let Some(state) = self.panes.last().and_then(|p| p.todo.clone()) else {
            return;
        };
        if state.entries.iter().all(|e| e.action == TodoAction::Drop) {
            self.message = Some("every commit is dropped — C-c C-k to abort instead".into());
            return;
        }
        let plan_path = self.git.git_dir.join("rugit-rebase-todo");
        if let Err(e) = std::fs::write(&plan_path, todo::serialize_todo(&state.entries)) {
            self.message = Some(format!("cannot write todo: {e}"));
            return;
        }
        let (desc, args) = match &state.base {
            Some(base) => {
                let mut args = svec(&["rebase", "--interactive"]);
                args.extend(state.flags.clone());
                args.push(base.clone());
                (format!("interactive rebase onto {base}"), args)
            }
            None => (
                "rebase edit-todo".to_string(),
                svec(&["rebase", "--edit-todo"]),
            ),
        };
        let mut req = EditorRequest::new(desc, args);
        req.envs.push((
            "GIT_SEQUENCE_EDITOR".into(),
            format!("cp {}", shell_quote(&plan_path.to_string_lossy())),
        ));
        self.editor_request = Some(req);
        self.panes.pop();
    }
}

/// Quote for the POSIX shell git runs `GIT_SEQUENCE_EDITOR` with.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}
