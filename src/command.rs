//! Commands are data, not closures: keymaps bind keys to `Command` values,
//! config remaps by name, and the help UI enumerates `COMMANDS`.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    Quit,
    Refresh,
    Nav(NavCmd),
    ToggleSection,
    Stage,
    Unstage,
    StageAll,
    UnstageAll,
    Discard,
    Visit,
    Search,
    /// Jump to the next/previous match of the active search.
    SearchNext,
    SearchPrev,
    Transient(Menu),
    /// Rebase-todo editing (only bound in the rebase-todo buffer).
    Todo(TodoCmd),
    Help,
    ProcessLog,
    /// Open the references buffer (branches, remotes, tags).
    ShowRefs,
    /// Copy the value at point (commit/ref/file/stash) to the clipboard.
    Copy,
    /// Copy the revision the current buffer is about to the clipboard.
    CopyRevision,
}

/// Rebase-todo buffer commands. Grouped like `NavCmd` so `dispatch`
/// forwards them wholesale to the todo editor instead of growing one arm
/// per action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TodoCmd {
    Pick,
    Reword,
    Edit,
    Squash,
    Fixup,
    Drop,
    MoveUp,
    MoveDown,
    Confirm,
    Abort,
}

/// Pure cursor motions. Grouped so `dispatch` forwards them wholesale to
/// `Pane::navigate` instead of growing one arm per motion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NavCmd {
    MoveDown,
    MoveUp,
    HalfPageDown,
    HalfPageUp,
    PageDown,
    PageUp,
    GotoTop,
    GotoBottom,
    NextSection,
    PrevSection,
    ParentSection,
}

/// Transient menus. `ui::transient::menu_def` maps each to its definition,
/// so opening a new menu never adds a `dispatch` arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Menu {
    Commit,
    Branch,
    Merge,
    Rebase,
    CherryPick,
    Revert,
    Reset,
    Stash,
    Tag,
    Remote,
    Push,
    Pull,
    Fetch,
    Log,
    Submodule,
    Worktree,
}

pub struct CommandInfo {
    pub cmd: Command,
    pub name: &'static str,
    pub desc: &'static str,
}

