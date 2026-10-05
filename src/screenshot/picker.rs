//! The picker: what `aria-shell screenshot` opens. The outputs frozen,
//! one overlay surface each showing its own picture, and on them a
//! selection to make: a click picks the window under the pointer (or
//! the output, on bare desktop), a drag draws an area; either is then
//! resized by its edges and corners, moved by its inside. A toolbar on
//! the selection's output takes it (save, copy, edit) or all the
//! outputs; Enter takes it the command's way, Escape or a right click
//! cancels.
//!
//! Everything is in the global logical space: the pointer's position
//! on a surface plus that surface's place. However the compositor
//! routes the pointer (a drag keeps sending to the surface it started
//! on, past its edges) the selection follows. A selection stays on the
//! output it started on.

use std::sync::Arc;

use iced::keyboard::{self, key::Named};
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::widget::{Space, container, image, stack};
use iced::{
    Alignment, Color, ContentFit, Element, Event, Length, Point, Rectangle, Renderer, Size,
    Subscription, mouse, window::Id,
};
use iced_exwlshell::reexport::{
    Anchor, KeyboardInteractivity, Layer, LayerSize, NewLayerShellSettings, OutputOption,
};
use iced_wayland_subscriber::OutputId;

use super::Destination;
use super::pixels::{Rect, Shot};
use crate::locale::Locale;
use crate::theme::{self, Node, Theme};

/// How near an edge, in logical pixels, a press grabs it.
const GRIP: f32 = 10.0;
/// How far a press must go to be a drag, not a click.
const SLOP: f32 = 4.0;

pub struct Picker {
    surfaces: Vec<Surface>,
    /// The frozen outputs, what the picture is cut from.
    pub shots: Arc<Vec<Shot>>,
    /// The windows on screen, topmost first.
    windows: Vec<Rect>,
    selection: Option<Selection>,
    drag: Option<Drag>,
    /// Last seen, global.
    cursor: Option<Point>,
    /// The surface with the keyboard: the toolbar's, without a
    /// selection.
    focused: usize,
    /// What Enter does.
    destination: Destination,
    can_copy: bool,
    can_edit: bool,
}

