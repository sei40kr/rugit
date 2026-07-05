//! The commit transient: plain commit and amend hand the terminal to
//! $EDITOR; extend runs headless.

use crate::app::{svec, App, EditorRequest};
use crate::ui::transient::TransientAction;

impl App {
    pub(super) fn commit_action(&mut self, action: TransientAction, mut args: Vec<String>) {
        match action {
            TransientAction::Commit => {
                let mut a = svec(&["commit"]);
                a.append(&mut args);
                self.editor_request = Some(EditorRequest::new("commit", a));
            }
            TransientAction::CommitAmend => {
                let mut a = svec(&["commit", "--amend"]);
                a.append(&mut args);
                self.editor_request = Some(EditorRequest::new("amend", a));
            }
            TransientAction::CommitExtend => {
                let mut a = svec(&["commit", "--amend", "--no-edit"]);
                a.append(&mut args);
                self.run_git_bg("extend commit".into(), a, None);
            }
            _ => unreachable!("not a commit action"),
        }
    }
}
