//! Animation: easing, the values that move (paint only: layout never
//! animates), and what a widget keeps of them between frames. See
//! the crate's ARCHITECTURE.md, "Animation".
//!
//! A transition starts at draw, where a widget's target style is
//! resolved (its own states, its node, its ancestors' through `Theme`),
//! and is kept in its tree state behind `Cell`s: draw has the tree
//! read-only. It's anchored to the instant of its cause, not to the
//! frame that first draws it. Frames are asked from `update`, on each
//! `RedrawRequested`, while something moves.

use std::cell::Cell;
use std::sync::Arc;
use std::time::Instant;

use iced::advanced::Shell;
use iced::{Color, Event, window};

use crate::Theme;
use crate::style::{
    AnimationDirection, Computed, Easing, FillMode, Keyframes, Moving, Node, Shadow, StepPosition,
    Transform,
};

impl Easing {
    /// The progress at `t` (0 to 1) of the time.
    pub fn at(self, t: f32) -> f32 {
        match self {
            Easing::Linear => t,
            Easing::CubicBezier(x1, y1, x2, y2) => cubic_bezier(x1, y1, x2, y2, t),
            Easing::Steps(n, position) => steps(n, position, t),
        }
    }
}

/// CSS's cubic Bézier easing: the curve's y where its x is `t`.
fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    if t <= 0.0 || t >= 1.0 {
        return t.clamp(0.0, 1.0);
    }
    let curve = |a: f32, b: f32, u: f32| {
        let v = 1.0 - u;
        3.0 * v * v * u * a + 3.0 * v * u * u * b + u * u * u
    };
    let slope = |a: f32, b: f32, u: f32| {
        let v = 1.0 - u;
        3.0 * v * v * a + 6.0 * v * u * (b - a) + 3.0 * u * u * (1.0 - b)
    };
    // Newton's method from t, bisection where it falters.
    let mut u = t;
    for _ in 0..8 {
        let error = curve(x1, x2, u) - t;
        if error.abs() < 1e-6 {
            return curve(y1, y2, u);
        }
        let d = slope(x1, x2, u);
        if d.abs() < 1e-6 {
            break;
        }
        u -= error / d;
    }
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    u = t;
    for _ in 0..32 {
        let x = curve(x1, x2, u);
        if (x - t).abs() < 1e-6 {
            break;
        }
        if x < t {
            lo = u;
        } else {
            hi = u;
        }
        u = (lo + hi) / 2.0;
    }
    curve(y1, y2, u)
}

/// CSS's `steps()`.
fn steps(n: u32, position: StepPosition, t: f32) -> f32 {
    let n = n.max(1) as f32;
    let mut step = (t * n).floor();
    if matches!(position, StepPosition::JumpStart | StepPosition::JumpBoth) {
        step += 1.0;
    }
    if t >= 0.0 && step < 0.0 {
        step = 0.0;
    }
    let jumps = match position {
        StepPosition::JumpStart | StepPosition::JumpEnd => n,
        StepPosition::JumpNone => (n - 1.0).max(1.0),
        StepPosition::JumpBoth => n + 1.0,
    };
    if t <= 1.0 && step > jumps {
        step = jumps;
    }
    step / jumps
}

/// What moves of a style: its paint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Paint {
    pub color: Color,
    pub background: Color,
    pub border_color: [Color; 4],
    pub outline_color: Color,
    pub opacity: f32,
    /// A transparent one with no offset or blur for none, as CSS
    /// interpolates from it.
    pub shadow: Shadow,
    /// The identity for none.
    pub transform: Transform,
    /// Degrees.
    pub rotate: f32,
}

pub(crate) const NO_SHADOW: Shadow = Shadow {
    x: 0.0,
    y: 0.0,
    blur: 0.0,
    color: Color::TRANSPARENT,
};

impl Paint {
    pub fn of(s: &Computed) -> Self {
        Self {
            color: s.color,
            background: s.background,
            border_color: s.border_color,
            outline_color: s.outline_color,
            opacity: s.opacity,
            shadow: s.shadow.unwrap_or(NO_SHADOW),
            transform: s.transform.unwrap_or(Transform::IDENTITY),
            rotate: s.rotate,
        }
    }