/// One output's surface.
pub struct Surface {
    pub window: Id,
    pub output: OutputId,
    name: String,
    rect: Rect,
    picture: image::Handle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Selection {
    rect: Rect,
    /// The surface it's on.
    surface: usize,
}

enum Drag {
    /// A press on nothing selected: a click picks, a drag draws.
    New {
        from: Point,
        surface: usize,
        moved: bool,
    },
    Move {
        from: Point,
        start: Rect,
    },
    Resize {
        from: Point,
        start: Rect,
        edges: Edges,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Edges {
    left: bool,
    right: bool,
    top: bool,
    bottom: bool,
}

/// What the outputs to open the picker on show and are called.
pub struct Output {
    pub id: OutputId,
    pub global: u32,
    pub name: String,
    pub focused: bool,
}

/// What the surfaces of a picker take as given.
pub struct Setup {
    pub shots: Arc<Vec<Shot>>,
    pub windows: Vec<Rect>,
    pub destination: Destination,
    pub can_copy: bool,
    pub can_edit: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// The pointer on a window, surface-local.
    Moved(Id, Point),
    /// Left button, on nothing that took it (a toolbar button does),
    /// surface-local: from the overlay, which knows where the pointer
    /// is even when it never moved on this surface (iced reports an
    /// enter without a position).
    Pressed(Id, Point),
    Released,
    /// Enter.
    Confirm,
    /// Escape, a right click, the button.
    Cancel,
    /// A toolbar button: the selection, this way.
    Take(Destination),
    /// Every output, Enter's way.
    All,
}

pub enum Action {
    /// These surfaces show something else now.
    Redraw(Vec<Id>),
    Cancel,
    Take { rect: Rect, destination: Destination },
}

/// What a surface draws, worked out from the state, before and after a
/// message: the surfaces it differs on are redrawn.
#[derive(PartialEq, Eq)]
struct Look {
    selection: Option<Rect>,
    hover: Option<Rect>,
    toolbar: usize,
}

impl Picker {
    /// The picker and its surfaces, one per output with a shot.
    pub fn open(setup: Setup, outputs: &[Output]) -> (Self, Vec<(Id, NewLayerShellSettings)>) {
        let mut surfaces = Vec::new();
        let mut settings = Vec::new();
        let mut focused = 0;
        for shot in setup.shots.iter() {
            let Some(output) = outputs.iter().find(|o| o.global == shot.output) else {
                continue;
            };
            if output.focused {
                focused = surfaces.len();
            }
            let window = Id::unique();
            let (w, h) = shot.image.dimensions();
            surfaces.push(Surface {
                window,
                output: output.id.clone(),
                name: output.name.clone(),
                rect: shot.rect,
                picture: image::Handle::from_rgba(w, h, shot.image.as_raw().clone()),
            });
            settings.push((
                window,
                NewLayerShellSettings {
                    anchor: Anchor::all(),
                    size: LayerSize::FILL,
                    layer: Layer::Overlay,
                    exclusive_zone: Some(-1),
                    margin: None,
                    // `OnDemand` takes the keyboard on map and on a
                    // click, and unlike `Exclusive` leaves the pointer
                    // to every surface on Hyprland; on all of them, or a
                    // click on one without would take the keyboard
                    // away (Sway focuses that output's workspace).
                    keyboard_interactivity: KeyboardInteractivity::OnDemand,
                    output_option: OutputOption::GlobalName(output.global),
                    namespace: Some("aria-screenshot".to_owned()),
                    ..Default::default()
                },
            ));
        }
        let picker = Self {
            surfaces,
            shots: setup.shots,
            windows: setup.windows,
            selection: None,
            drag: None,
            cursor: None,
            focused,
            destination: setup.destination,
            can_copy: setup.can_copy,
            can_edit: setup.can_edit,
        };
        (picker, settings)
    }

    pub fn surfaces(&self) -> &[Surface] {
        &self.surfaces
    }

    pub fn has_window(&self, window: Id) -> bool {
        self.surface_of(window).is_some()
    }

    fn surface_of(&self, window: Id) -> Option<usize> {
        self.surfaces.iter().position(|s| s.window == window)
    }

    /// The surface whose output holds `p`.
    fn surface_at(&self, p: Point) -> Option<usize> {
        self.surfaces.iter().position(|s| contains(s.rect, p))
    }

    pub fn subscription() -> Subscription<Message> {
        iced::event::listen_with(|event, _status, window| match event {
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                Some(Message::Moved(window, position))
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                Some(Message::Released)
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                Some(Message::Cancel)
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(named),
                ..
            }) => match named {
                Named::Escape => Some(Message::Cancel),
                Named::Enter => Some(Message::Confirm),
                _ => None,
            },
            _ => None,
        })
    }

    pub fn update(&mut self, message: Message) -> Action {
        let before = self.look();
        match message {
            Message::Moved(window, p) => {
                let Some(i) = self.surface_of(window) else {
                    return Action::Redraw(Vec::new());
                };
                let origin = self.surfaces[i].rect;
                let p = Point::new(p.x + origin.x as f32, p.y + origin.y as f32);
                self.cursor = Some(p);
                self.drag_to(p);
            }
            Message::Pressed(window, p) => {
                let Some(i) = self.surface_of(window) else {
                    return Action::Redraw(Vec::new());
                };
                let origin = self.surfaces[i].rect;
                let p = Point::new(p.x + origin.x as f32, p.y + origin.y as f32);
                self.cursor = Some(p);
                self.press(p);
            }
            Message::Released => self.release(),
            Message::Confirm => return self.take(self.destination),
            Message::Take(destination) => return self.take(destination),
            Message::Cancel => return Action::Cancel,
            Message::All => {
                let all = self.surfaces.iter().map(|s| s.rect).reduce(|a, b| a.union(&b));
                return match all {
                    Some(rect) => Action::Take {
                        rect,
                        destination: self.destination,
                    },
                    None => Action::Cancel,
                };
            }
        }
        Action::Redraw(self.changed(&before, &self.look()))
    }

