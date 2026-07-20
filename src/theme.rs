//! Color scheme. Every color the UI uses is a named role on `Theme`, so the
//! whole look is overridable from `[colors]` in config.toml. Values accept
//! ratatui's color syntax: names ("red", "lightblue"), hex ("#3a3a3a"), and
//! 256-color indexes ("42").

use std::str::FromStr;

use ratatui::style::Color;

macro_rules! theme {
    ($($field:ident: $default:expr => $key:literal),+ $(,)?) => {
        #[derive(Debug, Clone)]
        pub struct Theme {
            $(pub $field: Color),+
        }

        impl Default for Theme {
            fn default() -> Self {
                Self { $($field: $default),+ }
            }
        }

        impl Theme {
            /// Override one role by its config key. `Err` for an unknown key
            /// or an unparsable color, with a message for the warning list.
            pub fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
                let color = Color::from_str(value)
                    .map_err(|_| format!("colors: invalid color {value:?} for {key:?}"))?;
                match key {
                    $($key => self.$field = color,)+
                    _ => return Err(format!("colors: unknown key {key:?}")),
                }
                Ok(())
            }
        }
    };
}

theme! {
    // Sections
    section_heading: Color::Cyan     => "section-heading",   // group titles
    header_bg:       Color::Blue     => "header-bg",         // log header bar background
    header_fg:       Color::White    => "header-fg",         // log header bar text
    branch:          Color::Cyan     => "branch",            // local branch / HEAD in log
    branch_remote:   Color::Green    => "branch-remote",     // remote-tracking ref in log
    tag:             Color::Yellow   => "tag",               // tag ref in log
    upstream:        Color::Green    => "upstream",
    repo_state:      Color::Red      => "repo-state",        // "merging" etc.
    untracked:       Color::Magenta  => "untracked",
    conflict:        Color::Red      => "conflict",
    file_status:     Color::Blue     => "file-status",       // "modified" prefix
    hash:            Color::Yellow   => "hash",              // commit / stash ids
    log_author:      Color::Blue     => "log-author",        // log margin author
    log_date:        Color::Gray     => "log-date",          // log margin date
    todo_action:     Color::Magenta  => "todo-action",       // rebase todo verbs
    // Diffs
    hunk_header:     Color::Cyan     => "hunk-header",
    diff_add:        Color::Green    => "diff-add",
    diff_remove:     Color::Red      => "diff-remove",
    // Chrome
    cursor_bg:       Color::DarkGray => "cursor-bg",
    search_match_bg: Color::Magenta   => "search-match-bg",  // bg of matched text
    search_match_fg: Color::LightCyan => "search-match-fg",  // fg of matched text
    bar_bg:          Color::Black    => "bar-bg",
    bar_fg:          Color::Gray     => "bar-fg",
    message:         Color::Cyan     => "message",
    warning:         Color::Yellow   => "warning",           // busy indicator, confirm
    error:           Color::Red      => "error",
    success:         Color::Green    => "success",
    // Menus (transient / which-key / help)
    key:             Color::Yellow   => "key",
    input_prompt:    Color::Yellow   => "input-prompt",       // minibuffer "> "
    picker_match:    Color::Cyan     => "picker-match",       // fuzzy-matched chars
    picker_marker:   Color::Yellow   => "picker-marker",      // "▸" on the selection
    picker_selected_bg: Color::DarkGray => "picker-selected-bg",
    picker_selected_fg: Color::Reset    => "picker-selected-fg",
    picker_count:    Color::DarkGray => "picker-count",       // "12/45" hint
    menu_title:      Color::Magenta  => "menu-title",
    command:         Color::Cyan     => "command",           // command names in help
    help_border:     Color::Cyan     => "help-border",
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_accepts_names_hex_and_indexed() {
        let mut t = Theme::default();
        t.set("diff-add", "blue").unwrap();
        assert_eq!(t.diff_add, Color::Blue);
        t.set("cursor-bg", "#3a3a3a").unwrap();
        assert_eq!(t.cursor_bg, Color::Rgb(0x3a, 0x3a, 0x3a));
        t.set("key", "42").unwrap();
        assert_eq!(t.key, Color::Indexed(42));
    }

    #[test]
    fn set_rejects_unknown_key_and_bad_color() {
        let mut t = Theme::default();
        assert!(t.set("no-such-role", "red").is_err());
        assert!(t.set("diff-add", "not-a-color").is_err());
    }
}