    /// `s` painted as `self` says.
    pub fn onto(&self, s: &Computed) -> Computed {
        let mut out = s.clone();
        out.color = self.color;
        out.background = self.background;
        out.border_color = self.border_color;
        out.outline_color = self.outline_color;
        out.opacity = self.opacity;
        out.shadow = (self.shadow != NO_SHADOW).then_some(self.shadow);
        out.transform = (self.transform != Transform::IDENTITY).then_some(self.transform);
        out.rotate = self.rotate;
        out
    }

    /// `property` of `to` mixed into `self` at `t` from `from`.
    pub fn mix(&mut self, property: Moving, from: &Paint, to: &Paint, t: f32) {
        match property {
            Moving::Color => self.color = color(from.color, to.color, t),
            Moving::Background => self.background = color(from.background, to.background, t),
            Moving::BorderColor => {
                for i in 0..4 {
                    self.border_color[i] = color(from.border_color[i], to.border_color[i], t);
                }
            }
            Moving::OutlineColor => {
                self.outline_color = color(from.outline_color, to.outline_color, t)
            }
            Moving::Opacity => self.opacity = number(from.opacity, to.opacity, t),
            Moving::Shadow => {
                let (a, b) = (from.shadow, to.shadow);
                self.shadow = Shadow {
                    x: number(a.x, b.x, t),
                    y: number(a.y, b.y, t),
                    blur: number(a.blur, b.blur, t).max(0.0),
                    color: color(a.color, b.color, t),
                };
            }
            Moving::Transform => {
                let (a, b) = (from.transform, to.transform);
                self.transform = Transform {
                    scale: number(a.scale, b.scale, t).max(0.0),
                    x: number(a.x, b.x, t),
                    y: number(a.y, b.y, t),
                };
            }
            Moving::Rotate => self.rotate = number(from.rotate, to.rotate, t),
        }
    }
}

