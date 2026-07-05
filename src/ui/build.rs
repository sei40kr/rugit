//! Builders that turn git snapshots into styled section trees. All buffer
//! kinds funnel through the same `Section` machinery in `section.rs`.
//! Colors come exclusively from `Theme` so the scheme is configurable.

use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::git::client::ProcessEntry;
use crate::git::todo::{TodoAction, TodoEntry};
use crate::git::types::{DiffArea, FileDiff, LogEntry, StatusSnapshot};
use crate::theme::Theme;
use crate::ui::section::{Group, Section, SectionValue};

fn heading_style() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}

fn group_heading(t: &Theme, text: String) -> Line<'static> {
    Line::from(Span::styled(text, heading_style().fg(t.section_heading)))
}

pub fn build_status(t: &Theme, s: &StatusSnapshot) -> Section {
    let mut root = Section::root();
    root.body = header_lines(t, s);

    if !s.untracked.is_empty() {
        let mut g = Section::new(
            0,
            "group:untracked",
            SectionValue::Group(Group::Untracked),
            group_heading(t, format!("Untracked files ({})", s.untracked.len())),
        );
        for path in &s.untracked {
            g.children.push(Section::new(
                g.id,
                &format!("file:{path}"),
                SectionValue::File {
                    area: DiffArea::Untracked,
                    path: path.clone(),
                },
                Line::from(Span::styled(path.clone(), Style::new().fg(t.untracked))),
            ));
        }
        root.children.push(g);
    }

    if !s.unmerged.is_empty() {
        let mut g = Section::new(
            0,
            "group:unmerged",
            SectionValue::Group(Group::Unmerged),
            group_heading(t, format!("Unmerged paths ({})", s.unmerged.len())),
        );
        for path in &s.unmerged {
            g.children.push(Section::new(
                g.id,
                &format!("file:{path}"),
                SectionValue::Text,
                Line::from(Span::styled(
                    format!("conflict   {path}"),
                    Style::new().fg(t.conflict),
                )),
            ));
        }
        root.children.push(g);
    }

    for (key, group, title, area, files) in [
        (
            "group:unstaged",
            Group::Unstaged,
            "Unstaged changes",
            DiffArea::Unstaged,
            &s.unstaged,
        ),
        (
            "group:staged",
            Group::Staged,
            "Staged changes",
            DiffArea::Staged,
            &s.staged,
        ),
    ] {
        if files.is_empty() {
            continue;
        }
        let mut g = Section::new(
            0,
            key,
            SectionValue::Group(group),
            group_heading(t, format!("{title} ({})", files.len())),
        );
        for fd in files {
            g.children.push(file_section(t, g.id, area, fd));
        }
        root.children.push(g);
    }

    if !s.stashes.is_empty() {
        let mut g = Section::new(
            0,
            "group:stashes",
            SectionValue::Group(Group::Stashes),
            group_heading(t, format!("Stashes ({})", s.stashes.len())),
        );
        for st in &s.stashes {
            g.children.push(Section::new(
                g.id,
                &format!("stash:{}", st.index),
                SectionValue::Stash { index: st.index },
                Line::from(vec![
                    Span::styled(format!("stash@{{{}}}", st.index), Style::new().fg(t.hash)),
                    Span::raw(format!(" {}", st.message)),
                ]),
            ));
        }
        root.children.push(g);
    }

    if !s.recent.is_empty() {
        let mut g = Section::new(
            0,
            "group:recent",
            SectionValue::Group(Group::Recent),
            group_heading(t, "Recent commits".to_string()),
        );
        for c in &s.recent {
            g.children.push(Section::new(
                g.id,
                &format!("commit:{}", c.hash),
                SectionValue::Commit {
                    hash: c.hash.clone(),
                },
                Line::from(vec![
                    Span::styled(c.hash.clone(), Style::new().fg(t.hash)),
                    Span::raw(format!(" {}", c.subject)),
                ]),
            ));
        }
        root.children.push(g);
    }

    if root.children.is_empty() && root.body.len() <= 3 {
        root.body.push(Line::default());
        root.body.push(Line::from(Span::styled(
            "Nothing to see here — working tree clean.",
            Style::new().dim(),
        )));
    }
    root
}

