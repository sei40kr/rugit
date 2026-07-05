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
    Transient(Menu),
    Help,
    ProcessLog,
}

/// Pure cursor motions. Grouped so `dispatch` forwards them wholesale to
/// `Pane::navigate` instead of growing one arm per motion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NavCmd {
    MoveDown,
    MoveUp,
    HalfPageDown,
    HalfPageUp,
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
    Push,
    Pull,
    Fetch,
    Log,
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
        Command::Transient(Menu::Commit),
        "commit",
        "Open the commit menu",
    ),
    ci(
        Command::Transient(Menu::Branch),
        "branch",
        "Open the branch menu",
    ),
    ci(Command::Transient(Menu::Push), "push", "Open the push menu"),
    ci(Command::Transient(Menu::Pull), "pull", "Open the pull menu"),
    ci(
        Command::Transient(Menu::Fetch),
        "fetch",
        "Open the fetch menu",
    ),
    ci(Command::Transient(Menu::Log), "log", "Open the log menu"),
    ci(Command::Help, "help", "Show key bindings"),
    ci(
        Command::ProcessLog,
        "process-log",
        "Show the git process log",
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
