//! A surface without a compositor, for tests: the real layout, events
//! and drawing, through iced's `UserInterface` and the tiny-skia
//! renderer, at any scale. Frames come at the times a test gives, so a
//! transition is sampled where it should be.

use std::path::Path;
use std::time::Instant;

use iced::advanced::clipboard;
use iced::advanced::renderer::{self, Headless as _};
use iced::advanced::widget::operation::{self, Operation};
use iced::keyboard::{self, key};
use iced::{Color, Event, Point, Rectangle, Size, mouse, window};
use iced_runtime::user_interface::{self, UserInterface};

use crate::measure::headless;
use crate::query::{Found, Report};
use crate::{Element, Renderer, Sheet, Theme};

pub struct Harness<'a, Message> {
    ui: Option<UserInterface<'a, Message, Theme, Renderer>>,
    renderer: Renderer,
    size: Size,
    scale: f32,
    cursor: mouse::Cursor,
    /// What the widgets published, oldest first.
    pub messages: Vec<Message>,
}

/// A frame's pixels: RGBA, row by row.
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Pixels {
    /// The pixel at `x`, `y`, in device pixels.
    pub fn at(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ]
    }

    /// The pixel at the logical point `x`, `y`, at `scale`.
    pub fn at_point(&self, x: f32, y: f32, scale: f32) -> [u8; 4] {
        self.at((x * scale) as u32, (y * scale) as u32)
    }

    pub fn save_png(&self, path: impl AsRef<Path>) -> ::image::ImageResult<()> {
        ::image::save_buffer(
            path,
            &self.rgba,
            self.width,
            self.height,
            ::image::ColorType::Rgba8,
        )
    }
}

impl<'a, Message> Harness<'a, Message> {
    /// `element` laid out in `size` logical pixels, drawn at `scale`
    /// (built with the root node of the same scale).
    pub fn new(element: impl Into<Element<'a, Message>>, size: Size, scale: f32) -> Self {
        let mut renderer = headless();
        let ui = UserInterface::build(
            element,
            size,
            user_interface::Cache::default(),
            &mut renderer,
        );
        Self {
            ui: Some(ui),
            renderer,
            size,
            scale,
            cursor: mouse::Cursor::Unavailable,
            messages: Vec::new(),
        }
    }

    /// A new view, the widget tree (its states, its transitions) kept.
    pub fn rebuild(&mut self, element: impl Into<Element<'a, Message>>) {
        let cache = self.ui.take().expect("built").into_cache();
        self.ui = Some(UserInterface::build(
            element,
            self.size,
            cache,
            &mut self.renderer,
        ));
    }

    /// `events`, as the runtime sends them: what the widgets asked for.
    pub fn send(&mut self, events: &[Event]) -> window::RedrawRequest {
        let ui = self.ui.as_mut().expect("built");
        let (state, _) = ui.update(
            events,
            self.cursor,
            &mut self.renderer,
            &mut clipboard::Null,
            &mut self.messages,
        );
        match state {
            user_interface::State::Updated { redraw_request, .. } => redraw_request,
            user_interface::State::Outdated => window::RedrawRequest::NextFrame,
        }
    }

    pub fn move_to(&mut self, point: Point) -> window::RedrawRequest {
        self.cursor = mouse::Cursor::Available(point);
        self.send(&[Event::Mouse(mouse::Event::CursorMoved { position: point })])
    }

    pub fn press(&mut self) -> window::RedrawRequest {
        self.send(&[Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Left,
        ))])
    }

    pub fn release(&mut self) -> window::RedrawRequest {
        self.send(&[Event::Mouse(mouse::Event::ButtonReleased(
            mouse::Button::Left,
        ))])
    }

    /// The pointer to `point`, pressed and released there.
    pub fn click(&mut self, point: Point) -> window::RedrawRequest {
        self.move_to(point);
        self.press();
        self.release()
    }

    /// The border boxes of the widgets `selector` matches, in tree
    /// order.
    pub fn find(&mut self, sheet: &Sheet, selector: &str) -> Vec<Rectangle> {
        sheet.select(selector, &self.report()).expect("a selector")
    }

    /// Every widget, as the query reports them: for
    /// [`Sheet::matching`] and [`Found::path`].
    pub fn report(&mut self) -> Vec<Found> {
        self.operate(&mut Report::default()).unwrap_or_default()
    }

    /// An iced operation run on the tree, as a `Task` runs one: its
    /// outcome, when it has one.
    pub fn operate<T>(&mut self, operation: &mut dyn Operation<T>) -> Option<T> {
        let ui = self.ui.as_mut().expect("built");
        ui.operate(&self.renderer, &mut operation::black_box(operation));
        match operation.finish() {
            operation::Outcome::Some(outcome) => Some(outcome),
            _ => None,
        }
    }

    /// A click in the middle of the first widget `selector` matches.
    pub fn click_on(&mut self, sheet: &Sheet, selector: &str) -> window::RedrawRequest {
        let found = self.find(sheet, selector);
        let bounds = found
            .first()
            .unwrap_or_else(|| panic!("nothing matches `{selector}`"));
        self.click(bounds.center())
    }

    /// A named key, pressed and released.
    pub fn key(&mut self, named: key::Named) -> window::RedrawRequest {
        self.key_with(named, keyboard::Modifiers::default())
    }

    /// A named key pressed and released with `modifiers` held (Shift+Tab).
    pub fn key_with(
        &mut self,
        named: key::Named,
        modifiers: keyboard::Modifiers,
    ) -> window::RedrawRequest {
        let key = keyboard::Key::Named(named);
        let physical = key::Physical::Unidentified(key::NativeCode::Unidentified);
        self.send(&[Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key.clone(),
            physical_key: physical,
            location: keyboard::Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        })]);
        self.send(&[Event::Keyboard(keyboard::Event::KeyReleased {
            key: key.clone(),
            modified_key: key,
            physical_key: physical,
            location: keyboard::Location::Standard,
            modifiers,
        })])
    }

    /// `text` typed, a key per character.
    pub fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            let c = c.to_string();
            let key = keyboard::Key::Character(c.as_str().into());
            let physical = key::Physical::Unidentified(key::NativeCode::Unidentified);
            self.send(&[Event::Keyboard(keyboard::Event::KeyPressed {
                key: key.clone(),
                modified_key: key,
                physical_key: physical,
                location: keyboard::Location::Standard,
                modifiers: keyboard::Modifiers::default(),
                text: Some(c.into()),
                repeat: false,
            })]);
        }
    }

    /// A frame at `now`, as a surface gets one: `RedrawRequested`, then
    /// the drawing. What the widgets asked for in it.
    pub fn frame(&mut self, now: Instant) -> window::RedrawRequest {
        let asked = self.send(&[Event::Window(window::Event::RedrawRequested(now))]);
        let ui = self.ui.as_mut().expect("built");
        ui.draw(
            &mut self.renderer,
            &Theme::default(),
            &renderer::Style {
                text_color: Color::BLACK,
            },
            self.cursor,
        );
        asked
    }

    /// The last frame drawn, at the harness's scale, over `background`.
    pub fn pixels(&mut self, background: Color) -> Pixels {
        let width = (self.size.width * self.scale).round() as u32;
        let height = (self.size.height * self.scale).round() as u32;
        let rgba = self
            .renderer
            .screenshot(Size::new(width, height), self.scale, background);
        Pixels {
            width,
            height,
            rgba,
        }
    }
}