fn number(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Mixed premultiplied, as CSS does: from `transparent` a colour fades
/// in, not through grey.
fn color(a: Color, b: Color, t: f32) -> Color {
    if a == b || t <= 0.0 {
        return if t >= 1.0 { b } else { a };
    }
    if t >= 1.0 {
        return b;
    }
    let alpha = number(a.a, b.a, t);
    if alpha <= 0.0 {
        return Color::TRANSPARENT;
    }
    let channel = |x: f32, y: f32| number(x * a.a, y * b.a, t) / alpha;
    Color {
        r: channel(a.r, b.r),
        g: channel(a.g, b.g),
        b: channel(a.b, b.b),
        a: alpha,
    }
}

/// How a property transitions, as the target style says: the last of
/// its `transition`s naming it (or `all`).
fn timing(s: &Computed, property: Moving) -> Option<(f32, f32, Easing)> {
    let t = &s.transitions;
    let i = (0..t.property.len())
        .rev()
        .find(|&i| t.property[i] & property.bit() != 0)?;
    let at = |n: usize| i % n.max(1);
    let duration = t.duration.get(at(t.duration.len())).copied().unwrap_or(0.0);
    let delay = t.delay.get(at(t.delay.len())).copied().unwrap_or(0.0);
    let easing = t
        .easing
        .get(at(t.easing.len()))
        .copied()
        .unwrap_or(Easing::EASE);
    (duration > 0.0).then_some((duration, delay, easing))
}

#[derive(Debug, Clone, Copy)]
struct Running {
    from: Paint,
    to: Paint,
    /// When its cause happened.
    start: Instant,
    /// A reversed transition's durations are cut by this, as CSS's
    /// reversing shortening factor.
    shorten: f32,
}

impl Running {
    /// Its paint at `now`, under `s`'s timings, and whether it's over.
    fn at(&self, s: &Computed, now: Instant) -> (Paint, f32) {
        let elapsed = now.saturating_duration_since(self.start).as_secs_f32();
        let mut paint = self.to;
        let mut least: f32 = 1.0;
        for property in Moving::ALL {
            let Some((duration, delay, easing)) = timing(s, property) else {
                continue;
            };
            let duration = duration * self.shorten;
            let t = ((elapsed - delay * self.shorten) / duration).clamp(0.0, 1.0);
            least = least.min(t);
            paint.mix(property, &self.from, &self.to, easing.at(t));
        }
        (paint, least)
    }
}

/// What a widget keeps of its animations from one frame to the next.
#[derive(Debug, Default)]
pub(crate) struct Motion {
    /// The paint it was last drawn toward: its style's own.
    target: Cell<Option<Paint>>,
    running: Cell<Option<Running>>,
    /// Its keyframe animations: the list (its address) and when they
    /// started.
    animations: Cell<Option<(usize, Instant)>>,
    /// Its animations ended, the list they were: for `update` to report.
    ended: Cell<Option<usize>>,
    /// The list whose end was noted: an end is told once.
    finished: Cell<Option<usize>>,
    /// The last frame's time, from `RedrawRequested`.
    now: Cell<Option<Instant>>,
    /// Something still moves after the frame drawn last.
    moving: Cell<bool>,
}

impl Motion {
    /// A frame's time, from `update`'s `RedrawRequested`: whether to ask
    /// for the next one.
    pub fn frame(&self, now: Instant) -> bool {
        self.now.set(Some(now));
        self.moving.get()
    }

    /// Animations ended since asked last: their list's address.
    pub fn take_ended(&self) -> Option<usize> {
        self.ended.take()
    }

    /// `s` as it's drawn now: its paint on its way from what was shown,
    /// its keyframes over it. `cause`: when what made it change happened
    /// (a state, an ancestor's, its node). `None`: as it is.
    pub fn style(&self, s: &Computed, cause: Option<Instant>) -> Option<Computed> {
        let animated = s.animations.any();
        let has_transitions = s.transitions.any();
        if !animated && !has_transitions && self.running.get().is_none() {
            self.target.set(Some(Paint::of(s)));
            self.moving.set(false);
            return None;
        }
        let now = self.now.get().unwrap_or_else(Instant::now);
        let target = Paint::of(s);
        match self.target.get() {
            Some(old) if old != target => {
                let start = cause.map_or(now, |c| c.min(now));
                let (current, shorten) = match self.running.get() {
                    Some(r) => {
                        let (paint, reached) = r.at(s, now);
                        // Back to where it came from: as long as it went.
                        let shorten = if r.from == target {
                            (reached * r.shorten).clamp(0.05, 1.0)
                        } else {
                            1.0
                        };
                        (paint, shorten)
                    }
                    None => (old, 1.0),
                };
                self.running.set(Some(Running {
                    from: current,
                    to: target,
                    start,
                    shorten,
                }));
            }
            Some(_) => {}
            // Drawn the first time: nothing to come from.
            None => self.running.set(None),
        }
        self.target.set(Some(target));

        let mut paint = target;
        let mut moving = false;
        if let Some(running) = self.running.get() {
            let (now_paint, reached) = running.at(s, now);
            if reached >= 1.0 {
                self.running.set(None);
            } else {
                moving = true;
            }
            paint = now_paint;
        }
        if animated {
            moving |= self.keyframes(s, &mut paint, now);
        } else {
            self.animations.set(None);
        }
        self.moving.set(moving || self.ended.get().is_some());
        (paint != target).then(|| paint.onto(s))
    }

    /// The keyframe animations over `paint`: whether they go on.
    fn keyframes(&self, s: &Computed, paint: &mut Paint, now: Instant) -> bool {
        let list = &s.animations;
        let id = Arc::as_ptr(&list.names) as *const () as usize;
        let start = match self.animations.get() {
            Some((started, at)) if started == id => at,
            _ => {
                self.animations.set(Some((id, now)));
                self.ended.set(None);
                self.finished.set(None);
                now
            }
        };
        let elapsed = now.saturating_duration_since(start).as_secs_f32();
        let base = *paint;
        let mut going = false;
        let mut any = false;
        for (i, keyframes) in list.keyframes.iter().enumerate() {
            let Some(keyframes) = keyframes else {
                continue;
            };
            any = true;
            let pick = |n: usize| i % n.max(1);
            let duration = list
                .duration
                .get(pick(list.duration.len()))
                .copied()
                .unwrap_or(0.0);
            let delay = list
                .delay
                .get(pick(list.delay.len()))
                .copied()
                .unwrap_or(0.0);
            let easing = list
                .easing
                .get(pick(list.easing.len()))
                .copied()
                .unwrap_or(Easing::EASE);
            let count = list
                .count
                .get(pick(list.count.len()))
                .copied()
                .unwrap_or(1.0);
            let direction = list
                .direction
                .get(pick(list.direction.len()))
                .copied()
                .unwrap_or(AnimationDirection::Normal);
            let fill = list
                .fill
                .get(pick(list.fill.len()))
                .copied()
                .unwrap_or(FillMode::None);
            let (progress, active) = progress(elapsed - delay, duration, count, direction, fill);
            going |= active.1;
            if let Some(progress) = progress.filter(|_| active.0) {
                keyframes.apply(&base, paint, progress, easing);
            }
        }
        if any && !going && self.finished.get() != Some(id) {
            self.finished.set(Some(id));
            self.ended.set(Some(id));
        }
        going
    }
}

/// Where an animation is, `elapsed` seconds after its delay: its
/// progress through the keyframes, and (shown, still going).
fn progress(
    elapsed: f32,
    duration: f32,
    count: f32,
    direction: AnimationDirection,
    fill: FillMode,
) -> (Option<f32>, (bool, bool)) {
    let reversed = |iteration: f32| match direction {
        AnimationDirection::Normal => false,
        AnimationDirection::Reverse => true,
        AnimationDirection::Alternate => !(iteration as u64).is_multiple_of(2),
        AnimationDirection::AlternateReverse => (iteration as u64).is_multiple_of(2),
    };
    let at = |iteration: f32, local: f32| {
        if reversed(iteration) {
            1.0 - local
        } else {
            local
        }
    };
    let backwards = matches!(fill, FillMode::Backwards | FillMode::Both);
    let forwards = matches!(fill, FillMode::Forwards | FillMode::Both);
    if elapsed < 0.0 {
        return (Some(at(0.0, 0.0)), (backwards, true));
    }
    let total = duration * count;
    if duration <= 0.0 || elapsed >= total {
        // Over: where its last iteration stopped.
        let (iteration, local) = if !count.is_finite() {
            (0.0, 1.0)
        } else if count <= 0.0 {
            (0.0, 0.0)
        } else if count.fract() == 0.0 {
            (count - 1.0, 1.0)
        } else {
            (count.floor(), count.fract())
        };
        return (Some(at(iteration, local)), (forwards, false));
    }
    let iteration = (elapsed / duration).floor();
    let local = (elapsed - iteration * duration) / duration;
    (Some(at(iteration, local)), (true, true))
}

impl Keyframes {
    /// Each property its keyframes set, at `progress`, over `paint`; the
    /// ones it doesn't set at the ends are `base`'s, as CSS's implicit
    /// keyframes. `easing` between each two.
    fn apply(&self, base: &Paint, paint: &mut Paint, progress: f32, easing: Easing) {
        for property in Moving::ALL {
            if self.sets & property.bit() == 0 {
                continue;
            }
            // The keyframes around `progress` that set it.
            let mut before: (f32, Paint) = (0.0, *base);
            let mut after: (f32, Paint) = (1.0, *base);
            for frame in self.frames.iter() {
                if frame.sets & property.bit() == 0 {
                    continue;
                }
                let value = frame.paint(base);
                if frame.offset <= progress {
                    before = (frame.offset, value);
                }
                if frame.offset >= progress {
                    after = (frame.offset, value);
                    break;
                }
            }
            let span = after.0 - before.0;
            let t = if span > 0.0 {
                (progress - before.0) / span
            } else {
                1.0
            };
            paint.mix(property, &before.1, &after.1, easing.at(t));
        }
    }
}

/// What an animated widget keeps in its tree state: its motion, its
/// node as last seen, when it last changed, whether to ask a frame more.
#[derive(Debug)]
pub(crate) struct Animated {
    pub motion: Motion,
    node: Option<(usize, u32)>,
    /// When its states or its node last changed: what its transitions,
    /// and its descendants', are anchored to.
    pub since: Option<Instant>,
    /// A frame more to ask: a change is drawn (its transitions started)
    /// only after that frame's `update`, so they reach theirs at the
    /// next.
    pending: bool,
}

impl Animated {
    /// A widget's, for its `node`. New, it asks a frame more too: what it
    /// starts (an entry animation) is known at its first draw only, after
    /// the `update` that could ask for the next frame.
    pub fn new(node: Node) -> Self {
        Self {
            motion: Motion::default(),
            node: Some(node.key()),
            since: None,
            pending: true,
        }
    }

    /// A new view: its node may have changed (a class).
    pub fn diff(&mut self, node: Node) {
        let key = node.key();
        if self.node.is_some_and(|seen| seen != key) {
            self.changed();
        }
        self.node = Some(key);
    }

    /// Its states changed.
    pub fn changed(&mut self) {
        self.since = Some(Instant::now());
        self.pending = true;
    }

    /// `update`'s part: on a frame, the next one asked while something
    /// moves. Whether its animations just ended.
    pub fn update<Message>(&mut self, event: &Event, shell: &mut Shell<'_, Message>) -> bool {
        let Event::Window(window::Event::RedrawRequested(now)) = event else {
            return false;
        };
        if self.motion.frame(*now) || self.pending {
            shell.request_redraw();
        }
        self.pending = false;
        self.motion.take_ended().is_some()
    }

    /// `draw`'s part: `s` as drawn now, if it moves.
    pub fn style(&self, s: &Computed, theme: &Theme) -> Option<Computed> {
        self.motion.style(s, self.since.max(theme.changed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn easings_as_css_defines_them() {
        assert!(close(Easing::Linear.at(0.3), 0.3));
        // `ease` at its middle, as browsers compute it.
        assert!(close(Easing::EASE.at(0.5), 0.8024));
        let ease_in_out = Easing::CubicBezier(0.42, 0.0, 0.58, 1.0);
        assert!(close(ease_in_out.at(0.5), 0.5));
        assert!(ease_in_out.at(0.25) < 0.25);
        assert_eq!(Easing::EASE.at(0.0), 0.0);
        assert_eq!(Easing::EASE.at(1.0), 1.0);
        let end = Easing::Steps(4, StepPosition::JumpEnd);
        assert_eq!((end.at(0.0), end.at(0.3), end.at(1.0)), (0.0, 0.25, 1.0));
        let start = Easing::Steps(4, StepPosition::JumpStart);
        assert_eq!((start.at(0.0), start.at(0.3)), (0.25, 0.5));
        let none = Easing::Steps(5, StepPosition::JumpNone);
        assert_eq!((none.at(0.0), none.at(0.5), none.at(1.0)), (0.0, 0.5, 1.0));
    }

    #[test]
    fn colours_mix_premultiplied() {
        // From transparent, a colour fades in: no grey on the way.
        let red = Color::from_rgb(1.0, 0.0, 0.0);
        let half = color(Color::TRANSPARENT, red, 0.5);
        assert!(close(half.r, 1.0) && close(half.a, 0.5), "{half:?}");
        let purple = color(red, Color::from_rgb(0.0, 0.0, 1.0), 0.5);
        assert!(close(purple.r, 0.5) && close(purple.b, 0.5), "{purple:?}");
    }

    #[test]
    fn where_an_animation_is() {
        use AnimationDirection::*;
        let at = |elapsed, count, direction, fill| progress(elapsed, 1.0, count, direction, fill);
        assert_eq!(
            at(0.25, 1.0, Normal, FillMode::None),
            (Some(0.25), (true, true))
        );
        assert_eq!(
            at(1.25, 2.0, Alternate, FillMode::None),
            (Some(0.75), (true, true))
        );
        assert_eq!(
            at(0.25, 1.0, Reverse, FillMode::None),
            (Some(0.75), (true, true))
        );
        // Over: shown only filling forwards, where it stopped.
        assert_eq!(
            at(3.0, 2.0, Normal, FillMode::Forwards),
            (Some(1.0), (true, false))
        );
        assert_eq!(
            at(3.0, 2.0, Alternate, FillMode::Forwards),
            (Some(0.0), (true, false))
        );
        assert_eq!(at(3.0, 2.5, Normal, FillMode::Both).0, Some(0.5));
        assert!(!at(3.0, 1.0, Normal, FillMode::None).1.0);
        // Before its delay: shown only filling backwards.
        assert_eq!(
            at(-0.5, 1.0, Normal, FillMode::Backwards),
            (Some(0.0), (true, true))
        );
        assert!(!at(-0.5, 1.0, Normal, FillMode::None).1.0);
        // Infinite: never over.
        assert!(at(1e6, f32::INFINITY, Normal, FillMode::None).1.1);
    }
}
