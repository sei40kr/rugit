//! The push / pull / fetch transients: fire-and-forget mutations against
//! remotes, all running on worker threads.

use crate::app::{svec, App};
use crate::ui::transient::TransientAction;

impl App {
    pub(super) fn remote_action(&mut self, action: TransientAction, mut args: Vec<String>) {
        match action {
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
            _ => unreachable!("not a remote action"),
        }
    }
}