fn header_lines(t: &Theme, s: &StatusSnapshot) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let branch_span = match (&s.branch.head, s.branch.detached) {
        (Some(name), _) => Span::styled(name.clone(), Style::new().fg(t.branch).bold()),
        (None, true) => Span::styled("(detached)".to_string(), Style::new().fg(t.repo_state)),
        _ => Span::styled("(no branch)".to_string(), Style::new().fg(t.repo_state)),
    };
    let summary = s
        .head_summary
        .clone()
        .unwrap_or_else(|| "(no commits yet)".to_string());
    out.push(Line::from(vec![
        Span::styled("Head:     ".to_string(), Style::new().dim()),
        branch_span,
        Span::raw(" "),
        Span::raw(summary),
    ]));
    if let Some(upstream) = &s.branch.upstream {
        let mut spans = vec![
            Span::styled("Merge:    ".to_string(), Style::new().dim()),
            Span::styled(upstream.clone(), Style::new().fg(t.upstream)),
        ];
        if s.branch.ahead != 0 || s.branch.behind != 0 {
            spans.push(Span::raw(format!(
                " (ahead {}, behind {})",
                s.branch.ahead, s.branch.behind
            )));
        }
        out.push(Line::from(spans));
    }
    if let Some(state) = &s.state {
        out.push(Line::from(vec![
            Span::styled("State:    ".to_string(), Style::new().dim()),
            Span::styled(state.clone(), Style::new().fg(t.repo_state).bold()),
        ]));
    }
    out
}

/// A file section with its hunks as children — shared by status and
/// revision buffers.
pub fn file_section(t: &Theme, parent_id: u64, area: DiffArea, fd: &FileDiff) -> Section {
    let name = match &fd.old_path {
        Some(old) => format!("{old} -> {}", fd.path),
        None => fd.path.clone(),
    };
    let mut sec = Section::new(
        parent_id,
        &format!("file:{}", fd.path),
        SectionValue::File {
            area,
            path: fd.path.clone(),
        },
        Line::from(vec![
            Span::styled(
                format!("{}   ", fd.status_word()),
                Style::new().fg(t.file_status),
            ),
            Span::styled(name, heading_style()),
        ]),
    );
    if fd.is_binary {
        sec.body.push(Line::from(Span::styled(
            "(binary file)".to_string(),
            Style::new().dim(),
        )));
    }
    for (hunk_idx, hunk) in fd.hunks.iter().enumerate() {
        let mut h = Section::new(
            sec.id,
            &format!("hunk:{}", hunk.old_start),
            SectionValue::Hunk {
                area,
                path: fd.path.clone(),
                hunk_idx,
            },
            Line::from(Span::styled(
                hunk.header.clone(),
                Style::new().fg(t.hunk_header),
            )),
        );
        for l in &hunk.lines {
            h.body.push(diff_line(t, l));
        }
        sec.children.push(h);
    }
    sec
}

fn diff_line(t: &Theme, l: &str) -> Line<'static> {
    let style = match l.as_bytes().first() {
        Some(b'+') => Style::new().fg(t.diff_add),
        Some(b'-') => Style::new().fg(t.diff_remove),
        Some(b'\\') => Style::new().dim(),
        _ => Style::new(),
    };
    Line::from(Span::styled(l.to_string(), style))
}

/// Revision buffer (RET on a commit or stash): commit header text followed
/// by the diff as read-only file sections.
pub fn build_revision(t: &Theme, header: &str, files: &[FileDiff]) -> Section {
    let mut root = Section::root();
    root.body = header.lines().map(|l| Line::from(l.to_string())).collect();
    for fd in files {
        root.children
            .push(file_section(t, 0, DiffArea::Committed, fd));
    }
    root
}

