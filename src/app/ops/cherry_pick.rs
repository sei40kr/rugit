//! The cherry-pick transient: pick a revision to apply onto HEAD; the
//! transient flags ride into the picker's continuation as captures. While a
//! cherry-pick is stopped on conflicts the menu switches to
//! continue/skip/abort. Abort confirms first.

use crate::app::{svec, App, Confirm, EditorRequest, PendingAction};
use crate::ui::transient::TransientAction;

impl App {
    /// Whether a cherry-pick is in progress, per the latest status snapshot.
    /// Decides both the cherry-pick menu variant and whether the in-progress
    /// actions are allowed.
    pub(super) fn cherry_picking(&self) -> bool {
        self.snapshot.as_ref().and_then(|s| s.state.as_deref()) == Some("cherry-picking")
    }

    pub(super) fn cherry_pick_action(&mut self, action: TransientAction, args: Vec<String>) {
        // The in-progress actions operate on sequencer state that must
        // exist; gate on an actual cherry-pick because the in-progress menu
        // is chosen from the snapshot, which can lag.
        if matches!(
            action,
            TransientAction::CherryPickContinue
                | TransientAction::CherryPickSkip
                | TransientAction::CherryPickAbort
        ) && !self.cherry_picking()
        {
            self.message = Some("no cherry-pick in progress".into());
            return;
        }
        match action {
            TransientAction::CherryPick => {
                let revs = self.list_revs_at_point();
                self.open_picker("Cherry-pick", revs, move |app, rev| {
                    let mut cli = svec(&["cherry-pick"]);
                    cli.extend(args);
                    cli.push(rev.clone());
                    let desc = format!("cherry-pick {rev}");
                    // --edit stops for the commit message; hand the terminal
                    // to $EDITOR.
                    if cli.iter().any(|f| f == "--edit") {
                        app.editor_request = Some(EditorRequest::new(desc, cli));
                    } else {
                        app.run_git_bg(desc, cli, None);
                    }
                });
            }
            TransientAction::CherryPickApply => {
                let revs = self.list_revs_at_point();
                self.open_picker("Apply changes from commit", revs, move |app, rev| {
                    // Apply without committing; a fast-forward would create
                    // a commit, so drop --ff.
                    let mut cli = svec(&["cherry-pick", "--no-commit"]);
                    cli.extend(args.into_iter().filter(|f| f != "--ff"));
                    cli.push(rev.clone());
                    app.run_git_bg(format!("cherry-pick {rev}"), cli, None);
                });
            }
            // Continuing commits the resolved conflict, which can open
            // $EDITOR for the message; hand the terminal over.
            TransientAction::CherryPickContinue => {
                self.editor_request = Some(EditorRequest::new(
                    "cherry-pick continue",
                    svec(&["cherry-pick", "--continue"]),
                ));
            }
            TransientAction::CherryPickSkip => {
                self.run_git_bg(
                    "cherry-pick skip".into(),
                    svec(&["cherry-pick", "--skip"]),
                    None,
                );
            }
            TransientAction::CherryPickAbort => {
                // Aborting throws away every commit picked so far plus any
                // conflict resolutions in progress; confirm like discard.
                self.confirm = Some(Confirm {
                    prompt: "Abort the cherry-pick in progress?".into(),
                    action: PendingAction::Git {
                        desc: "abort cherry-pick".into(),
                        args: svec(&["cherry-pick", "--abort"]),
                        stdin: None,
                    },
                });
            }
            _ => unreachable!("not a cherry-pick action"),
        }
    }
}
