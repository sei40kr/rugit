//! Builders that turn git snapshots into styled section trees. All buffer
//! kinds funnel through the same `Section` machinery in `section.rs`.
//! Colors come exclusively from `Theme` so the scheme is configurable.

use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};

use crate::git::client::ProcessEntry;
use crate::git::types::{DiffArea, FileDiff, StatusSnapshot};
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
