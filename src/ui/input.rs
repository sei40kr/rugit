//! Minibuffer input and picker — a single component with two modes:
//! - plain input (`candidates` empty): free text, e.g. a new branch name
//! - picker: text filters `candidates` (case-insensitive fuzzy subsequence,
//!   best match first), UP/DOWN (or C-p/C-n) move the selection, TAB
//!   completes, RET submits the selected candidate (or the raw text when
//!   nothing matches).

use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};

use crate::keymap::KeyPress;
use crate::theme::Theme;

/// What the submitted value is for. The app maps these to git operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputPurpose {
    CheckoutRev,
    CreateCheckoutBranch,
    CreateBranch,
    /// A revision to merge headlessly (`--no-edit`).
    MergeRev,
    /// A revision to merge, editing the message in $EDITOR.
    MergeEditRev,
    /// A revision to merge with `--no-commit`, leaving the merge pending.
    MergeNoCommitRev,
    /// A revision to squash-merge onto the worktree/index.
    MergeSquashRev,
    /// A branch to merge and then delete (absorb).
    MergeAbsorbRev,
    /// A revision whose merge into HEAD is previewed, committing nothing.
    MergePreviewRev,
    /// A branch to check out and merge the current branch into, deleting
    /// the current branch afterwards.
    MergeIntoRev,
    /// A revision to rebase the current branch onto.
    RebaseOntoRev,
    /// A revision to rebase onto interactively (todo list in $EDITOR).
    RebaseInteractiveRev,
    /// A branch/revision to log (from the log menu's "other").
    LogRev,
    /// A value for a transient value-argument (e.g. `--author=`); the flag is
    /// carried so the submit handler knows which argument to set.
    TransientArg(&'static str),
    /// Incremental buffer search: the app reacts to every keystroke, not
    /// just the final submit.
    Search,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputResult {
    Consumed,
    Cancel,
    Submit(String),
}

#[derive(Debug, Clone)]
pub struct InputState {
    pub prompt: String,
    pub purpose: InputPurpose,
    pub text: String,
    /// Cursor as a char index into `text`.
    pub cursor: usize,
    /// Non-empty makes this a picker.
    pub candidates: Vec<String>,
    /// Selection index into `filtered()`.
    pub selected: usize,
    /// Opaque values handed back to the submit handler alongside the entered
    /// text — context for the pending operation that outlives the transient
    /// that opened it (e.g. log flags awaiting the "log other" rev).
    pub carry: Vec<String>,
}

impl InputState {
    pub fn plain(prompt: impl Into<String>, purpose: InputPurpose) -> Self {
        Self {
            prompt: prompt.into(),
            purpose,
            text: String::new(),
            cursor: 0,
            candidates: Vec::new(),
            selected: 0,
            carry: Vec::new(),
        }
    }

    pub fn picker(
        prompt: impl Into<String>,
        purpose: InputPurpose,
        candidates: Vec<String>,
    ) -> Self {
        Self {
            candidates,
            ..Self::plain(prompt, purpose)
        }
    }

    /// Attach context to hand back to the submit handler with the entered text.
    pub fn with_carry(mut self, carry: Vec<String>) -> Self {
        self.carry = carry;
        self
    }

    pub fn is_picker(&self) -> bool {
        !self.candidates.is_empty()
    }

    /// Candidates fuzzy-matching the current text, best score first (ties
    /// keep the original candidate order).
    pub fn filtered(&self) -> Vec<&str> {
        self.matches().into_iter().map(|(c, _)| c).collect()
    }

    /// Like `filtered`, but each candidate carries the char indices matched
    /// by the current text, for highlighting.
    fn matches(&self) -> Vec<(&str, Vec<usize>)> {
        // Empty input: everything matches, in the original order.
        if self.text.is_empty() {
            return self
                .candidates
                .iter()
                .map(|c| (c.as_str(), Vec::new()))
                .collect();
        }
        let matcher = SkimMatcherV2::default().ignore_case();
        let mut scored: Vec<(i64, &str, Vec<usize>)> = self
            .candidates
            .iter()
            .filter_map(|c| {
                matcher
                    .fuzzy_indices(c, &self.text)
                    .map(|(score, indices)| (score, c.as_str(), indices))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.into_iter().map(|(_, c, ix)| (c, ix)).collect()
    }

    pub fn on_key(&mut self, kp: &KeyPress) -> InputResult {
        let ctrl = kp.mods.contains(KeyModifiers::CONTROL);
        match kp.code {
            KeyCode::Esc => return InputResult::Cancel,
            KeyCode::Char('g') if ctrl => return InputResult::Cancel,
            KeyCode::Enter => {
                let value = if self.is_picker() {
                    let filtered = self.filtered();
                    match filtered.get(self.selected.min(filtered.len().saturating_sub(1))) {
                        Some(sel) => sel.to_string(),
                        None => self.text.trim().to_string(),
                    }
                } else {
                    self.text.trim().to_string()
                };
                return InputResult::Submit(value);
            }
            KeyCode::Up => self.move_selection(-1),
            KeyCode::Down => self.move_selection(1),
            KeyCode::Char('p') if ctrl => self.move_selection(-1),
            KeyCode::Char('n') if ctrl => self.move_selection(1),
            KeyCode::Tab => {
                // Complete the input to the selected candidate.
                if let Some(sel) = self.filtered().get(self.selected) {
                    self.text = sel.to_string();
                    self.cursor = self.text.chars().count();
                }
            }
            KeyCode::Char('u') if ctrl => {
                self.text.clear();
                self.cursor = 0;
                self.selected = 0;
            }
            KeyCode::Char(c) if !ctrl && !kp.mods.contains(KeyModifiers::ALT) => {
                let at = byte_index(&self.text, self.cursor);
                self.text.insert(at, c);
                self.cursor += 1;
                self.selected = 0;
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    let at = byte_index(&self.text, self.cursor - 1);
                    self.text.remove(at);
                    self.cursor -= 1;
                    self.selected = 0;
                }
            }
            KeyCode::Delete => {
                if self.cursor < self.text.chars().count() {
                    let at = byte_index(&self.text, self.cursor);
                    self.text.remove(at);
                    self.selected = 0;
                }
            }
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.text.chars().count()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.text.chars().count(),
            _ => {}
        }
        InputResult::Consumed
    }

    fn move_selection(&mut self, delta: isize) {
        let len = self.filtered().len();
        if len == 0 {
            return;
        }
        self.selected = self
            .selected
            .min(len - 1)
            .saturating_add_signed(delta)
            .min(len - 1);
    }

    /// Lines for the bottom panel: candidate list (picker only), then the
    /// input line with a block cursor.
    pub fn render_lines(&self, t: &Theme, max_candidates: usize) -> Vec<Line<'static>> {
        let mut out = Vec::new();
        if self.is_picker() {
            let filtered = self.filtered();
            let selected = self.selected.min(filtered.len().saturating_sub(1));
            // Keep the selection in view within the candidate window.
            let start = selected.saturating_sub(max_candidates.saturating_sub(1));
            for (i, cand) in filtered.iter().enumerate().skip(start).take(max_candidates) {
                let style = if i == selected {
                    Style::new().bg(t.cursor_bg).add_modifier(Modifier::BOLD)
                } else {
                    Style::new()
                };
                let marker = if i == selected { "▸ " } else { "  " };
                out.push(Line::from(Span::styled(format!("{marker}{cand}"), style)));
            }
            if filtered.is_empty() {
                out.push(Line::from(Span::styled(
                    "  (no match — RET uses the typed text)".to_string(),
                    Style::new().dim(),
                )));
            }
        }
        // Input line with a visible cursor cell.
        let chars: Vec<char> = self.text.chars().collect();
        let before: String = chars[..self.cursor].iter().collect();
        let at: String = chars
            .get(self.cursor)
            .map(|c| c.to_string())
            .unwrap_or_else(|| " ".to_string());
        let after: String = chars[(self.cursor + 1).min(chars.len())..].iter().collect();
        out.push(Line::from(vec![
            Span::styled("> ".to_string(), Style::new().fg(t.key).bold()),
            Span::raw(before),
            Span::styled(at, Style::new().add_modifier(Modifier::REVERSED)),
            Span::raw(after),
        ]));
        out
    }
}

fn byte_index(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyPress {
        KeyPress::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyPress {
        KeyPress::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn type_str(st: &mut InputState, s: &str) {
        for c in s.chars() {
            st.on_key(&key(KeyCode::Char(c)));
        }
    }

    #[test]
    fn plain_input_edit_and_submit() {
        let mut st = InputState::plain("Name", InputPurpose::CreateBranch);
        type_str(&mut st, "featurex");
        st.on_key(&key(KeyCode::Left));
        st.on_key(&key(KeyCode::Backspace)); // delete 'e' before 'x'
        type_str(&mut st, "-");
        assert_eq!(st.text, "featur-x");
        assert_eq!(
            st.on_key(&key(KeyCode::Enter)),
            InputResult::Submit("featur-x".into())
        );
    }

    #[test]
    fn multibyte_editing_is_char_based() {
        let mut st = InputState::plain("Name", InputPurpose::CreateBranch);
        type_str(&mut st, "日本語");
        st.on_key(&key(KeyCode::Backspace));
        assert_eq!(st.text, "日本");
        st.on_key(&key(KeyCode::Home));
        st.on_key(&key(KeyCode::Delete));
        assert_eq!(st.text, "本");
    }

    #[test]
    fn picker_filters_and_submits_selection() {
        let mut st = InputState::picker(
            "Checkout",
            InputPurpose::CheckoutRev,
            vec![
                "main".into(),
                "develop".into(),
                "feature/a".into(),
                "feature/b".into(),
            ],
        );
        type_str(&mut st, "feat");
        assert_eq!(st.filtered(), vec!["feature/a", "feature/b"]);
        st.on_key(&key(KeyCode::Down));
        assert_eq!(
            st.on_key(&key(KeyCode::Enter)),
            InputResult::Submit("feature/b".into())
        );
    }

    #[test]
    fn picker_matches_fuzzy_subsequences() {
        let mut st = InputState::picker(
            "Checkout",
            InputPurpose::CheckoutRev,
            vec!["feature/a".into(), "feature/b".into(), "main".into()],
        );
        type_str(&mut st, "fb");
        assert_eq!(st.filtered(), vec!["feature/b"]);
    }

    #[test]
    fn picker_ranks_better_matches_first() {
        let st = InputState {
            text: "ma".into(),
            ..InputState::picker(
                "Checkout",
                InputPurpose::CheckoutRev,
                vec!["dev-map".into(), "main".into(), "master".into()],
            )
        };
        // Prefix matches outrank a late match; ties keep candidate order.
        assert_eq!(st.filtered(), vec!["main", "master", "dev-map"]);
    }

    #[test]
    fn matching_is_case_insensitive_with_char_indices() {
        let st = InputState {
            text: "fa".into(),
            ..InputState::picker(
                "Checkout",
                InputPurpose::CheckoutRev,
                vec!["Feature/A".into(), "日本語".into()],
            )
        };
        assert_eq!(st.filtered(), vec!["Feature/A"]);

        let st = InputState {
            text: "本語".into(),
            ..st
        };
        let matches = st.matches();
        assert_eq!(matches, vec![("日本語", vec![1, 2])]); // char indices, multibyte-safe
    }

    #[test]
    fn picker_falls_back_to_typed_text_when_no_match() {
        let mut st = InputState::picker("Checkout", InputPurpose::CheckoutRev, vec!["main".into()]);
        type_str(&mut st, "v1.2.3");
        assert_eq!(
            st.on_key(&key(KeyCode::Enter)),
            InputResult::Submit("v1.2.3".into())
        );
    }

    #[test]
    fn tab_completes_to_selection_and_ctrl_u_clears() {
        let mut st = InputState::picker(
            "Checkout",
            InputPurpose::CheckoutRev,
            vec!["main".into(), "master".into()],
        );
        type_str(&mut st, "mas");
        st.on_key(&key(KeyCode::Tab));
        assert_eq!(st.text, "master");
        st.on_key(&ctrl('u'));
        assert_eq!(st.text, "");
        assert_eq!(st.filtered().len(), 2);
    }

    #[test]
    fn esc_and_ctrl_g_cancel() {
        let mut st = InputState::plain("Name", InputPurpose::CreateBranch);
        assert_eq!(st.on_key(&key(KeyCode::Esc)), InputResult::Cancel);
        assert_eq!(st.on_key(&ctrl('g')), InputResult::Cancel);
    }
}
