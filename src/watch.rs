//! File hot-reload: a stream that yields which of the watched files
//! changed (the config, the theme). The `notify` watcher runs its own
//! thread and feeds a channel the stream reads from; the watcher lives as
//! long as the stream.

use std::path::PathBuf;
use std::time::Duration;

use iced::Subscription;
use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, Stream, StreamExt};
use notify::{EventKind, RecursiveMode, Watcher};

/// The watched files written, created or removed in one burst.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed(pub Vec<PathBuf>);

/// Watch `files` for changes. The subscription is keyed on the file
/// list, so a different set (a theme switch) restarts it.
pub fn watch(files: &[PathBuf]) -> Subscription<Changed> {
    if files.is_empty() {
        return Subscription::none();
    }
    Subscription::run_with(files.to_vec(), |files| events(files.clone()))
}

fn events(files: Vec<PathBuf>) -> impl Stream<Item = Changed> {
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
        log::debug!("watching {files:?}");

        let touched = |event: &notify::Event, changed: &mut Vec<PathBuf>| {
            if matches!(
                event.kind,
                EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
            ) {
                for p in &event.paths {
                    if files.contains(p) && !changed.contains(p) {
                        changed.push(p.clone());
                    }
                }
            }
        };
        while let Some(event) = rx.next().await {
            let mut changed = Vec::new();
            touched(&event, &mut changed);
            if changed.is_empty() {
                continue;
            }
            // Let the write finish, then collapse the burst into one.
            tokio::time::sleep(SETTLE).await;
            while let Ok(event) = rx.try_recv() {
                touched(&event, &mut changed);
            }
            if output.send(Changed(changed)).await.is_err() {
                break;
            }
        }
    })
}