    fn take(&self, destination: Destination) -> Action {
        match self.selection {
            Some(s) => Action::Take {
                rect: s.rect,
                destination,
            },
            None => Action::Redraw(Vec::new()),
        }
    }

    fn press(&mut self, p: Point) {
        if let Some(s) = self.selection {
            let edges = grip(s.rect, p);
            if edges != Edges::default() {
                self.drag = Some(Drag::Resize {
                    from: p,
                    start: s.rect,
                    edges,
                });
                return;
            }
            if contains(s.rect, p) {
                self.drag = Some(Drag::Move {
                    from: p,
                    start: s.rect,
                });
                return;
            }
        }
        if let Some(surface) = self.surface_at(p) {
            self.drag = Some(Drag::New {
                from: p,
                surface,
                moved: false,
            });
        }
    }

    fn drag_to(&mut self, p: Point) {
        let Some(drag) = &mut self.drag else {
            return;
        };
        let (rect, surface) = match drag {
            Drag::New {
                from,
                surface,
                moved,
            } => {
                if !*moved && from.distance(p) < SLOP {
                    return;
                }
                *moved = true;
                let rect = Rect::new(
                    from.x.min(p.x).round() as i32,
                    from.y.min(p.y).round() as i32,
                    ((from.x - p.x).abs().round() as i32).max(1),
                    ((from.y - p.y).abs().round() as i32).max(1),
                );
                (rect, *surface)
            }
            Drag::Move { from, start } => {
                let (dx, dy) = delta(*from, p);
                let rect = Rect::new(start.x + dx, start.y + dy, start.width, start.height);
                let Some(s) = self.selection else {
                    return;
                };
                (shift_inside(rect, self.surfaces[s.surface].rect), s.surface)
            }
            Drag::Resize { from, start, edges } => {
                let (dx, dy) = delta(*from, p);
                let Some(s) = self.selection else {
                    return;
                };
                (resized(*start, *edges, dx, dy), s.surface)
            }
        };
        let bounds = self.surfaces[surface].rect;
        if let Some(rect) = rect.intersection(&bounds) {
            self.selection = Some(Selection { rect, surface });
        }
    }

    fn release(&mut self) {
        if let Some(Drag::New {
            from,
            surface,
            moved: false,
        }) = self.drag.take()
        {
            let bounds = self.surfaces[surface].rect;
            let rect = self
                .window_at(from)
                .and_then(|w| w.intersection(&bounds))
                .unwrap_or(bounds);
            self.selection = Some(Selection { rect, surface });
        }
    }

    /// The topmost window under `p`.
    fn window_at(&self, p: Point) -> Option<Rect> {
        self.windows.iter().copied().find(|w| contains(*w, p))
    }

    /// What a click would pick: shown while nothing's dragged and the
    /// pointer isn't on the selection.
    fn hover(&self) -> Option<Rect> {
        if self.drag.is_some() {
            return None;
        }
        let p = self.cursor?;
        if let Some(s) = self.selection
            && (contains(s.rect, p) || grip(s.rect, p) != Edges::default())
        {
            return None;
        }
        let bounds = self.surfaces[self.surface_at(p)?].rect;
        self.window_at(p)
            .and_then(|w| w.intersection(&bounds))
            .or(Some(bounds))
    }

    fn toolbar(&self) -> usize {
        self.selection.map_or(self.focused, |s| s.surface)
    }

    fn look(&self) -> Look {
        Look {
            selection: self.selection.map(|s| s.rect),
            hover: self.hover(),
            toolbar: self.toolbar(),
        }
    }

    /// The surfaces showing something else from `before` to `after`.
    fn changed(&self, before: &Look, after: &Look) -> Vec<Id> {
        if before == after {
            return Vec::new();
        }
        let rects: Vec<Rect> = [before.selection, after.selection, before.hover, after.hover]
            .into_iter()
            .flatten()
            .collect();
        self.surfaces
            .iter()
            .enumerate()
            .filter(|(i, s)| {
                *i == before.toolbar
                    || *i == after.toolbar
                    || rects.iter().any(|r| r.intersection(&s.rect).is_some())
            })
            .map(|(_, s)| s.window)
            .collect()
    }

