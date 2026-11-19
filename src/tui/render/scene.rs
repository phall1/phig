//! The terminal scene: declare one keyboard-led surface, paint in order, then
//! route the next input burst against the live reducer state.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Position, Rect},
    style::Style,
    widgets::Widget,
};
use ratcn::runtime::{
    Component, DeclareCtx, Event, EventCtx, EventResult, KeyCode, MouseKind, PaintCtx, Ratcn,
    ScopeOptions, ScrollDirection,
};

use crate::app::{Action, App, Overlay};

use super::{
    RenderContext, chrome, graph::GraphCache, layout::Screen, render_body, render_overlay,
};
use crate::tui::input::resolve_ratcn_action;

#[derive(Debug, PartialEq, Eq)]
pub(in crate::tui) enum Input {
    Action(Action),
    Paste(String),
    Interrupt,
}

pub(in crate::tui) struct Renderer {
    runtime: Ratcn<App, Input>,
    shared: Rc<SceneState>,
}

struct SceneState {
    context: RenderContext,
    theme: ratcn::Theme,
    history: RefCell<GraphCache>,
    cursor: Cell<Option<Position>>,
}

impl Renderer {
    pub(in crate::tui) fn new(context: RenderContext) -> Self {
        Self {
            runtime: Ratcn::new(),
            shared: Rc::new(SceneState {
                theme: context.ratcn_theme(),
                context,
                history: RefCell::default(),
                cursor: Cell::default(),
            }),
        }
    }

    pub(in crate::tui) fn draw(&mut self, frame: &mut Frame<'_>, app: &App) {
        let area = frame.area();
        self.shared.cursor.set(None);
        self.runtime
            .render(frame, area, app, &self.shared.theme, |ctx| {
                ctx.component("phig", Surface(Rc::clone(&self.shared)), area);
            });
        if let Some(cursor) = self.shared.cursor.get() {
            frame.set_cursor_position(cursor);
        }
    }

    pub(in crate::tui) fn input(
        &mut self,
        event: crossterm::event::Event,
        app: &App,
    ) -> Option<Input> {
        match self.runtime.handle_event(event, app) {
            EventResult::Emit(input) => Some(input),
            EventResult::Consumed | EventResult::Ignored => None,
        }
    }

    pub(in crate::tui) fn clear_history(&self) {
        self.shared.history.borrow_mut().clear();
    }
}

struct Surface(Rc<SceneState>);

impl Component<App, Input> for Surface {
    fn scope_options(&self, _: &App) -> ScopeOptions {
        ScopeOptions::default().focusable(true)
    }

    fn declare(&mut self, ctx: &mut DeclareCtx<'_, App, Input>) {
        let area = ctx.area();
        let screen = Screen::new(area);
        let sections = [
            ("header", Section::Header, screen.header),
            ("body", Section::Body, screen.body),
            ("footer", Section::Footer, screen.footer),
            // This is painted last, but input still bubbles to the surface.
            ("overlay", Section::Overlay(screen.body), area),
        ];
        for (id, section, area) in sections {
            if !area.is_empty() {
                ctx.component(id, PaintSection(section, Rc::clone(&self.0)), area);
            }
        }
    }

    fn handle_event(
        &mut self,
        event: &Event,
        app: &App,
        _: &mut EventCtx<'_>,
    ) -> EventResult<Input> {
        let message = match event {
            Event::Key(key) if key.modifiers.ctrl && key.code == KeyCode::Char('c') => {
                Input::Interrupt
            }
            Event::Key(key) => {
                let Some(action) = resolve_ratcn_action(app, self.0.context.bindings(), *key)
                else {
                    return EventResult::Ignored;
                };
                Input::Action(action)
            }
            Event::Paste(value)
                if matches!(
                    app.overlay,
                    Overlay::Search { .. } | Overlay::Palette { .. } | Overlay::FilePicker { .. }
                ) =>
            {
                Input::Paste(value.clone())
            }
            Event::Mouse(mouse) => match mouse.kind {
                MouseKind::Scroll(ScrollDirection::Down) => Input::Action(Action::Move(3)),
                MouseKind::Scroll(ScrollDirection::Up) => Input::Action(Action::Move(-3)),
                _ => return EventResult::Ignored,
            },
            _ => return EventResult::Ignored,
        };
        EventResult::Emit(message)
    }
}

