//! Event sources feeding the main loop: terminal input and the `.git`
//! directory watcher. Both forward into one `AppEvent` channel.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crossbeam_channel::Sender;
use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode, DebounceEventResult, Debouncer};
use ratatui::crossterm::event::{self, Event};

use crate::app::AppEvent;

/// Spawn the terminal input reader. While `paused` is set (an external
/// $EDITOR owns the terminal) the thread must not touch stdin at all.
pub fn spawn_input_thread(tx: Sender<AppEvent>, paused: Arc<AtomicBool>) {
    thread::spawn(move || {
        loop {
            if paused.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(50));
                continue;
            }
            // Poll with a timeout so a pause request takes effect promptly.
            match event::poll(Duration::from_millis(100)) {
                Ok(true) => {
                    if paused.load(Ordering::SeqCst) {
                        continue;
                    }
                    let ev = match event::read() {
                        Ok(ev) => ev,
                        Err(_) => return,
                    };
                    let msg = match ev {
                        Event::Key(k) => AppEvent::Key(k),
                        Event::Resize(_, _) => AppEvent::Resize,
                        _ => continue,
                    };
                    if tx.send(msg).is_err() {
                        return;
                    }
                }
                Ok(false) => {}
                Err(_) => return,
            }
        }
    });
}

/// Watch the git dir so external `git` invocations refresh the status buffer.
/// Returns the debouncer, which must stay alive for the watch to persist.
pub fn spawn_repo_watcher(
    tx: Sender<AppEvent>,
    git_dir: &Path,
) -> Option<Debouncer<notify_debouncer_mini::notify::RecommendedWatcher>> {
    let mut debouncer = new_debouncer(
        Duration::from_millis(200),
        move |res: DebounceEventResult| {
            if res.is_ok() {
                let _ = tx.send(AppEvent::RepoChanged);
            }
        },
    )
    .ok()?;
    // `.git` itself (index, HEAD, MERGE_HEAD, ...) plus refs for branch moves.
    debouncer
        .watcher()
        .watch(git_dir, RecursiveMode::NonRecursive)
        .ok()?;
    let refs = git_dir.join("refs");
    if refs.is_dir() {
        let _ = debouncer.watcher().watch(&refs, RecursiveMode::Recursive);
    }
    Some(debouncer)
}