    /// For `debug screenshot`.
    pub fn describe(&self) -> String {
        match self.selection {
            Some(s) => format!("picker={} selection={}", self.surfaces[s.surface].name, s.rect),
            None => "picker=open selection=none".to_owned(),
        }
    }

    pub fn view<'a>(
        &'a self,
        window: Id,
        theme: &'a Theme,
        locale: &'a Locale,
    ) -> Option<Element<'a, Message>> {
        let i = self.surface_of(window)?;
        let surface = &self.surfaces[i];
        let root = Node::root("screenshot").attr("output", surface.name.clone());
        let local = |r: Rect| {
            Rectangle::new(
                Point::new((r.x - surface.rect.x) as f32, (r.y - surface.rect.y) as f32),
                Size::new(r.width as f32, r.height as f32),
            )
        };
        let on_this = |r: &Rect| r.intersection(&surface.rect).is_some();
        let overlay = Overlay {
            window,
            selection: self.selection.map(|s| s.rect).filter(on_this).map(local),
            hover: self.hover().filter(on_this).map(local),
            dragging: matches!(self.drag, Some(Drag::Move { .. })),
            look: Paint::of(theme, &root),
        };
        let mut layers: Vec<Element<'a, Message>> = vec![
            image(surface.picture.clone())
                .width(Length::Fill)
                .height(Length::Fill)
                .content_fit(ContentFit::Fill)
                .into(),
            canvas::Canvas::new(overlay)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ];
        if self.toolbar() == i {
            layers.push(
                container(self.toolbar_view(theme, locale, &root))
                    .padding(theme.resolve(&root).padding)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(Alignment::Center)
                    .align_y(Alignment::End)
                    .into(),
            );
        }
        Some(theme.tag(&root, stack(layers)).into())
    }

    fn toolbar_view<'a>(
        &'a self,
        theme: &'a Theme,
        locale: &'a Locale,
        root: &Node,
    ) -> Element<'a, Message> {
        let bar = root.child("toolbar");
        let selected = self.selection.is_some();
        let label = match self.selection {
            Some(s) => format!("{} × {}", s.rect.width, s.rect.height),
            None => locale.tr("screenshot.hint").to_owned(),
        };
        let button = |class: &'static str, key: &'static str, message: Option<Message>| {
            let node = bar.child("button").class(class);
            let text = theme.text(&node.child("text"), locale.tr(key));
            Element::from(theme.button(&node, text).on_press_maybe(message))
        };
        let take = |d: Destination| selected.then_some(Message::Take(d));
        let mut children = vec![
            Element::from(theme.text(&bar.child("text"), label)),
            Element::from(Space::new().width(Length::Fixed(theme.resolve(&bar).gap))),
            button("all", "screenshot.all", Some(Message::All)),
            button("cancel", "screenshot.cancel", Some(Message::Cancel)),
        ];
        if self.can_copy {
            children.push(button(
                "copy",
                "screenshot.copy",
                take(Destination::Clipboard),
            ));
        }
        if self.can_edit {
            children.push(button(
                "edit",
                "screenshot.edit",
                take(Destination::File { edit: true }),
            ));
        }
        children.push(button(
            "save",
            "screenshot.save",
            take(Destination::File { edit: false }),
        ));
        theme
            .container(&bar, theme.row(&bar, children).align_y(Alignment::Center))
            .into()
    }
}

fn contains(r: Rect, p: Point) -> bool {
    p.x >= r.x as f32
        && p.y >= r.y as f32
        && p.x < (r.x + r.width) as f32
        && p.y < (r.y + r.height) as f32
}

fn delta(from: Point, to: Point) -> (i32, i32) {
    (
        (to.x - from.x).round() as i32,
        (to.y - from.y).round() as i32,
    )
}

