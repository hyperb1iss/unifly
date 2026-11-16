//! Topology screen: the uplink tree on a canvas, with a camera that fits,
//! pans, and zooms, and a cursor that opens the device it rests on.

mod input;
mod layout;
mod render;
mod state;

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use color_eyre::eyre::Result;
use crossterm::event::{KeyEvent, MouseEvent};
use ratatui::Frame;
use ratatui::layout::Rect;

use unifly_api::{Device, EntityId};

use crate::tui::action::Action;
use crate::tui::component::Component;
use crate::tui::screens::link::Link;
use crate::tui::widgets::click::{Click, Hint, HitMap};

/// Where the camera is.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Camera {
    /// On the whole tree, at the largest zoom that shows all of it. It
    /// follows the terminal size and the device list.
    Fit,
    /// Centered on a layout point, at a zoom the user chose, over the tree
    /// as it was wrapped when the camera left the fitted view.
    Free {
        cx: f64,
        cy: f64,
        zoom: f64,
        wrap: layout::Wrap,
    },
}

/// A left press that has not been released yet.
#[derive(Debug, Clone, Copy)]
struct Press {
    /// The cell the pointer last reached.
    at: (u16, u16),
    /// The device box the press landed on, by device index.
    node: Option<usize>,
    /// Whether the pointer moved since, making it a drag and not a tap.
    dragged: bool,
}

pub struct TopologyScreen {
    focused: bool,
    devices: Arc<Vec<Arc<Device>>>,
    camera: Camera,
    /// The device under the cursor.
    selected: Option<EntityId>,
    link: Link,
    /// The canvas the last frame drew on, so input works in its cells.
    canvas: Cell<Rect>,
    /// What the last frame drew where, for clicks.
    hits: RefCell<HitMap<Click>>,
    /// The left press under way, when there is one.
    press: Option<Press>,
}

impl Default for TopologyScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for TopologyScreen {
    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        Ok(self.handle_key_input(key))
    }

    fn handle_mouse_event(&mut self, mouse: MouseEvent) -> Result<Option<Action>> {
        Ok(self.handle_mouse_input(mouse))
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        self.apply_action(action);
        Ok(None)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        self.render_screen(frame, area);
    }

    fn hints(&self) -> Vec<(u8, Hint)> {
        self.status_hints()
    }

    fn help(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("hjkl \u{2190}\u{2193}\u{2191}\u{2192}", "Pan the map"),
            ("PgUp/PgDn", "Pan a screen up / down"),
            ("g/G", "Top / bottom of the tree"),
            ("+/-", "Zoom in / out"),
            ("f", "Fit the whole tree"),
            ("r", "Reset to 100%"),
            ("[ / ]", "Previous / next device"),
            ("Enter", "Open the selected device"),
            ("Esc", "Clear the selection"),
        ]
    }

    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn id(&self) -> &'static str {
        "Topo"
    }
}
