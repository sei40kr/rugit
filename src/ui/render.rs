//! Drawing: pane lines with cursor highlight, status bar, and the overlay
//! stack (transient panel, which-key, help, confirm prompt).

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthChar;

use crate::app::App;
use crate::keymap::format_keys;
use crate::theme::Theme;

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    if area.height < 2 {
        return;
    }
    let main = Rect::new(area.x, area.y, area.width, area.height - 1);
    let bar = Rect::new(area.x, area.y + area.height - 1, area.width, 1);

    draw_pane(f, app, main);
    draw_status_bar(f, app, bar);

    if let Some(input) = &app.input {
        // Show up to 10 candidates plus the input line.
        let lines = input.render_lines(&app.theme, 10);
        draw_bottom_panel(f, &app.theme, area, &input.prompt, lines);
    } else if let Some(transient) = &app.transient {
        draw_bottom_panel(
            f,
            &app.theme,
            area,
            transient.def.title,
            transient.render_lines(&app.theme),
        );
    } else if !app.pending.is_empty() {
        let cands = app.which_key_candidates();
        if !cands.is_empty() {
            let lines = cands
                .into_iter()
                .map(|(k, label)| {
                    Line::from(vec![
                        Span::styled(format!(" {k:<8}"), Style::new().fg(app.theme.key)),
                        Span::raw(label),
                    ])
                })
                .collect();
            draw_bottom_panel(f, &app.theme, area, &format_keys(&app.pending), lines);
        }
    }

    if app.show_help {
        draw_help(f, app, area);
    }
}

fn draw_pane(f: &mut Frame, app: &mut App, area: Rect) {
    let scrolloff = app.scrolloff;
    let cursor_bg = Style::new().bg(app.theme.cursor_bg);
    let search_style = Style::new()
        .bg(app.theme.search_match)
        .add_modifier(Modifier::BOLD);
    let query = app.search.clone();
    let Some(pane) = app.panes.last_mut() else {
        return;
    };
    pane.follow(area.height as usize, scrolloff);
    let top = pane.top;
    let end = (top + area.height as usize).min(pane.flat.len());
    let cursor = pane.cursor;

    let lines: Vec<Line> = pane.flat[top..end]
        .iter()
        .enumerate()
        .map(|(i, fl)| {
            let mut content = match &fl.margin {
                Some(m) => with_margin(&fl.line, m, area.width),
                None => fl.line.clone(),
            };
            if let Some(bg) = fl.fill_bg {
                content = fill_background(content, bg, area.width);
            }
            let mut line = if top + i == cursor {
                highlight(&content, cursor_bg)
            } else {
                content
            };
            if let Some(q) = &query {
                line = highlight_query(line, q, search_style);
            }
            line
        })
        .collect();
    f.render_widget(Paragraph::new(Text::from(lines)), area);
}

/// One blank column kept at the far right so the margin never touches the edge.
const MARGIN_RIGHT_GAP: usize = 1;
/// Minimum blank columns between the content and the margin block.
const MARGIN_LEFT_GAP: usize = 2;
/// Below this many content columns the margin is dropped rather than squeezing
/// the subject into nothing (narrow terminals).
const MARGIN_MIN_CONTENT: usize = 12;

/// Lay the heading out as two aligned regions: `content` on the left (padded,
/// and truncated when too long so it never eats into the margin) and the
/// fixed-width `margin` block flush to the right with a one-column gap. All
/// arithmetic is in display columns, so full-width glyphs stay aligned.
fn with_margin(content: &Line<'static>, margin: &Line<'static>, width: u16) -> Line<'static> {
    let total = width as usize;
    let mw = margin.width();
    let reserved = mw + MARGIN_LEFT_GAP + MARGIN_RIGHT_GAP;
    // No room for the margin — just show the (possibly truncated) content.
    if total < reserved + MARGIN_MIN_CONTENT {
        return Line::from(truncate_line(content, total).0);
    }
    let content_col = total - reserved;
    let (mut spans, used) = truncate_line(content, content_col);
    spans.push(Span::raw(spaces(content_col - used + MARGIN_LEFT_GAP)));
    spans.extend(margin.spans.iter().cloned());
    spans.push(Span::raw(spaces(MARGIN_RIGHT_GAP)));
    Line::from(spans)
}

fn spaces(n: usize) -> String {
    " ".repeat(n)
}