/// Log buffer: one commit per line, each a `Commit` section so RET (visit)
/// opens it in a revision buffer.
pub fn build_log(t: &Theme, title: &str, entries: &[LogEntry]) -> Section {
    let mut root = Section::root();
    // Commits are top-level sections; render them as one tight list rather
    // than blank-line-separated like status groups.
    root.compact = true;
    // Header naming what is being logged. Mirrors magit's log header-line
    // ("Commits in HEAD"); the caller supplies the whole phrase as the title.
    // Rendered as a full-width bar (see `body_fill`) in the header colors.
    root.body_fill = Some(t.header_bg);
    root.body.push(Line::from(Span::styled(
        title.to_string(),
        heading_style().fg(t.header_fg).bg(t.header_bg),
    )));
    if entries.is_empty() {
        root.body.push(Line::from(Span::styled(
            "No commits.".to_string(),
            Style::new().dim(),
        )));
        return root;
    }
    // The margin is a fixed two-column block (author left-aligned, date
    // right-aligned) sized to the widest of each across the whole buffer, so
    // every row's columns line up. Widths are in display columns, so CJK /
    // full-width names don't skew the alignment.
    let author_col = entries.iter().map(|e| e.author.width()).max().unwrap_or(0);
    let date_col = entries.iter().map(|e| e.date.width()).max().unwrap_or(0);
    for e in entries {
        let mut spans = vec![
            Span::styled(e.hash.clone(), Style::new().fg(t.hash)),
            Span::raw(" "),
        ];
        spans.extend(ref_spans(t, &e.refs));
        spans.push(Span::raw(e.subject.clone()));
        let mut sec = Section::new(
            0,
            &format!("commit:{}", e.hash),
            SectionValue::Commit {
                hash: e.hash.clone(),
            },
            Line::from(spans),
        );
        sec.margin = log_margin(t, &e.author, author_col, &e.date, date_col);
        root.children.push(sec);
    }
    root
}

/// The log's right-margin block: `author` (left-aligned in `author_col`, in the
/// `log_author` role), two spaces, then `date` (right-aligned in `date_col`, in
/// the `log_date` role). `None` when both are empty. Padding is by display
/// width so the columns stay aligned with wide glyphs.
fn log_margin(
    t: &Theme,
    author: &str,
    author_col: usize,
    date: &str,
    date_col: usize,
) -> Option<Line<'static>> {
    if author.is_empty() && date.is_empty() {
        return None;
    }
    Some(Line::from(vec![
        Span::styled(pad_end(author, author_col), Style::new().fg(t.log_author)),
        Span::raw("  "),
        Span::styled(pad_start(date, date_col), Style::new().fg(t.log_date)),
    ]))
}

/// Right-pad `s` with spaces to `cols` display columns (no-op if already wider).
fn pad_end(s: &str, cols: usize) -> String {
    let w = s.width();
    if w >= cols {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(cols - w))
    }
}

/// Left-pad `s` with spaces to `cols` display columns (no-op if already wider).
fn pad_start(s: &str, cols: usize) -> String {
    let w = s.width();
    if w >= cols {
        s.to_string()
    } else {
        format!("{}{s}", " ".repeat(cols - w))
    }
}

/// Turn git's `%D` decoration string ("HEAD -> main, origin/x, tag: v1")
/// into colored tokens, magit-style: the current branch (in the branch color),
/// remotes and tags in their own roles, no enclosing parentheses. Following
/// magit, the bare `HEAD` pointer and symbolic `*/HEAD` refs are not shown.
fn ref_spans(t: &Theme, refs: &str) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    for raw in refs.split(", ") {
        let raw = raw.trim();
        if raw.is_empty() || raw == "HEAD" || raw.ends_with("/HEAD") {
            continue;
        }
        if let Some(branch) = raw.strip_prefix("HEAD -> ") {
            // Current branch — the "HEAD ->" pointer itself is elided.
            out.push(Span::styled(
                branch.to_string(),
                Style::new().fg(t.branch).bold(),
            ));
        } else if let Some(tag) = raw.strip_prefix("tag: ") {
            out.push(Span::styled(tag.to_string(), Style::new().fg(t.tag).bold()));
        } else if raw.contains('/') {
            out.push(Span::styled(
                raw.to_string(),
                Style::new().fg(t.branch_remote),
            ));
        } else {
            out.push(Span::styled(raw.to_string(), Style::new().fg(t.branch)));
        }
        out.push(Span::raw(" "));
    }
    out
}

