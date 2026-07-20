//! Buffer search: live preview while the input is open, n/N match
//! navigation (with wraparound) while a query is active.

use crate::ui::input::InputState;

use super::{App, InputHandler, InputOverlay};

/// Active buffer-search state. While `query` is set, matches are highlighted
/// and n/N navigate them; ESC clears it.
#[derive(Default)]
pub struct SearchState {
    pub query: Option<String>,
    /// Cursor position when `/` was pressed, for restoring on cancel.
    origin: usize,
}

impl App {
    pub(super) fn start_search(&mut self) {
        self.search.origin = self.panes.last().map(|p| p.cursor).unwrap_or(0);
        self.input = Some(InputOverlay {
            state: InputState::plain("Search"),
            handler: InputHandler::Search,
        });
    }

    /// Live update while the search input is open: highlight matches and jump
    /// to the first one at or after where the search started.
    pub(super) fn search_preview(&mut self, query: String) {
        if query.is_empty() {
            self.restore_search_origin();
            return;
        }
        self.search.query = Some(query.clone());
        let origin = self.search.origin;
        let Some(pane) = self.panes.last_mut() else {
            return;
        };
        let matches = pane.matches_cached(&query);
        let target = matches
            .iter()
            .copied()
            .find(|&i| i >= origin)
            .or(matches.first().copied())
            .unwrap_or(origin);
        pane.cursor = target;
    }

    /// n/N while a search is active: jump to the next/previous match,
    /// wrapping around the buffer.
    pub(super) fn search_move(&mut self, dir: isize) {
        let Some(query) = self.search.query.clone() else {
            return;
        };
        let Some(pane) = self.panes.last_mut() else {
            return;
        };
        let cur = pane.cursor;
        let matches = pane.matches_cached(&query);
        if matches.is_empty() {
            self.message = Some(format!("no matches for \"{query}\""));
            return;
        }
        let (next, wrapped) = if dir > 0 {
            match matches.iter().copied().find(|&i| i > cur) {
                Some(i) => (i, false),
                None => (matches[0], true),
            }
        } else {
            match matches.iter().rev().copied().find(|&i| i < cur) {
                Some(i) => (i, false),
                None => (*matches.last().unwrap(), true),
            }
        };
        pane.cursor = next;
        if wrapped {
            self.message = Some(if dir > 0 {
                "wrapped to top".into()
            } else {
                "wrapped to bottom".into()
            });
        }
    }

    /// Enter in the search input: keep the query active for n/N navigation.
    pub(super) fn search_submit(&mut self, value: String) {
        if value.is_empty() {
            self.search.query = None;
        } else if let Some(pane) = self.panes.last_mut() {
            let n = pane.matches_cached(&value).len();
            self.message = Some(format!("{n} match(es) — n/N to navigate, ESC to clear"));
        }
    }

    /// Clear the query and put the cursor back where the search started.
    pub(super) fn restore_search_origin(&mut self) {
        self.search.query = None;
        let origin = self.search.origin;
        self.pane_mut(|p| p.cursor = origin.min(p.line_count().saturating_sub(1)));
    }
}