/// Extend `line`'s background across the full pane width by appending a
/// `bg`-colored blank tail (the log header bar). The line's own spans keep
/// their styles; only the trailing gap is added.
fn fill_background(line: Line<'static>, bg: ratatui::style::Color, width: u16) -> Line<'static> {
    let used = line.width();
    let mut spans = line.spans;
    if (width as usize) > used {
        spans.push(Span::styled(spaces(width as usize - used), Style::new().bg(bg)));
    }
    Line::from(spans)
}

/// Truncate a styled line to at most `max` display columns, preserving span
/// styles and never splitting a full-width glyph. Returns the surviving spans
/// and their total display width (`<= max`); an ellipsis takes the last column
/// when anything was cut.
fn truncate_line(line: &Line<'static>, max: usize) -> (Vec<Span<'static>>, usize) {
    if line.width() <= max {
        return (line.spans.to_vec(), line.width());
    }
    let budget = max.saturating_sub(1); // reserve one column for '…'
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;
    'spans: for span in &line.spans {
        let mut seg = String::new();
        for c in span.content.chars() {
            let w = UnicodeWidthChar::width(c).unwrap_or(0);
            if used + w > budget {
                if !seg.is_empty() {
                    out.push(Span::styled(seg, span.style));
                }
                break 'spans;
            }
            seg.push(c);
            used += w;
        }
        if !seg.is_empty() {
            out.push(Span::styled(seg, span.style));
        }
    }
    out.push(Span::raw("…"));
    (out, used + 1)
}

/// Character mask of query matches in `text` (smart-case, non-overlapping).
/// `None` when nothing matches.
fn match_mask(text: &str, query: &str) -> Option<Vec<bool>> {
    if query.is_empty() {
        return None;
    }
    let sensitive = query.chars().any(char::is_uppercase);
    let norm = |c: char| {
        if sensitive {
            c
        } else {
            c.to_lowercase().next().unwrap_or(c)
        }
    };
    let t: Vec<char> = text.chars().map(norm).collect();
    let q: Vec<char> = query.chars().map(norm).collect();
    if t.len() < q.len() {
        return None;
    }
    let mut mask = vec![false; t.len()];
    let mut found = false;
    let mut i = 0;
    while i + q.len() <= t.len() {
        if t[i..i + q.len()] == q[..] {
            mask[i..i + q.len()].fill(true);
            found = true;
            i += q.len();
        } else {
            i += 1;
        }
    }
    found.then_some(mask)
}

/// Restyle the matched substrings of a line, splitting spans at match
/// boundaries so the surrounding styling (diff colors etc.) is preserved.
fn highlight_query(line: Line<'static>, query: &str, hl: Style) -> Line<'static> {
    let text = line.to_string();
    let Some(mask) = match_mask(&text, query) else {
        return line;
    };
    let mut out: Vec<Span> = Vec::new();
    let mut off = 0usize;
    for span in &line.spans {
        let mut seg = String::new();
        let mut seg_matched = None::<bool>;
        for (j, c) in span.content.chars().enumerate() {
            let matched = mask.get(off + j).copied().unwrap_or(false);
            if seg_matched != Some(matched) {
                if let Some(m) = seg_matched.take() {
                    out.push(segment(std::mem::take(&mut seg), span.style, m, hl));
                }
                seg_matched = Some(matched);
            }
            seg.push(c);
        }
        if let Some(m) = seg_matched {
            out.push(segment(seg, span.style, m, hl));
        }
        off += span.content.chars().count();
    }
    Line::from(out)
}

fn segment(s: String, base: Style, matched: bool, hl: Style) -> Span<'static> {
    Span::styled(s, if matched { base.patch(hl) } else { base })
}