enum Section {
    Header,
    Body,
    Footer,
    Overlay(Rect),
}

struct PaintSection(Section, Rc<SceneState>);

impl Component<App, Input> for PaintSection {
    fn declare(&mut self, _: &mut DeclareCtx<'_, App, Input>) {}

    fn interaction_area(&self, _: Rect, _: &App) -> Rect {
        // This is paint, not a second set of cursor targets. The reducer and
        // semantic keymap own focus and modal shortcuts throughout a burst.
        Rect::ZERO
    }

    fn paint(&mut self, ctx: &mut PaintCtx<'_, App>) {
        let app = ctx.state();
        let area = ctx.area();
        let context = &self.1.context;
        ctx.with_buffer(area, |_, buffer| {
            let mut canvas = Canvas::new(buffer, &self.1.cursor);
            match self.0 {
                Section::Header => chrome::render_header(&mut canvas, app, area, context),
                Section::Body => render_body(
                    &mut canvas,
                    app,
                    area,
                    context,
                    &mut self.1.history.borrow_mut(),
                ),
                Section::Footer => chrome::render_footer(&mut canvas, app, area, context),
                Section::Overlay(body) => render_overlay(&mut canvas, app, area, body, context),
            }
        });
    }
}

/// Paint directly into ratcn's borrowed Ratatui buffer. A hardware caret is
/// reported to the host separately; it is not part of the cell buffer.
pub(super) struct Canvas<'a> {
    buffer: &'a mut Buffer,
    cursor: &'a Cell<Option<Position>>,
}

impl<'a> Canvas<'a> {
    pub(super) fn new(buffer: &'a mut Buffer, cursor: &'a Cell<Option<Position>>) -> Self {
        Self { buffer, cursor }
    }

    pub(super) fn render_widget(&mut self, widget: impl Widget, area: Rect) {
        if !area.is_empty() {
            widget.render(area, self.buffer);
        }
    }

    pub(super) fn set_cursor_position(&self, position: (u16, u16)) {
        let position = position.into();
        if self.buffer.area.contains(position) {
            self.cursor.set(Some(position));
        }
    }

    pub(super) fn set_style(&mut self, area: Rect, style: Style) {
        self.buffer.set_style(area, style);
    }

    /// Erase just the overlay, plus any underlying wide glyph crossing an edge.
    /// A surviving lead cell would otherwise skip the border when emitted.
    pub(super) fn clear_overlay(&mut self, area: Rect) {
        let area = area.intersection(self.buffer.area);
        if area.is_empty() {
            return;
        }
        for y in area.top()..area.bottom() {
            if area.left() > self.buffer.area.left() {
                self.clear_crossing_glyph(area.left() - 1, y);
            }
            if area.right() < self.buffer.area.right() {
                self.clear_crossing_glyph(area.right() - 1, y);
            }
        }
        self.render_widget(ratatui::widgets::Clear, area);
    }

    fn clear_crossing_glyph(&mut self, x: u16, y: u16) {
        let cell = &self.buffer[(x, y)];
        let width = super::format::display_width(cell.symbol());
        if width > 1 {
            let end = usize::from(x)
                .saturating_add(width)
                .min(usize::from(self.buffer.area.right()));
            for column in x..end as u16 {
                self.buffer[(column, y)].reset();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{
        Terminal,
        backend::TestBackend,
        widgets::{Block, Borders},
    };

    use super::*;

    #[test]
    fn overlay_border_replaces_both_halves_of_wide_content() {
        let mut terminal = Terminal::new(TestBackend::new(12, 3)).unwrap();
        terminal
            .draw(|frame| {
                let buffer = frame.buffer_mut();
                buffer.set_string(0, 1, "text界 xxx界", Style::default());
                let cursor = Cell::new(None);
                let mut canvas = Canvas::new(buffer, &cursor);
                let popup = Rect::new(5, 0, 6, 3);
                canvas.clear_overlay(popup);
                canvas.render_widget(Block::default().borders(Borders::ALL), popup);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(3, 1)].symbol(), "t");
        assert_eq!(buffer[(4, 1)].symbol(), " ");
        assert_eq!(buffer[(5, 1)].symbol(), "│");
        assert_eq!(buffer[(10, 1)].symbol(), "│");
        assert_eq!(buffer[(11, 1)].symbol(), " ");
    }
}
