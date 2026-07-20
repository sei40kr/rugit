//! Key routing: the confirm > input > help > transient > keymap priority
//! chain (see DESIGN.md).

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::keymap::{normalize, KeyPress, Lookup, PaneKind};
use crate::ui::input::InputResult;
use crate::ui::transient::TransientResult;

use super::{App, InputHandler, PendingAction};

impl App {
    pub(super) fn on_key(&mut self, ev: KeyEvent) {
        if ev.kind != KeyEventKind::Press {
            return;
        }
        let kp = normalize(&ev);
        self.message = None;

        if self.confirm.is_some() {
            self.on_confirm_key(&kp);
            return;
        }
        // Take the overlay so a submit can consume its continuation (which
        // may open the next input of a chain); Consumed puts it back.
        if let Some(mut overlay) = self.input.take() {
            match overlay.state.on_key(&kp) {
                InputResult::Consumed => {
                    // Incremental search reacts to every edit.
                    let query = matches!(overlay.handler, InputHandler::Search)
                        .then(|| overlay.state.text.clone());
                    self.input = Some(overlay);
                    if let Some(query) = query {
                        self.search_preview(query);
                    }
                }
                InputResult::Cancel => {
                    if matches!(overlay.handler, InputHandler::Search) {
                        self.restore_search_origin();
                    }
                    self.message = Some("aborted".into());
                }
                InputResult::Submit(value) => match overlay.handler {
                    InputHandler::Search => self.search_submit(value),
                    InputHandler::Submit(on_submit) => on_submit(self, value),
                },
            }
            return;
        }
        if self.show_help {
            let ctrl = kp.mods.contains(KeyModifiers::CONTROL);
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
                TransientResult::Prompt { flag, desc } => self.prompt_transient_arg(flag, desc),
                TransientResult::EditVariable { var } => self.edit_transient_variable(var),
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
            } else if self.search.query.take().is_some() {
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
                PendingAction::GitSeq { desc, cmds } => self.run_git_seq_bg(desc, cmds),
                PendingAction::DeletePaths(paths) => {
                    let mut deleted = 0;
                    let mut last_err = None;
                    for path in &paths {
                        let full = self.git.repo_root.join(path);
                        let result = if full.is_dir() {
                            std::fs::remove_dir_all(&full)
                        } else {
                            std::fs::remove_file(&full)
                        };
                        match result {
                            Ok(()) => deleted += 1,
                            Err(e) => last_err = Some(e),
                        }
                    }
                    self.message = Some(match (last_err, paths.as_slice()) {
                        (Some(e), _) => format!("deleted {deleted}, delete failed: {e}"),
                        (None, [path]) => format!("deleted {path}"),
                        (None, _) => format!("deleted {deleted} untracked file(s)"),
                    });
                    self.refresh();
                }
            }
        } else {
            self.message = Some("aborted".into());
        }
    }
}