/// Apply a background highlight while keeping per-span foregrounds.
fn highlight(line: &Line<'static>, hl: Style) -> Line<'static> {
    let mut spans: Vec<Span> = line
        .spans
        .iter()
        .map(|s| Span::styled(s.content.clone(), s.style.patch(hl)))
        .collect();
    if spans.is_empty() {
        spans.push(Span::styled(" ", hl));
    } else {
        // Extend the highlight to the full width via a padded tail span.
        spans.push(Span::styled(" ".repeat(200), hl));
    }
    Line::from(spans)
}

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let base = Style::new().bg(t.bar_bg).fg(t.bar_fg);

    if let Some(confirm) = &app.confirm {
        let p = Paragraph::new(Line::from(vec![
            Span::styled(confirm.prompt.clone(), Style::new().fg(t.warning).bold()),
            Span::styled(" (y/n)", Style::new().fg(t.warning)),
        ]))
        .style(base);
        f.render_widget(p, area);
        return;
    }

    let mut left: Vec<Span> = vec![Span::styled(
        format!(
            " {} ",
            app.panes.last().map(|p| p.title.as_str()).unwrap_or("")
        ),
        Style::new().add_modifier(Modifier::BOLD),
    )];
    if let Some(busy) = &app.busy {
        left.push(Span::styled(
            format!("  {busy}…"),
            Style::new().fg(t.warning),
        ));
    } else if let Some(msg) = &app.message {
        left.push(Span::styled(format!("  {msg}"), Style::new().fg(t.message)));
    }

    let right = if !app.pending.is_empty() {
        format!("{} ", format_keys(&app.pending))
    } else if let Some(query) = &app.search {
        let n = app
            .panes
            .last()
            .map(|p| p.find_matches(query).len())
            .unwrap_or(0);
        format!("/{query} ({n}) ")
    } else {
        String::new()
    };
    let left_width = area.width.saturating_sub(right.len() as u16);
    f.render_widget(
        Paragraph::new(Line::from(left)).style(base),
        Rect::new(area.x, area.y, left_width, 1),
    );
    if !right.is_empty() {
        f.render_widget(
            Paragraph::new(right).style(base.fg(t.key)),
            Rect::new(area.x + left_width, area.y, area.width - left_width, 1),
        );
    }
}

/// Bordered panel anchored to the bottom of the screen (transient/which-key).
fn draw_bottom_panel(
    f: &mut Frame,
    t: &Theme,
    screen: Rect,
    title: &str,
    lines: Vec<Line<'static>>,
) {
    let height = (lines.len() as u16 + 2).min(screen.height.saturating_sub(2));
    let area = Rect::new(
        screen.x,
        screen.y + screen.height - 1 - height,
        screen.width,
        height,
    );
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {title} "))
        .border_style(Style::new().fg(t.menu_title));
    f.render_widget(Paragraph::new(Text::from(lines)).block(block), area);
}