/// The edges of `r` within reach of `p`: one, or two at a corner.
fn grip(r: Rect, p: Point) -> Edges {
    let (left, top) = (r.x as f32, r.y as f32);
    let (right, bottom) = ((r.x + r.width) as f32, (r.y + r.height) as f32);
    let along_x = p.x > left - GRIP && p.x < right + GRIP;
    let along_y = p.y > top - GRIP && p.y < bottom + GRIP;
    Edges {
        left: along_y && (p.x - left).abs() <= GRIP,
        right: along_y && (p.x - right).abs() <= GRIP,
        top: along_x && (p.y - top).abs() <= GRIP,
        bottom: along_x && (p.y - bottom).abs() <= GRIP,
    }
}

/// `start` with the grabbed edges moved by `dx`, `dy`; an edge dragged
/// past the opposite one turns it inside out, never below 1×1.
fn resized(start: Rect, edges: Edges, dx: i32, dy: i32) -> Rect {
    let (mut left, mut top) = (start.x, start.y);
    let (mut right, mut bottom) = (start.x + start.width, start.y + start.height);
    if edges.left {
        left += dx;
    }
    if edges.right {
        right += dx;
    }
    if edges.top {
        top += dy;
    }
    if edges.bottom {
        bottom += dy;
    }
    let (x0, x1) = (left.min(right), left.max(right));
    let (y0, y1) = (top.min(bottom), top.max(bottom));
    Rect::new(x0, y0, (x1 - x0).max(1), (y1 - y0).max(1))
}

/// `r` moved back inside `bounds` (as far as it fits).
fn shift_inside(r: Rect, bounds: Rect) -> Rect {
    let x = r.x.min(bounds.x + bounds.width - r.width).max(bounds.x);
    let y = r.y.min(bounds.y + bounds.height - r.height).max(bounds.y);
    Rect::new(x, y, r.width, r.height)
}

/// The theme's colours and sizes for the overlay's drawing.
struct Paint {
    shade: Color,
    hover: (Option<Color>, Color, f32),
    selection: (Color, f32),
    handle: (Color, Color, f32, f32),
}

impl Paint {
    fn of(theme: &Theme, root: &Node) -> Self {
        let shade = theme.resolve(&root.child("shade"));
        let hover = theme.resolve(&root.child("hover"));
        let selection = root.child("selection");
        let outline = theme.resolve(&selection);
        let handle = theme.resolve(&selection.child("handle"));
        let px = |l: Option<theme::Length>, default: f32| match l {
            Some(theme::Length::Px(v)) => v,
            _ => default,
        };
        Self {
            shade: shade.background.unwrap_or(Color::TRANSPARENT),
            hover: (
                hover.background,
                hover.border_color.unwrap_or(Color::TRANSPARENT),
                hover.border_width,
            ),
            selection: (
                outline.border_color.unwrap_or(Color::WHITE),
                outline.border_width,
            ),
            handle: (
                handle.background.unwrap_or(Color::WHITE),
                handle.border_color.unwrap_or(Color::TRANSPARENT),
                handle.border_width,
                px(handle.height, 8.0),
            ),
        }
    }
}

/// What's drawn over a surface's picture, surface-local; the presses
/// on it.
struct Overlay {
    window: Id,
    selection: Option<Rectangle>,
    hover: Option<Rectangle>,
    dragging: bool,
    look: Paint,
}

