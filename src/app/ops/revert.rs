//! The revert transient: pick a revision to revert; the transient flags
//! ride into the picker's continuation as captures. Without `--edit` the
//! revert runs headless (`--no-edit`), since git would otherwise stop for
//! the message. While a revert is stopped on conflicts the menu switches to
//! continue/skip/abort. Abort confirms first.

use crate::app::{svec, App, Confirm, EditorRequest, PendingAction};
use crate::ui::transient::TransientAction;

impl App {
    /// Whether a revert is in progress, per the latest status snapshot.
    /// Decides both the revert menu variant and whether the in-progress
    /// actions are allowed.
    pub(super) fn reverting(&self) -> bool {
        self.snapshot.as_ref().and_then(|s| s.state.as_deref()) == Some("reverting")
    }

    pub(super) fn revert_action(&mut self, action: TransientAction, args: Vec<String>) {
        // The in-progress actions operate on sequencer state that must
        // exist; gate on an actual revert because the in-progress menu is
        // chosen from the snapshot, which can lag.
        if matches!(
            action,
            TransientAction::RevertContinue
                | TransientAction::RevertSkip
                | TransientAction::RevertAbort
        ) && !self.reverting()
        {
            self.message = Some("no revert in progress".into());
            return;
        }
        match action {
            TransientAction::Revert => {
                let revs = self.list_revs_at_point();
                self.open_picker("Revert commit", revs, move |app, rev| {
                    let mut cli = svec(&["revert"]);
                    // git stops for the message by default; only do so when
                    // the --edit switch asks for it.
                    if !args.iter().any(|f| f == "--edit" || f == "--no-edit") {
                        cli.push("--no-edit".into());
                    }
                    cli.extend(args);
                    cli.push(rev.clone());
                    let desc = format!("revert {rev}");
                    if cli.iter().any(|f| f == "--edit") {
                        app.editor_request = Some(EditorRequest::new(desc, cli));
                    } else {
                        app.run_git_bg(desc, cli, None);
                    }
                });
            }
            TransientAction::RevertNoCommit => {
                let revs = self.list_revs_at_point();
                self.open_picker("Revert changes", revs, move |app, rev| {
                    // Revert onto the worktree/index without committing;
                    // --edit is about the commit message, so drop it.
                    let mut cli = svec(&["revert", "--no-commit"]);
                    cli.extend(args.into_iter().filter(|f| f != "--edit"));
                    cli.push(rev.clone());
                    app.run_git_bg(format!("revert {rev}"), cli, None);
                });
            }
            // Continuing commits the resolved conflict, which can open
            // $EDITOR for the message; hand the terminal over.
            TransientAction::RevertContinue => {
                self.editor_request = Some(EditorRequest::new(
                    "revert continue",
                    svec(&["revert", "--continue"]),
                ));
            }
            TransientAction::RevertSkip => {
                self.run_git_bg("revert skip".into(), svec(&["revert", "--skip"]), None);
            }
            TransientAction::RevertAbort => {
                // Aborting throws away every revert made so far plus any
                // conflict resolutions in progress; confirm like discard.
                self.confirm = Some(Confirm {
                    prompt: "Abort the revert in progress?".into(),
                    action: PendingAction::Git {
                        desc: "abort revert".into(),
                        args: svec(&["revert", "--abort"]),
                        stdin: None,
                    },
                });
            }
            _ => unreachable!("not a revert action"),
        }
    }
}
