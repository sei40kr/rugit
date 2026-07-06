//! The tag transient: creating asks for the name, then for the revision to
//! place the tag on (defaulting to the commit at point); the flags and the
//! name ride across both inputs as captures. Annotated/signed/edited tags
//! stop for a message, so they hand the terminal to $EDITOR.

use crate::app::{svec, App, EditorRequest};
use crate::ui::transient::TransientAction;

impl App {
    pub(super) fn tag_action(&mut self, action: TransientAction, args: Vec<String>) {
        match action {
            TransientAction::TagCreate => {
                self.open_input("Tag name", move |app, name| {
                    let revs = app.list_revs_at_point();
                    app.open_picker("Place tag on", revs, move |app, rev| {
                        // A tag that takes a message stops in $EDITOR; a
                        // lightweight one runs headless.
                        let needs_editor = args.iter().any(|f| {
                            f == "--annotate"
                                || f == "--sign"
                                || f == "--edit"
                                || f.starts_with("--local-user=")
                        });
                        let mut cli = svec(&["tag"]);
                        cli.extend(args);
                        cli.push(name.clone());
                        cli.push(rev.clone());
                        let desc = format!("tag {name} at {rev}");
                        if needs_editor {
                            app.editor_request = Some(EditorRequest::new(desc, cli));
                        } else {
                            app.run_git_bg(desc, cli, None);
                        }
                    });
                });
            }
            TransientAction::TagDelete => {
                let tags = self.list_tags();
                self.open_strict_picker("Delete tag", tags, "no tags", |app, tag| {
                    app.run_git_bg(
                        format!("delete tag {tag}"),
                        svec(&["tag", "-d", &tag]),
                        None,
                    );
                });
            }
            _ => unreachable!("not a tag action"),
        }
    }

    /// Tag names for the delete picker, newest first. Listing refs is a
    /// fast local read, like `list_branches`.
    pub(super) fn list_tags(&self) -> Vec<String> {
        self.git
            .run(&["tag", "--list", "--sort=-creatordate"])
            .map(|o| o.stdout)
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect()
    }
}
