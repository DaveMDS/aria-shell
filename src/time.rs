//! Wall-clock ticks for whoever shows the time: the Clock gadget, the
//! lock screen, the notifications' ages, the system monitor's sampler.

use std::time::Duration;

use chrono::{DateTime, Local, Timelike};
use iced::futures::stream;

/// Crude but sufficient: any strftime specifier that renders seconds
/// or finer, so a display knows whether to tick every second.
pub fn shows_seconds(format: &str) -> bool {
    [
        "%S", "%T", "%X", "%s", "%f", "%.f", "%3f", "%6f", "%9f", "%c", "%+",
    ]
    .iter()
    .any(|spec| format.contains(spec))
}

/// Yields the current time every `step` seconds, on the boundary, so a
/// displayed value never lags or skips.
pub fn aligned_ticks(step: u32) -> impl stream::Stream<Item = DateTime<Local>> {
    stream::unfold((), move |()| async move {
        let now = Local::now();
        let elapsed = now.second() % step;
        let wait = Duration::from_secs(u64::from(step - elapsed))
            .saturating_sub(Duration::from_nanos(u64::from(now.nanosecond())));
        tokio::time::sleep(wait).await;
        Some((Local::now(), ()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seconds_detection() {
        assert!(shows_seconds("%H:%M:%S"));
        assert!(shows_seconds("%T"));
        assert!(!shows_seconds("%H:%M"));
        assert!(!shows_seconds("%e %b %Y  %H:%M"));
    }
}
