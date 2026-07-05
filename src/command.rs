//! Commands are data, not closures: keymaps bind keys to `Command` values,
//! config remaps by name, and the help UI enumerates `COMMANDS`.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    Quit,
    Refresh,
    MoveDown,
    MoveUp,
    HalfPageDown,
    HalfPageUp,
    GotoTop,
    GotoBottom,
    NextSection,
    PrevSection,
    ParentSection,
    ToggleSection,
    Stage,
    Unstage,
    StageAll,
    UnstageAll,
    Discard,
    Visit,
    Search,
    TransientCommit,
    TransientBranch,
    TransientPush,
    TransientPull,
    TransientFetch,
    TransientLog,
    Help,
    ProcessLog,
}

pub struct CommandInfo {
    pub cmd: Command,
    pub name: &'static str,
    pub desc: &'static str,
}

pub const COMMANDS: &[CommandInfo] = &[
    ci(Command::Quit, "quit", "Close current buffer (quit if last)"),
    ci(Command::Refresh, "refresh", "Refresh the current buffer"),
    ci(Command::MoveDown, "move-down", "Move cursor down"),
    ci(Command::MoveUp, "move-up", "Move cursor up"),
    ci(
        Command::HalfPageDown,
        "half-page-down",
        "Scroll half a page down",
    ),
    ci(Command::HalfPageUp, "half-page-up", "Scroll half a page up"),
    ci(Command::GotoTop, "goto-top", "Go to the first line"),
    ci(Command::GotoBottom, "goto-bottom", "Go to the last line"),
    ci(
        Command::NextSection,
        "next-section",
        "Jump to next section heading",
    ),
    ci(
        Command::PrevSection,
        "prev-section",
        "Jump to previous section heading",
    ),
    ci(
        Command::ParentSection,
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
    ci(Command::TransientCommit, "commit", "Open the commit menu"),
    ci(Command::TransientBranch, "branch", "Open the branch menu"),
    ci(Command::TransientPush, "push", "Open the push menu"),
    ci(Command::TransientPull, "pull", "Open the pull menu"),
    ci(Command::TransientFetch, "fetch", "Open the fetch menu"),
    ci(Command::TransientLog, "log", "Open the log menu"),
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
