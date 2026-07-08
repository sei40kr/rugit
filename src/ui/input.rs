//! Minibuffer input and picker — a single component with two modes:
//! - plain input (`candidates` empty): free text, e.g. a new branch name
//! - picker: text filters `candidates` (case-insensitive fuzzy matching via
//!   `fuzzy-matcher`, best match first), UP/DOWN (or C-p/C-n) move the
//!   selection, TAB completes, RET submits the selected candidate (or the
//!   raw text when nothing matches).

use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};

use crate::keymap::KeyPress;
use crate::theme::Theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputResult {
    Consumed,
    Cancel,
    Submit(String),
}

/// Pure editing/filtering state; what the submitted text *means* lives with
/// the caller (`App::open_input`/`open_picker` take a continuation).
#[derive(Debug, Clone)]
pub struct InputState {
    pub prompt: String,
    pub text: String,
    /// Cursor as a char index into `text`.
    pub cursor: usize,
    /// Non-empty makes this a picker.
    pub candidates: Vec<String>,
    /// Selection index into `filtered()`.
    pub selected: usize,
}

impl InputState {
    pub fn plain(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            text: String::new(),
            cursor: 0,
            candidates: Vec::new(),
            selected: 0,
        }
    }

    pub fn picker(prompt: impl Into<String>, candidates: Vec<String>) -> Self {
        Self {
            candidates,
            ..Self::plain(prompt)
        }
    }

    /// Prefill the text (e.g. a variable's current value), cursor at the end.
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = text.into();
        self.cursor = self.text.chars().count();
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
        scored.sort_by_key(|&(score, _, _)| std::cmp::Reverse(score));
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
            KeyCode::Backspace if self.cursor > 0 => {
                let at = byte_index(&self.text, self.cursor - 1);
                self.text.remove(at);
                self.cursor -= 1;
                self.selected = 0;
            }
            KeyCode::Delete if self.cursor < self.text.chars().count() => {
                let at = byte_index(&self.text, self.cursor);
                self.text.remove(at);
                self.selected = 0;
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
        // Filtered / total candidate counts, appended to the input line.
        let mut counts = None;
        if self.is_picker() {
            let matches = self.matches();
            counts = Some((matches.len(), self.candidates.len()));
            let selected = self.selected.min(matches.len().saturating_sub(1));
            // Keep the selection in view within the candidate window.
            let start = selected.saturating_sub(max_candidates.saturating_sub(1));
            for (i, (cand, indices)) in matches.iter().enumerate().skip(start).take(max_candidates)
            {
                let base = if i == selected {
                    Style::new()
                        .bg(t.picker_selected_bg)
                        .fg(t.picker_selected_fg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::new()
                };
                let marker = if i == selected { "▸ " } else { "  " };
                let mut spans = vec![Span::styled(marker.to_string(), base.fg(t.picker_marker))];
                spans.extend(highlight_spans(cand, indices, base, t.picker_match));
                out.push(Line::from(spans));
            }
            if matches.is_empty() {
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
        let mut spans = vec![
            Span::styled("> ".to_string(), Style::new().fg(t.input_prompt).bold()),
            Span::raw(before),
            Span::styled(at, Style::new().add_modifier(Modifier::REVERSED)),
            Span::raw(after),
        ];
        if let Some((shown, total)) = counts {
            spans.push(Span::styled(
                format!("  {shown}/{total}"),
                Style::new().fg(t.picker_count),
            ));
        }
        out.push(Line::from(spans));
        out
    }
}

/// Split `text` into spans, styling the chars at `indices` (ascending char
/// positions) with `match_fg` layered over `base`. Consecutive chars with
/// the same styling are grouped into one span.
fn highlight_spans(
    text: &str,
    indices: &[usize],
    base: Style,
    match_fg: ratatui::style::Color,
) -> Vec<Span<'static>> {
    let matched_style = base.fg(match_fg).add_modifier(Modifier::BOLD);
    let mut spans = Vec::new();
    let mut buf = String::new();
    let mut buf_matched = false;
    let mut next = indices.iter().peekable();
    for (i, c) in text.chars().enumerate() {
        let matched = next.peek() == Some(&&i);
        if matched {
            next.next();
        }
        if matched != buf_matched && !buf.is_empty() {
            let style = if buf_matched { matched_style } else { base };
            spans.push(Span::styled(std::mem::take(&mut buf), style));
        }
        buf_matched = matched;
        buf.push(c);
    }
    if !buf.is_empty() {
        let style = if buf_matched { matched_style } else { base };
        spans.push(Span::styled(buf, style));
    }
    spans
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
        let mut st = InputState::plain("Name");
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
        let mut st = InputState::plain("Name");
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
                vec!["dev-map".into(), "main".into(), "master".into()],
            )
        };
        // Prefix matches outrank a late match; ties keep candidate order.
        assert_eq!(st.filtered(), vec!["main", "master", "dev-map"]);
    }

    #[test]
    fn picker_empty_text_lists_all_candidates_in_order() {
        let st = InputState::picker(
            "Checkout",
            vec!["main".into(), "develop".into(), "feature/a".into()],
        );
        assert_eq!(st.filtered(), vec!["main", "develop", "feature/a"]);
        assert!(st.matches().iter().all(|(_, ix)| ix.is_empty()));
    }

    #[test]
    fn picker_match_indices_are_ascending_char_positions() {
        // `highlight_spans` walks the indices front to back, so the matcher
        // must hand them over sorted.
        let st = InputState {
            text: "fb".into(),
            ..InputState::picker("Checkout", vec!["feature/b".into()])
        };
        let matches = st.matches();
        assert!(matches[0].1.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn picker_match_ignores_case_both_ways() {
        // Forced ignore-case (not smart-case): an uppercase query still
        // matches lowercase candidates.
        let st = InputState {
            text: "FEAT".into(),
            ..InputState::picker("Checkout", vec!["feature/a".into(), "main".into()])
        };
        assert_eq!(st.filtered(), vec!["feature/a"]);
    }

    #[test]
    fn picker_match_is_case_insensitive_and_multibyte_safe() {
        let st = InputState {
            text: "fa".into(),
            ..InputState::picker("Checkout", vec!["Feature/A".into(), "docs".into()])
        };
        let matches = st.matches();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "Feature/A");
        assert!(!matches[0].1.is_empty());

        let st = InputState {
            text: "本語".into(),
            ..InputState::picker("Checkout", vec!["日本語".into()])
        };
        // Highlight indices are char positions, multibyte-safe.
        assert_eq!(st.matches()[0].1, vec![1, 2]);
    }

    #[test]
    fn highlight_spans_groups_matched_runs() {
        let t = Theme::default();
        let spans = highlight_spans("feature/a", &[0, 1, 8], Style::new(), t.picker_match);
        let texts: Vec<&str> = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(texts, vec!["fe", "ature/", "a"]);
        assert_eq!(spans[0].style.fg, Some(t.picker_match));
        assert_eq!(spans[1].style.fg, None);
        assert_eq!(spans[2].style.fg, Some(t.picker_match));
    }

    #[test]
    fn picker_falls_back_to_typed_text_when_no_match() {
        let mut st = InputState::picker("Checkout", vec!["main".into()]);
        type_str(&mut st, "v1.2.3");
        assert_eq!(
            st.on_key(&key(KeyCode::Enter)),
            InputResult::Submit("v1.2.3".into())
        );
    }

    #[test]
    fn tab_completes_to_selection_and_ctrl_u_clears() {
        let mut st = InputState::picker("Checkout", vec!["main".into(), "master".into()]);
        type_str(&mut st, "mas");
        st.on_key(&key(KeyCode::Tab));
        assert_eq!(st.text, "master");
        st.on_key(&ctrl('u'));
        assert_eq!(st.text, "");
        assert_eq!(st.filtered().len(), 2);
    }

    #[test]
    fn picker_input_line_shows_filtered_and_total_counts() {
        let mut st = InputState::picker(
            "Checkout",
            vec![
                "main".into(),
                "develop".into(),
                "feature/a".into(),
                "feature/b".into(),
            ],
        );
        type_str(&mut st, "feat");
        let lines = st.render_lines(&Theme::default(), 10);
        let input_line: String = lines
            .last()
            .unwrap()
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(input_line.ends_with("2/4"), "got {input_line:?}");
    }

    #[test]
    fn esc_and_ctrl_g_cancel() {
        let mut st = InputState::plain("Name");
        assert_eq!(st.on_key(&key(KeyCode::Esc)), InputResult::Cancel);
        assert_eq!(st.on_key(&ctrl('g')), InputResult::Cancel);
    }
}