pub const COMMANDS: &[CommandInfo] = &[
    ci(Command::Quit, "quit", "Close current buffer (quit if last)"),
    ci(Command::Refresh, "refresh", "Refresh the current buffer"),
    ci(
        Command::Nav(NavCmd::MoveDown),
        "move-down",
        "Move cursor down",
    ),
    ci(Command::Nav(NavCmd::MoveUp), "move-up", "Move cursor up"),
    ci(
        Command::Nav(NavCmd::HalfPageDown),
        "half-page-down",
        "Scroll half a page down",
    ),
    ci(
        Command::Nav(NavCmd::HalfPageUp),
        "half-page-up",
        "Scroll half a page up",
    ),
    ci(
        Command::Nav(NavCmd::PageDown),
        "page-down",
        "Scroll a full page down",
    ),
    ci(
        Command::Nav(NavCmd::PageUp),
        "page-up",
        "Scroll a full page up",
    ),
    ci(
        Command::Nav(NavCmd::GotoTop),
        "goto-top",
        "Go to the first line",
    ),
    ci(
        Command::Nav(NavCmd::GotoBottom),
        "goto-bottom",
        "Go to the last line",
    ),
    ci(
        Command::Nav(NavCmd::NextSection),
        "next-section",
        "Jump to next section heading",
    ),
    ci(
        Command::Nav(NavCmd::PrevSection),
        "prev-section",
        "Jump to previous section heading",
    ),
    ci(
        Command::Nav(NavCmd::ParentSection),
        "parent-section",
        "Jump to parent section",
    ),
    ci(
        Command::ToggleSection,
        "toggle-section",
        "Collapse/expand section at point",
    ),
    ci(
        Command::Stage,
        "stage",
        "Stage the thing at point (file/hunk/line)",
    ),
    ci(Command::Unstage, "unstage", "Unstage the thing at point"),
    ci(Command::StageAll, "stage-all", "Stage all tracked changes"),
    ci(
        Command::UnstageAll,
        "unstage-all",
        "Unstage all staged changes",
    ),
    ci(Command::Discard, "discard", "Discard the change at point"),
    ci(
        Command::Visit,
        "visit",
        "Show the thing at point (commit/stash)",
    ),
    ci(
        Command::Search,
        "search",
        "Incremental search in the buffer",
    ),
    ci(
        Command::SearchNext,
        "search-next",
        "Jump to the next search match",
    ),
    ci(
        Command::SearchPrev,
        "search-prev",
        "Jump to the previous search match",
    ),
    ci(
        Command::Transient(Menu::Commit),
        "commit",
        "Open the commit menu",
    ),
    ci(
        Command::Transient(Menu::Branch),
        "branch",
        "Open the branch menu",
    ),
    ci(
        Command::Transient(Menu::Merge),
        "merge",
        "Open the merge menu",
    ),
    ci(
        Command::Transient(Menu::Rebase),
        "rebase",
        "Open the rebase menu",
    ),
    ci(
        Command::Transient(Menu::CherryPick),
        "cherry-pick",
        "Open the cherry-pick menu",
    ),
    ci(
        Command::Transient(Menu::Revert),
        "revert",
        "Open the revert menu",
    ),
    ci(
        Command::Transient(Menu::Reset),
        "reset",
        "Open the reset menu",
    ),
    ci(
        Command::Transient(Menu::Stash),
        "stash",
        "Open the stash menu",
    ),
    ci(Command::Transient(Menu::Tag), "tag", "Open the tag menu"),
    ci(
        Command::Transient(Menu::Remote),
        "remote",
        "Open the remote menu",
    ),
    ci(Command::Transient(Menu::Push), "push", "Open the push menu"),
    ci(Command::Transient(Menu::Pull), "pull", "Open the pull menu"),
    ci(
        Command::Transient(Menu::Fetch),
        "fetch",
        "Open the fetch menu",
    ),
    ci(Command::Transient(Menu::Log), "log", "Open the log menu"),
    ci(
        Command::Transient(Menu::Submodule),
        "submodule",
        "Open the submodule menu",
    ),
    ci(
        Command::Transient(Menu::Worktree),
        "worktree",
        "Open the worktree menu",
    ),
    ci(
        Command::Todo(TodoCmd::Pick),
        "todo-pick",
        "Rebase todo: use this commit",
    ),
    ci(
        Command::Todo(TodoCmd::Reword),
        "todo-reword",
        "Rebase todo: use commit, edit its message",
    ),
    ci(
        Command::Todo(TodoCmd::Edit),
        "todo-edit",
        "Rebase todo: use commit, stop for amending",
    ),
    ci(
        Command::Todo(TodoCmd::Squash),
        "todo-squash",
        "Rebase todo: meld into previous commit",
    ),
    ci(
        Command::Todo(TodoCmd::Fixup),
        "todo-fixup",
        "Rebase todo: meld into previous, discard message",
    ),
    ci(
        Command::Todo(TodoCmd::Drop),
        "todo-drop",
        "Rebase todo: remove this commit",
    ),
    ci(
        Command::Todo(TodoCmd::MoveUp),
        "todo-move-up",
        "Rebase todo: move commit up",
    ),
    ci(
        Command::Todo(TodoCmd::MoveDown),
        "todo-move-down",
        "Rebase todo: move commit down",
    ),
    ci(
        Command::Todo(TodoCmd::Confirm),
        "todo-confirm",
        "Rebase todo: confirm and run the rebase",
    ),
    ci(
        Command::Todo(TodoCmd::Abort),
        "todo-abort",
        "Rebase todo: close without rebasing",
    ),
    ci(Command::Help, "help", "Show key bindings"),
    ci(
        Command::ProcessLog,
        "process-log",
        "Show the git process log",
    ),
    ci(
        Command::ShowRefs,
        "show-refs",
        "Show branches, remotes and tags",
    ),
    ci(
        Command::Copy,
        "copy",
        "Copy the value at point to the clipboard",
    ),
    ci(
        Command::CopyRevision,
        "copy-revision",
        "Copy the buffer's revision to the clipboard",
    ),
];

const fn ci(cmd: Command, name: &'static str, desc: &'static str) -> CommandInfo {
    CommandInfo { cmd, name, desc }
}

pub fn by_name(name: &str) -> Option<Command> {
    COMMANDS.iter().find(|c| c.name == name).map(|c| c.cmd)
}

pub fn info(cmd: Command) -> &'static CommandInfo {
    COMMANDS
        .iter()
        .find(|c| c.cmd == cmd)
        .expect("every Command has a COMMANDS entry")
}