/// The rebase-todo editor buffer: one commit per line, `<action> <hash>
/// <subject>`, oldest first (the order git applies them). Each row is a
/// `Commit` section, so RET (visit) shows the commit and the cursor sticks
/// to its commit across reorders. A dim key-hint block sits at the bottom,
/// like the comment block git appends to the todo file.
pub fn build_rebase_todo(t: &Theme, title: &str, entries: &[TodoEntry]) -> Section {
    let mut root = Section::root();
    root.compact = true;
    root.body_fill = Some(t.header_bg);
    root.body.push(Line::from(Span::styled(
        title.to_string(),
        heading_style().fg(t.header_fg).bg(t.header_bg),
    )));
    for e in entries {
        let dropped = e.action == TodoAction::Drop;
        let action_style = if dropped {
            Style::new().fg(t.error).add_modifier(Modifier::CROSSED_OUT)
        } else {
            Style::new().fg(t.todo_action)
        };
        let rest_style = if dropped {
            Style::new().dim().add_modifier(Modifier::CROSSED_OUT)
        } else {
            Style::new()
        };
        root.children.push(Section::new(
            0,
            &format!("commit:{}", e.hash),
            SectionValue::Commit {
                hash: e.hash.clone(),
            },
            Line::from(vec![
                // Widest action word is "reword"/"squash" (6); pad to align.
                Span::styled(format!("{:<7}", e.action.word()), action_style),
                Span::styled(e.hash.clone(), rest_style.patch(Style::new().fg(t.hash))),
                Span::styled(format!(" {}", e.subject), rest_style),
            ]),
        ));
    }
    let mut hints = Section::new(0, "hints", SectionValue::Text, Line::default());
    for l in [
        "p pick  r reword  e edit  s squash  f fixup  d drop",
        "M-j/M-k move commit  RET show commit  C-c C-c rebase  C-c C-k abort",
    ] {
        hints
            .body
            .push(Line::from(Span::styled(l.to_string(), Style::new().dim())));
    }
    root.children.push(hints);
    root
}

