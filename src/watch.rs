//! Hot-reload: a stream that yields which of the watched paths changed
//! (the config, the theme, the icon directories). A file is reported
//! when it's written, created or removed; a directory when anything
//! directly inside it is. The `notify` watcher runs its own thread and
//! feeds a channel the stream reads from; the watcher lives as long as
//! the stream.

use std::path::PathBuf;
use std::time::Duration;

use iced::Subscription;
use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, Stream, StreamExt};
use notify::{EventKind, RecursiveMode, Watcher};

/// The watched paths (files, or directories with a change inside)
/// touched in one burst.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed(pub Vec<PathBuf>);

/// Watch `paths` (files or directories) for changes. The subscription is
/// keyed on the list, so a different set (a theme switch) restarts it.
pub fn watch(paths: &[PathBuf]) -> Subscription<Changed> {
    if paths.is_empty() {
        return Subscription::none();
    }
    Subscription::run_with(paths.to_vec(), |paths| events(paths.clone()))
}

fn events(paths: Vec<PathBuf>) -> impl Stream<Item = Changed> {
    // Editors save by writing a temp file and renaming it over the
    // original, so for a file watch its directory and filter on the
    // name. Bursts (an editor save, a package install touching hundreds
    // of files) are reported once, after they go quiet.
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
        let (dirs, files): (Vec<PathBuf>, Vec<PathBuf>) =
            paths.into_iter().partition(|p| p.is_dir());
        let mut to_watch: Vec<PathBuf> = files
            .iter()
            .filter_map(|f| f.parent().map(PathBuf::from))
            .chain(dirs.iter().cloned())
            .collect();
        to_watch.sort();
        to_watch.dedup();
        for dir in &to_watch {
            if let Err(e) = watcher.watch(dir, RecursiveMode::NonRecursive) {
                log::error!("cannot watch {}: {e}", dir.display());
            }
        }
        log::debug!(
            "watching {} file(s) and {} dir(s) ({} inotify watches)",
            files.len(),
            dirs.len(),
            to_watch.len()
        );

        let touched = |event: &notify::Event, changed: &mut Vec<PathBuf>| {
            if !matches!(
                event.kind,
                EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
            ) {
                return;
            }
            for p in &event.paths {
                let hit = if files.contains(p) {
                    Some(p)
                } else {
                    p.parent().and_then(|d| dirs.iter().find(|w| *w == d))
                };
                if let Some(hit) = hit
                    && !changed.contains(hit)
                {
                    changed.push(hit.clone());
                }
            }
        };
        while let Some(event) = rx.next().await {
            let mut changed = Vec::new();
            touched(&event, &mut changed);
            if changed.is_empty() {
                continue;
            }
            // Wait for the burst to go quiet.
            loop {
                tokio::time::sleep(SETTLE).await;
                let mut more = false;
                while let Ok(event) = rx.try_recv() {
                    touched(&event, &mut changed);
                    more = true;
                }
                if !more {
                    break;
                }
            }
            if output.send(Changed(changed)).await.is_err() {
                break;
            }
        }
    })
}