impl canvas::Program<Message> for Overlay {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let p = cursor.position_in(bounds)?;
                Some(canvas::Action::publish(Message::Pressed(self.window, p)).and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let size = bounds.size();
        match self.selection {
            // The shade around the selection: above, below, left, right.
            Some(s) => {
                let (right, bottom) = (s.x + s.width, s.y + s.height);
                for (x, y, w, h) in [
                    (0.0, 0.0, size.width, s.y),
                    (0.0, bottom, size.width, size.height - bottom),
                    (0.0, s.y, s.x, s.height),
                    (right, s.y, size.width - right, s.height),
                ] {
                    if w > 0.0 && h > 0.0 {
                        frame.fill_rectangle(Point::new(x, y), Size::new(w, h), self.look.shade);
                    }
                }
            }
            None => frame.fill_rectangle(Point::ORIGIN, size, self.look.shade),
        }
        if let Some(h) = self.hover {
            let (fill, border, width) = self.look.hover;
            if let Some(fill) = fill {
                frame.fill_rectangle(h.position(), h.size(), fill);
            }
            if width > 0.0 {
                frame.stroke(
                    &inset(h, width),
                    Stroke::default().with_color(border).with_width(width),
                );
            }
        }
        if let Some(s) = self.selection {
            let (color, width) = self.look.selection;
            if width > 0.0 {
                frame.stroke(
                    &inset(s, width),
                    Stroke::default().with_color(color).with_width(width),
                );
            }
            let (fill, border, border_width, side) = self.look.handle;
            for c in handles(s) {
                let square = Path::rectangle(
                    Point::new(c.x - side / 2.0, c.y - side / 2.0),
                    Size::new(side, side),
                );
                frame.fill(&square, fill);
                if border_width > 0.0 {
                    frame.stroke(
                        &square,
                        Stroke::default()
                            .with_color(border)
                            .with_width(border_width),
                    );
                }
            }
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &(),
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        let Some(p) = cursor.position_in(bounds) else {
            return mouse::Interaction::default();
        };
        let Some(s) = self.selection else {
            return mouse::Interaction::Crosshair;
        };
        let r = Rect::new(
            s.x.round() as i32,
            s.y.round() as i32,
            s.width.round() as i32,
            s.height.round() as i32,
        );
        let e = grip(r, p);
        match (e.left || e.right, e.top || e.bottom) {
            (true, true) if e.left == e.top => mouse::Interaction::ResizingDiagonallyDown,
            (true, true) => mouse::Interaction::ResizingDiagonallyUp,
            (true, false) => mouse::Interaction::ResizingHorizontally,
            (false, true) => mouse::Interaction::ResizingVertically,
            _ if self.dragging => mouse::Interaction::Grabbing,
            _ if contains(r, p) => mouse::Interaction::Grab,
            _ => mouse::Interaction::Crosshair,
        }
    }
}

/// A stroke of `width` inside `r`'s edges.
fn inset(r: Rectangle, width: f32) -> Path {
    let half = width / 2.0;
    Path::rectangle(
        Point::new(r.x + half, r.y + half),
        Size::new((r.width - width).max(0.0), (r.height - width).max(0.0)),
    )
}

/// The corners and the middles of the edges.
fn handles(r: Rectangle) -> [Point; 8] {
    let (x0, y0) = (r.x, r.y);
    let (x1, y1) = (r.x + r.width, r.y + r.height);
    let (xm, ym) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    [
        Point::new(x0, y0),
        Point::new(xm, y0),
        Point::new(x1, y0),
        Point::new(x1, ym),
        Point::new(x1, y1),
        Point::new(xm, y1),
        Point::new(x0, y1),
        Point::new(x0, ym),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grip_finds_edges_and_corners() {
        let r = Rect::new(100, 100, 200, 100);
        assert_eq!(grip(r, Point::new(200.0, 150.0)), Edges::default());
        assert_eq!(
            grip(r, Point::new(103.0, 150.0)),
            Edges {
                left: true,
                ..Edges::default()
            }
        );
        assert_eq!(
            grip(r, Point::new(298.0, 205.0)),
            Edges {
                right: true,
                bottom: true,
                ..Edges::default()
            }
        );
        assert_eq!(grip(r, Point::new(103.0, 300.0)), Edges::default());
    }

    #[test]
    fn resizing_past_the_other_edge_turns_inside_out() {
        let r = Rect::new(100, 100, 200, 100);
        let right = Edges {
            right: true,
            ..Edges::default()
        };
        assert_eq!(resized(r, right, 50, 0), Rect::new(100, 100, 250, 100));
        assert_eq!(resized(r, right, -250, 0), Rect::new(50, 100, 50, 100));
        assert_eq!(resized(r, right, -200, 0), Rect::new(100, 100, 1, 100));
    }

    #[test]
    fn a_move_stays_on_the_output() {
        let bounds = Rect::new(0, 0, 1920, 1080);
        assert_eq!(
            shift_inside(Rect::new(1800, -20, 200, 100), bounds),
            Rect::new(1720, 0, 200, 100)
        );
    }
}