fn draw_help(f: &mut Frame, app: &mut App, screen: Rect) {
    let t = &app.theme;
    let kind = app.panes.last().map(|p| p.kind);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut push_map = |title: &str, bindings: Vec<(String, crate::command::Command)>| {
        if bindings.is_empty() {
            return;
        }
        lines.push(Line::from(Span::styled(
            title.to_string(),
            Style::new().fg(t.menu_title).bold(),
        )));
        for (keys, cmd) in bindings {
            let info = crate::command::info(cmd);
            lines.push(Line::from(vec![
                Span::styled(format!(" {keys:<10}"), Style::new().fg(t.key)),
                Span::styled(format!("{:<16}", info.name), Style::new().fg(t.command)),
                Span::raw(info.desc.to_string()),
            ]));
        }
        lines.push(Line::default());
    };
    if let Some(kind) = kind {
        if let Some(local) = app.keymaps.local.get(&kind) {
            push_map("Buffer keys", local.bindings());
        }
    }
    push_map("Global keys", app.keymaps.global.bindings());
    lines.push(Line::from(Span::styled(
        "j/k scroll · q or ESC to close".to_string(),
        Style::new().dim(),
    )));

    let width = screen.width.saturating_sub(6).clamp(20, 72);
    let height = screen.height.saturating_sub(2).min(lines.len() as u16 + 2);
    let area = Rect::new(
        screen.x + (screen.width - width) / 2,
        screen.y + (screen.height - height) / 2,
        width,
        height,
    );

    // Clamp the scroll so the last content line stays at the bottom edge.
    let content_height = height.saturating_sub(2) as usize;
    let max_scroll = lines.len().saturating_sub(content_height);
    app.help_scroll = app.help_scroll.min(max_scroll);
    let title = if max_scroll > 0 {
        format!(
            " Help ({}-{}/{}) ",
            app.help_scroll + 1,
            (app.help_scroll + content_height).min(lines.len()),
            lines.len()
        )
    } else {
        " Help ".to_string()
    };

    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::new().fg(t.help_border));
    f.render_widget(
        Paragraph::new(Text::from(lines))
            .block(block)
            .scroll((app.help_scroll as u16, 0)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    #[test]
    fn with_margin_places_block_at_fixed_right_column_with_gap() {
        let content = Line::from("abc".to_string());
        let margin = Line::from("xy".to_string()); // 2 cols
        // width 30: content col = 30 - (2 + 2 + 1) = 25; margin ends 1 col
        // short of the edge, so a blank column is kept on the right.
        let out = with_margin(&content, &margin, 30);
        assert_eq!(out.to_string(), "abc                        xy ");
        assert_eq!(out.width(), 30);
    }

    #[test]
    fn with_margin_truncates_content_to_protect_the_column() {
        // A subject too long for the content region gets an ellipsis; the
        // margin still lands in the same column as short-subject rows.
        let content = Line::from("this subject is definitely far too long to fit".to_string());
        let margin = Line::from("Ada  2 days ago".to_string()); // 15 cols
        let out = with_margin(&content, &margin, 40);
        assert_eq!(out.width(), 40);
        assert!(out.to_string().contains('…'));
        assert!(out.to_string().ends_with("Ada  2 days ago "));
    }

    #[test]
    fn with_margin_full_width_glyphs_do_not_break_alignment() {
        // A CJK subject (each glyph 2 cols) must be truncated on a cell
        // boundary and the total width must stay exactly `width`.
        let content = Line::from("日本語のコミットメッセージ本文".to_string());
        let margin = Line::from("著者  2 日前".to_string());
        let out = with_margin(&content, &margin, 30);
        assert_eq!(out.width(), 30);
    }

    #[test]
    fn fill_background_pads_to_full_width_with_the_bg() {
        let line = Line::from(Span::styled("Commits in HEAD".to_string(), Style::new()));
        let out = fill_background(line, Color::Blue, 30);
        assert_eq!(out.width(), 30);
        // The appended tail carries the fill background.
        assert_eq!(out.spans.last().unwrap().style.bg, Some(Color::Blue));
    }

    #[test]
    fn with_margin_drops_margin_on_a_narrow_viewport() {
        let content = Line::from("abcdef".to_string());
        let margin = Line::from("Author  2 days ago".to_string());
        // width 15 leaves no room for content + margin; margin is dropped.
        let out = with_margin(&content, &margin, 15);
        assert_eq!(out.to_string(), "abcdef");
    }





    #[test]
    fn match_mask_smart_case_and_multiple() {
        let mask = match_mask("foo bar foo", "foo").unwrap();
        let on: Vec<usize> = mask
            .iter()
            .enumerate()
            .filter(|(_, m)| **m)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(on, vec![0, 1, 2, 8, 9, 10]);
        assert!(match_mask("FOO", "foo").is_some()); // insensitive
        assert!(match_mask("foo", "FOO").is_none()); // sensitive
        assert!(match_mask("short", "longer-query").is_none());
    }

    #[test]
    fn highlight_query_splits_spans_and_keeps_base_style() {
        let base = Style::new().fg(Color::Green);
        let line = Line::from(vec![
            Span::styled("+hello ".to_string(), base),
            Span::styled("world".to_string(), base),
        ]);
        // Match spans the boundary between the two spans.
        let hl = Style::new().bg(Color::Yellow);
        let out = highlight_query(line, "lo wor", hl);
        assert_eq!(out.to_string(), "+hello world");
        // Split into: "+hel", "lo ", "wor", "ld".
        let bgs: Vec<Option<Color>> = out.spans.iter().map(|s| s.style.bg).collect();
        assert_eq!(
            bgs,
            vec![None, Some(Color::Yellow), Some(Color::Yellow), None]
        );
        assert!(out.spans.iter().all(|s| s.style.fg == Some(Color::Green)));
    }

    #[test]
    fn highlight_query_without_match_returns_line_unchanged() {
        let line = Line::from("nothing here".to_string());
        let out = highlight_query(line, "zzz", Style::new().bg(Color::Yellow));
        assert_eq!(out.spans.len(), 1);
        assert_eq!(out.spans[0].style.bg, None);
    }

    #[test]
    fn highlight_query_multibyte_alignment() {
        let line = Line::from("日本語 diff 行".to_string());
        let out = highlight_query(line, "diff", Style::new().bg(Color::Yellow));
        let hl_text: String = out
            .spans
            .iter()
            .filter(|s| s.style.bg.is_some())
            .map(|s| s.content.clone())
            .collect();
        assert_eq!(hl_text, "diff");
    }
}