/// The `$` buffer: every git command run by the app, newest last.
pub fn build_process_log(t: &Theme, entries: &[ProcessEntry]) -> Section {
    let mut root = Section::root();
    if entries.is_empty() {
        root.body.push(Line::from(Span::styled(
            "No git commands run yet.".to_string(),
            Style::new().dim(),
        )));
        return root;
    }
    for (i, e) in entries.iter().enumerate() {
        let status_style = if e.status == 0 {
            Style::new().fg(t.success)
        } else {
            Style::new().fg(t.error)
        };
        let mut sec = Section::new(
            0,
            &format!("proc:{i}"),
            SectionValue::Text,
            Line::from(vec![
                Span::styled(format!("[{}] ", e.status), status_style),
                Span::styled(e.cmd.clone(), heading_style()),
            ]),
        );
        for l in e.output.lines() {
            sec.body
                .push(Line::from(Span::styled(l.to_string(), Style::new().dim())));
        }
        root.children.push(sec);
    }
    root
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(hash: &str, refs: &str, subject: &str) -> LogEntry {
        LogEntry {
            hash: hash.into(),
            refs: refs.into(),
            subject: subject.into(),
            author: "Ada".into(),
            date: "2 days ago".into(),
        }
    }

    #[test]
    fn log_decorations_drop_head_and_drop_parentheses() {
        let t = Theme::default();
        let root = build_log(
            &t,
            "Commits in HEAD",
            &[entry(
                "abc123",
                "HEAD -> main, origin/main, origin/HEAD, tag: v1.0",
                "fix: thing",
            )],
        );
        let heading = root.children[0].heading.to_string();
        // magit-style: no parens, the "HEAD ->" pointer and origin/HEAD elided,
        // the current branch and remaining refs shown as tokens.
        assert_eq!(heading, "abc123 main origin/main v1.0 fix: thing");
    }

    #[test]
    fn log_ref_tokens_carry_type_colors() {
        let t = Theme::default();
        let root = build_log(&t, "Log HEAD", &[entry("abc", "origin/main, tag: v1", "s")]);
        let fgs: Vec<_> = root.children[0]
            .heading
            .spans
            .iter()
            .map(|s| (s.content.to_string(), s.style.fg))
            .collect();
        assert!(fgs.contains(&("origin/main".to_string(), Some(t.branch_remote))));
        assert!(fgs.contains(&("v1".to_string(), Some(t.tag))));
    }

    #[test]
    fn log_author_and_date_are_colored_columns_in_the_margin() {
        let t = Theme::default();
        let root = build_log(&t, "Log HEAD", &[entry("abc", "", "subject")]);
        let sec = &root.children[0];
        // Author/date are not in the heading — they live in the right margin.
        assert_eq!(sec.heading.to_string(), "abc subject");
        let margin = sec.margin.as_ref().unwrap();
        assert_eq!(margin.to_string(), "Ada  2 days ago");
        let fgs: Vec<_> = margin.spans.iter().map(|s| s.style.fg).collect();
        assert!(fgs.contains(&Some(t.log_author)));
        assert!(fgs.contains(&Some(t.log_date)));
    }

    #[test]
    fn log_header_names_the_range_magit_style() {
        let t = Theme::default();
        let root = build_log(
            &t,
            "Commits in HEAD",
            &[entry("a", "", "s1"), entry("b", "", "s2")],
        );
        // First body line is the header verbatim; commits are children.
        assert_eq!(root.body[0].to_string(), "Commits in HEAD");
        assert_eq!(root.children.len(), 2);
    }

    #[test]
    fn log_header_is_a_colored_full_width_bar() {
        let t = Theme::default();
        let root = build_log(&t, "Commits in HEAD", &[entry("a", "", "s")]);
        // The header carries the header colors, and the root requests a
        // full-width fill in the header background so it renders as a bar.
        let style = root.body[0].spans[0].style;
        assert_eq!(style.fg, Some(t.header_fg));
        assert_eq!(style.bg, Some(t.header_bg));
        assert_eq!(root.body_fill, Some(t.header_bg));
    }

    #[test]
    fn log_has_no_blank_line_under_the_header() {
        use crate::ui::section::flatten;
        let t = Theme::default();
        let root = build_log(&t, "Commits in HEAD", &[entry("a", "", "s1")]);
        let texts: Vec<String> = flatten(&root).iter().map(|f| f.line.to_string()).collect();
        // Header immediately followed by the first commit — no spacer.
        assert_eq!(texts, vec!["Commits in HEAD", "a s1"]);
    }

    #[test]
    fn rebase_todo_rows_are_commit_sections_with_action_words() {
        let t = Theme::default();
        let entries = vec![
            TodoEntry {
                action: TodoAction::Pick,
                hash: "abc".into(),
                subject: "one".into(),
            },
            TodoEntry {
                action: TodoAction::Drop,
                hash: "def".into(),
                subject: "two".into(),
            },
        ];
        let root = build_rebase_todo(&t, "Interactive rebase onto main", &entries);
        assert_eq!(root.body[0].to_string(), "Interactive rebase onto main");
        assert_eq!(root.children[0].heading.to_string(), "pick   abc one");
        assert_eq!(
            root.children[0].value,
            SectionValue::Commit { hash: "abc".into() }
        );
        // Dropped rows are struck through in the error color.
        let drop_span = &root.children[1].heading.spans[0];
        assert_eq!(drop_span.style.fg, Some(t.error));
        assert!(drop_span
            .style
            .add_modifier
            .contains(Modifier::CROSSED_OUT));
        // The trailing child is the dim key-hint block, not a commit.
        assert_eq!(root.children.last().unwrap().value, SectionValue::Text);
    }

    #[test]
    fn log_root_is_compact() {
        let t = Theme::default();
        let root = build_log(&t, "Log HEAD", &[entry("abc", "", "subject")]);
        assert!(root.compact);
    }
}
