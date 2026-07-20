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

        if let Some(text) = app.take_clipboard_request() {
            if let Err(e) = copy_to_clipboard(&text) {
                app.message = Some(format!("copy failed: {e}"));
            }
        }
        if let Some(req) = app.take_editor_request() {
            run_editor(terminal, app, input_paused, req)?;
        }
        if app.should_quit {
            return Ok(());
        }
    }
}

/// Put `text` on the system clipboard. A real clipboard tool is preferred so
/// success or failure is observable via its exit status; when none is
/// installed we fall back to an OSC 52 escape sequence (fire-and-forget, but
/// works over SSH and in a bare terminal — the terminal must have OSC 52
/// enabled, e.g. tmux `set -g set-clipboard on`).
///
/// Returns `Err` only when a clipboard tool *is* present but fails; a missing
/// tool is not an error (we fall through to OSC 52).
fn copy_to_clipboard(text: &str) -> std::io::Result<()> {
    // (binary, args) in preference order: Wayland, X11 (xclip / xsel), macOS.
    const TOOLS: &[(&str, &[&str])] = &[
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard"]),
        ("xsel", &["--clipboard", "--input"]),
        ("pbcopy", &[]),
    ];
    let mut tool_error = None;
    for (bin, args) in TOOLS {
        match run_clipboard_tool(bin, args, text) {
            Ok(true) => return Ok(()), // ran and exited 0
            Ok(false) => {}            // not installed — try the next
            Err(e) => tool_error = Some(e),
        }
    }
    // A tool was present but failed: report it rather than silently falling
    // back, so the failure is visible.
    if let Some(e) = tool_error {
        return Err(e);
    }
    // Nothing installed: best-effort OSC 52.
    osc52(text)
}

/// Pipe `text` into a clipboard `bin`. `Ok(true)` = ran and exited 0,
/// `Ok(false)` = binary not installed (fall through), `Err` = ran but failed.
fn run_clipboard_tool(bin: &str, args: &[&str], text: &str) -> std::io::Result<bool> {
    use std::io::{Error, ErrorKind, Write};
    use std::process::{Command, Stdio};
    let mut child = match Command::new(bin)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    // Close stdin (drop) before waiting so the tool sees EOF.
    {
        let mut stdin = child.stdin.take().expect("piped stdin");
        stdin.write_all(text.as_bytes())?;
    }
    let status = child.wait()?;
    if status.success() {
        Ok(true)
    } else {
        Err(Error::other(format!("{bin} exited with {status}")))
    }
}

/// Write an OSC 52 clipboard escape sequence to the terminal.
fn osc52(text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let seq = format!("\x1b]52;c;{}\x07", base64_encode(text.as_bytes()));
    let mut out = stdout();
    out.write_all(seq.as_bytes())?;
    out.flush()
}

/// Standard base64 (with padding). Small and pure so OSC 52 needs no crate.
fn base64_encode(input: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
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
        .envs(req.envs.iter().cloned())
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

#[cfg(test)]
mod tests {
    use super::base64_encode;

    #[test]
    fn base64_matches_known_vectors() {
        // RFC 4648 test vectors exercise all padding cases.
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }
}
