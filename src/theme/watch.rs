//! Theme hot-reload: a stream that yields when one of the theme files
//! changes. The `notify` watcher runs its own thread and feeds a channel
//! the stream reads from; the watcher lives as long as the stream.

use std::path::PathBuf;
use std::time::Duration;

use iced::Subscription;
use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, Stream, StreamExt};
use notify::{EventKind, RecursiveMode, Watcher};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// At least one theme file was written, created or removed.
    Changed,
}

/// Watch `files` for changes. The subscription is keyed on the file
/// list, so a theme switch restarts it with the new files.
pub fn watch(files: &[PathBuf]) -> Subscription<Event> {
    if files.is_empty() {
        return Subscription::none();
    }
    Subscription::run_with(files.to_vec(), |files| events(files.clone()))
}

fn events(files: Vec<PathBuf>) -> impl Stream<Item = Event> {
    // Editors save by writing a temp file and renaming it over the
    // original, so watch the directories and filter on the file names.
    const SETTLE: Duration = Duration::from_millis(150);

    iced::stream::channel(8, async move |mut output| {
        let (tx, mut rx) = mpsc::unbounded::<notify::Event>();
        let mut watcher = match notify::recommended_watcher(move |res| match res {
            Ok(event) => {
                let _ = tx.unbounded_send(event);
            }
            Err(e) => log::warn!("theme watcher: {e}"),
        }) {
            Ok(w) => w,
            Err(e) => {
                log::error!("cannot watch theme files: {e}");
                return;
            }
        };
        let mut dirs: Vec<PathBuf> = files
            .iter()
            .filter_map(|f| f.parent().map(PathBuf::from))
            .collect();
        dirs.sort();
        dirs.dedup();
        for dir in &dirs {
            if let Err(e) = watcher.watch(dir, RecursiveMode::NonRecursive) {
                log::error!("cannot watch {}: {e}", dir.display());
            }
        }
        log::debug!("watching theme files {files:?}");

        let relevant = |event: &notify::Event| {
            matches!(
                event.kind,
                EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
            ) && event.paths.iter().any(|p| files.contains(p))
        };
        while let Some(event) = rx.next().await {
            if !relevant(&event) {
                continue;
            }
            // Let the write finish, then collapse the burst into one.
            tokio::time::sleep(SETTLE).await;
            while rx.try_recv().is_ok() {}
            if output.send(Event::Changed).await.is_err() {
                break;
            }
        }
    })
}
