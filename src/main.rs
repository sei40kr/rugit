use rugit::{app, config, event, keymap, ui};

use std::io::stdout;
use std::process::Command as ProcessCommand;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::crossterm::ExecutableCommand;
use ratatui::Terminal;

use crate::app::{App, AppEvent, EditorRequest};
use rugit::git::client::GitClient;

fn main() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let git = GitClient::discover(&cwd)
        .context("not inside a git repository (or git is not installed)")?;

    let (cfg, mut warnings) = config::load();
    let mut keymaps = keymap::default_keymaps();
    config::apply_keys(&cfg, &mut keymaps, &mut warnings);
    let mut theme = rugit::theme::Theme::default();
    config::apply_colors(&cfg, &mut theme, &mut warnings);
    let scrolloff = cfg.scrolloff.unwrap_or(3);

    let (tx, rx) = crossbeam_channel::unbounded::<AppEvent>();
    let input_paused = Arc::new(AtomicBool::new(false));
    event::spawn_input_thread(tx.clone(), input_paused.clone());
    let _watcher = event::spawn_repo_watcher(tx.clone(), &git.git_dir);

    let mut app = App::new(git, tx, keymaps, theme, scrolloff);
    if let Some(w) = warnings.first() {
        app.message = Some(w.clone());
    }
    app.refresh();

    // From here on the terminal is in raw mode; make sure panics restore it.
    install_panic_hook();
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    let result = run(&mut terminal, &mut app, &rx, &input_paused);

    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    result
}

fn run(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
    rx: &crossbeam_channel::Receiver<AppEvent>,
    input_paused: &Arc<AtomicBool>,
) -> Result<()> {
    loop {
        terminal.draw(|f| ui::render::draw(f, app))?;

        let ev = rx.recv()?;
        app.update(ev);
        // Coalesce whatever else is queued before redrawing.
        while let Ok(ev) = rx.try_recv() {
            app.update(ev);
        }

        if let Some(req) = app.take_editor_request() {
            run_editor(terminal, app, input_paused, req)?;
        }
        if app.should_quit {
            return Ok(());
        }
    }
}

/// Suspend the TUI and hand the terminal to `git commit` (which launches
/// $GIT_EDITOR), then restore and refresh.
fn run_editor(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
    input_paused: &Arc<AtomicBool>,
    req: EditorRequest,
) -> Result<()> {
    input_paused.store(true, Ordering::SeqCst);
    // Give the input thread time to finish its current poll window so it
    // cannot steal keystrokes destined for the editor.
    std::thread::sleep(std::time::Duration::from_millis(150));
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    let status = ProcessCommand::new("git")
        .args(&req.args)
        .current_dir(&app.git.repo_root)
        .status()
        .map(|s| s.code().unwrap_or(-1))
        .unwrap_or(-1);

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    terminal.clear()?;
    input_paused.store(false, Ordering::SeqCst);

    app.on_editor_done(req.desc, req.args, status);
    Ok(())
}

fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = stdout().execute(LeaveAlternateScreen);
        original(info);
    }));
}
