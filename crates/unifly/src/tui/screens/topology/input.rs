use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use super::state::ZOOM_RANGE;
use super::{Camera, Press, TopologyScreen};
use crate::tui::action::Action;
use crate::tui::widgets::click::Click;

/// How much one zoom step scales the map.
const ZOOM_STEP: f64 = 1.2;

impl TopologyScreen {
    pub(super) fn handle_key_input(&mut self, key: KeyEvent) -> Option<Action> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let canvas = self.canvas.get();
        // A step is a slice of the screen, so it covers the same share of
        // the view at any zoom.
        let across = f64::from((canvas.width / 8).max(1));
        let down = f64::from((canvas.height / 6).max(1));
        let page = f64::from(canvas.height.saturating_sub(2).max(1));
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => self.pan(-across, 0.0),
            KeyCode::Right | KeyCode::Char('l') => self.pan(across, 0.0),
            KeyCode::Up | KeyCode::Char('k') => self.pan(0.0, -down),
            KeyCode::Down | KeyCode::Char('j') => self.pan(0.0, down),
            KeyCode::Char('d') if ctrl => self.pan(0.0, page),
            KeyCode::Char('u') if ctrl => self.pan(0.0, -page),
            KeyCode::Char('g') => self.pan(0.0, f64::MIN),
            KeyCode::Char('G') => self.pan(0.0, f64::MAX),
            KeyCode::Char('+' | '=') => self.zoom_by(ZOOM_STEP, None),
            KeyCode::Char('-') => self.zoom_by(1.0 / ZOOM_STEP, None),
            KeyCode::Char('f') => self.camera = Camera::Fit,
            KeyCode::Char('r') => self.reset(),
            KeyCode::Char(']') => self.step_selection(1),
            KeyCode::Char('[') => self.step_selection(-1),
            KeyCode::Enter => return Some(self.open_selected()),
            KeyCode::Esc if self.selected.is_some() => self.selected = None,
            _ => return None,
        }
        Some(Action::Invalidate)
    }

    /// Move the camera `columns` cells across and `rows` cells down.
    fn pan(&mut self, columns: f64, rows: f64) {
        let scene = self.last_scene();
        self.aim(
            &scene,
            scene.cx + columns / scene.zoom,
            scene.cy + rows / scene.zoom,
            scene.zoom,
        );
    }

    /// Scale the map by `factor`, holding the layout point under `pivot`
    /// (a canvas cell) where it is. Without a pivot the center holds.
    /// A step that crosses 100% stops there, so repeated steps from either
    /// end of the range return to it.
    fn zoom_by(&mut self, factor: f64, pivot: Option<(f64, f64)>) {
        let scene = self.last_scene();
        let old = scene.zoom;
        let mut zoom = (old * factor).clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
        if (old < 1.0 && zoom > 1.0) || (old > 1.0 && zoom < 1.0) {
            zoom = 1.0;
        }
        let (columns, rows) = (
            f64::from(scene.canvas.width.max(2)) - 1.0,
            f64::from(scene.canvas.height.max(2)) - 1.0,
        );
        let (column, row) = pivot.unwrap_or((columns / 2.0, rows / 2.0));
        let (left, top) = scene.origin();
        // The point under the pivot, then the center that keeps it there.
        let (x, y) = (left + column / old, top + row / old);
        let cx = x - column / zoom + columns / zoom / 2.0;
        let cy = y - row / zoom + rows / zoom / 2.0;
        self.aim(&scene, cx, cy, zoom);
    }

    /// Move the cursor `step` devices along the tree in reading order,
    /// wrapping, and bring that device on screen.
    fn step_selection(&mut self, step: isize) {
        let scene = self.last_scene();
        let order = scene.reading_order();
        if order.is_empty() {
            return;
        }
        let at = self
            .selected_index()
            .and_then(|index| order.iter().position(|each| *each == index));
        let count = isize::try_from(order.len()).unwrap_or(1);
        let next = match at {
            Some(at) => (isize::try_from(at).unwrap_or(0) + step).rem_euclid(count),
            None if step < 0 => count - 1,
            None => 0,
        };
        let index = order[usize::try_from(next).unwrap_or(0)];
        self.selected = self.devices.get(index).map(|device| device.id.clone());
        if !scene.shows(index)
            && let Some(node) = scene.tree.nodes.get(index)
        {
            let (cx, cy) = (node.center_x(), node.y + node.height / 2.0);
            self.aim(&scene, cx, cy, scene.zoom);
        }
    }

    /// Open the selected device, or put the cursor on the first one.
    fn open_selected(&mut self) -> Action {
        if let Some(id) = &self.selected {
            return Action::ShowDevice(id.clone());
        }
        self.step_selection(1);
        Action::Invalidate
    }

    /// A tap selects the device under it, and a tap on the selected device
    /// opens it, like `Enter`.
    fn tap(&mut self, index: usize) -> Option<Action> {
        let id = self.devices.get(index)?.id.clone();
        if self.selected.as_ref() == Some(&id) {
            return Some(Action::ShowDevice(id));
        }
        self.selected = Some(id);
        Some(Action::Invalidate)
    }

    /// Mouse and touch input: key hints press their key, a tap picks a
    /// device, a drag pans the map under the pointer, and the wheel zooms
    /// on the pointer.
    pub(super) fn handle_mouse_input(&mut self, mouse: MouseEvent) -> Option<Action> {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let node = match self.hits.borrow().clicked(mouse) {
                    Some(Click::Key(key)) => return Some(Action::PressKey(key)),
                    Some(Click::Row(index)) => Some(index),
                    _ => None,
                };
                self.press = Some(Press {
                    at: (mouse.column, mouse.row),
                    node,
                    dragged: false,
                });
                None
            }
            // Only a press that began on the map pans it; one that began
            // on a hint button never armed a drag.
            MouseEventKind::Drag(MouseButton::Left) => {
                let press = self.press.as_mut()?;
                let (column, row) = press.at;
                if (mouse.column, mouse.row) == (column, row) {
                    return None;
                }
                press.at = (mouse.column, mouse.row);
                press.dragged = true;
                // The map follows the pointer, so the camera moves the
                // other way.
                self.pan(
                    f64::from(column) - f64::from(mouse.column),
                    f64::from(row) - f64::from(mouse.row),
                );
                Some(Action::Invalidate)
            }
            MouseEventKind::Up(MouseButton::Left) => {
                let press = self.press.take()?;
                if press.dragged {
                    return None;
                }
                self.tap(press.node?)
            }
            MouseEventKind::ScrollUp => {
                self.zoom_by(ZOOM_STEP, self.pivot(mouse));
                Some(Action::Invalidate)
            }
            MouseEventKind::ScrollDown => {
                self.zoom_by(1.0 / ZOOM_STEP, self.pivot(mouse));
                Some(Action::Invalidate)
            }
            _ => None,
        }
    }

    /// The canvas cell under the pointer, when it is over the canvas.
    fn pivot(&self, mouse: MouseEvent) -> Option<(f64, f64)> {
        let canvas = self.canvas.get();
        canvas
            .contains(ratatui::layout::Position::new(mouse.column, mouse.row))
            .then(|| {
                (
                    f64::from(mouse.column - canvas.x),
                    f64::from(mouse.row - canvas.y),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEventKind};

    use crate::tui::action::Action;
    use crate::tui::component::Component;
    use crate::tui::screens::topology::state::tests::screen_with;
    use crate::tui::screens::topology::{Camera, TopologyScreen};
    use crate::tui::table::render_lines;
    use crate::tui::widgets::click::{self, click_label, mouse};

    fn draw(screen: &TopologyScreen, width: u16, height: u16) -> Vec<String> {
        render_lines(width, height, |frame, area| screen.render(frame, area))
    }

    fn press(screen: &mut TopologyScreen, code: KeyCode) -> Option<Action> {
        screen.handle_key_input(KeyEvent::from(code))
    }

    /// The camera's center and zoom, as the last frame resolved them.
    fn view(screen: &TopologyScreen) -> (f64, f64, f64) {
        let scene = screen.last_scene();
        (scene.cx, scene.cy, scene.zoom)
    }

    /// The canvas cell the layout point `(x, y)` is drawn at.
    fn cell_of(screen: &TopologyScreen, x: f64, y: f64) -> (f64, f64) {
        let scene = screen.last_scene();
        let (left, top) = scene.origin();
        ((x - left) * scene.zoom, (y - top) * scene.zoom)
    }

    #[test]
    fn the_map_opens_fitted_and_f_returns_to_it() {
        // Forty access points wrap into far more rows than the canvas has.
        let mut screen = screen_with(40);
        let rows = draw(&screen, 100, 30);
        assert!(rows[0].contains("fit"), "{}", rows[0]);
        let scene = screen.last_scene();
        assert!(scene.zoom < 1.0, "the fixture must not fit at 100%");
        let [x_lo, x_hi, y_lo, y_hi] = scene.tree.bounds;
        let (left, top) = scene.origin();
        let (x_span, y_span) = scene.spans();
        assert!(x_lo >= left && x_hi <= left + x_span, "fits across");
        assert!(y_lo >= top && y_hi <= top + y_span, "fits down");

        press(&mut screen, KeyCode::Char('r'));
        assert!((view(&screen).2 - 1.0).abs() < f64::EPSILON);
        assert!(draw(&screen, 100, 30)[0].contains("100%"));

        press(&mut screen, KeyCode::Char('f'));
        assert_eq!(screen.camera, Camera::Fit);
        // A fitted view follows the terminal: a larger one zooms back in.
        draw(&screen, 220, 80);
        assert!(screen.last_scene().zoom > scene.zoom);
    }

    #[test]
    fn a_tree_that_fits_opens_at_full_size() {
        let screen = screen_with(3);
        draw(&screen, 100, 30);

        assert!((view(&screen).2 - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn key_zoom_holds_the_center_and_wheel_zoom_holds_the_pointer() {
        let mut screen = screen_with(40);
        draw(&screen, 100, 30);
        press(&mut screen, KeyCode::Char('r'));
        press(&mut screen, KeyCode::Down);
        let (cx, cy, _) = view(&screen);

        press(&mut screen, KeyCode::Char('+'));
        let (zoomed_x, zoomed_y, zoom) = view(&screen);
        assert!(zoom > 1.0);
        assert!((zoomed_x - cx).abs() < 1e-9 && (zoomed_y - cy).abs() < 1e-9);

        // The wheel keeps the layout point under the pointer on its cell.
        let canvas = screen.canvas.get();
        let pointer = (canvas.x + 20, canvas.y + 5);
        let scene = screen.last_scene();
        let (left, top) = scene.origin();
        let under = (left + 20.0 / scene.zoom, top + 5.0 / scene.zoom);
        screen.handle_mouse_input(mouse(MouseEventKind::ScrollUp, pointer.0, pointer.1));
        let (column, row) = cell_of(&screen, under.0, under.1);
        assert!((column - 20.0).abs() < 1e-6, "{column}");
        assert!((row - 5.0).abs() < 1e-6, "{row}");
    }

    #[test]
    fn zoom_steps_return_to_100_percent_from_either_limit() {
        let mut screen = screen_with(3);
        draw(&screen, 100, 30);
        for _ in 0..20 {
            press(&mut screen, KeyCode::Char('+'));
        }
        assert!((view(&screen).2 - 5.0).abs() < f64::EPSILON);
        let mut seen = Vec::new();
        for _ in 0..30 {
            press(&mut screen, KeyCode::Char('-'));
            seen.push(view(&screen).2);
        }
        assert!(
            seen.iter().any(|zoom| (zoom - 1.0).abs() < f64::EPSILON),
            "{seen:?}"
        );
        assert!((view(&screen).2 - 0.2).abs() < f64::EPSILON);
        for _ in 0..12 {
            press(&mut screen, KeyCode::Char('='));
            if (view(&screen).2 - 1.0).abs() < f64::EPSILON {
                return;
            }
        }
        panic!("zooming in from the minimum skipped 100%");
    }

    #[test]
    fn a_pan_step_covers_the_same_share_of_the_screen_at_any_zoom() {
        let mut screen = screen_with(40);
        draw(&screen, 100, 30);
        let step = |screen: &mut TopologyScreen| {
            let before = view(screen);
            press(screen, KeyCode::Char('j'));
            let after = view(screen);
            (after.1 - before.1) * after.2
        };

        press(&mut screen, KeyCode::Char('r'));
        let at_100 = step(&mut screen);
        for _ in 0..6 {
            press(&mut screen, KeyCode::Char('+'));
        }
        press(&mut screen, KeyCode::Char('g'));
        let zoomed = step(&mut screen);

        assert!(at_100 > 0.0);
        assert!((at_100 - zoomed).abs() < 1e-6, "{at_100} vs {zoomed}");
    }

    #[test]
    fn vim_keys_pan_like_the_arrows() {
        for (vim, arrow) in [
            ('h', KeyCode::Left),
            ('j', KeyCode::Down),
            ('k', KeyCode::Up),
            ('l', KeyCode::Right),
        ] {
            let mut by_vim = screen_with(40);
            let mut by_arrow = screen_with(40);
            for screen in [&mut by_vim, &mut by_arrow] {
                draw(screen, 100, 30);
                press(screen, KeyCode::Char('r'));
                // Zoomed in and off the corner, so every direction has
                // room to move.
                press(screen, KeyCode::Char('+'));
                press(screen, KeyCode::Char('+'));
                press(screen, KeyCode::Down);
                press(screen, KeyCode::Right);
            }
            let start = view(&by_vim);
            press(&mut by_vim, KeyCode::Char(vim));
            press(&mut by_arrow, arrow);
            assert_eq!(view(&by_vim), view(&by_arrow), "{vim}");
            assert_ne!(view(&by_vim), start, "{vim} moved nothing");
        }
    }

    #[test]
    fn the_map_cannot_be_panned_out_of_sight() {
        let mut screen = screen_with(6);
        draw(&screen, 100, 30);
        for key in [KeyCode::Left, KeyCode::Up, KeyCode::Right, KeyCode::Down] {
            for _ in 0..200 {
                press(&mut screen, key);
            }
            // The fixture fits the canvas, so all of it must stay on it.
            let scene = screen.last_scene();
            let [x_lo, x_hi, y_lo, y_hi] = scene.tree.bounds;
            let (left, top) = scene.origin();
            let (x_span, y_span) = scene.spans();
            assert!(x_lo >= left && x_hi <= left + x_span, "{key:?}");
            assert!(y_lo >= top && y_hi <= top + y_span, "{key:?}");
            let text = draw(&screen, 100, 30).join("\n");
            assert!(
                text.chars()
                    .any(|ch| ('\u{2800}'..='\u{28FF}').contains(&ch)),
                "{key:?} left nothing drawn:\n{text}"
            );
        }
    }

    #[test]
    fn a_drag_pans_the_map_with_the_pointer() {
        let mut screen = screen_with(40);
        draw(&screen, 100, 30);
        press(&mut screen, KeyCode::Char('r'));
        press(&mut screen, KeyCode::Char('+'));
        press(&mut screen, KeyCode::Char('+'));
        press(&mut screen, KeyCode::Down);
        press(&mut screen, KeyCode::Right);
        let (cx, cy, zoom) = view(&screen);

        screen.handle_mouse_input(mouse(MouseEventKind::Down(MouseButton::Left), 30, 12));
        screen.handle_mouse_input(mouse(MouseEventKind::Drag(MouseButton::Left), 34, 14));
        // Dragging right and down moves the camera left and up by the
        // same cells, so the point under the pointer stays under it.
        let (dragged_x, dragged_y, _) = view(&screen);
        assert!(
            ((cx - dragged_x) * zoom - 4.0).abs() < 1e-9,
            "{dragged_x} from {cx}"
        );
        assert!(
            ((cy - dragged_y) * zoom - 2.0).abs() < 1e-9,
            "{dragged_y} from {cy}"
        );

        screen.handle_mouse_input(mouse(MouseEventKind::Up(MouseButton::Left), 34, 14));
        screen.handle_mouse_input(mouse(MouseEventKind::Drag(MouseButton::Left), 10, 5));
        assert_eq!(view(&screen).0, dragged_x, "a released drag stops");
    }

    #[test]
    fn a_drag_that_starts_on_a_hint_never_pans() {
        let mut screen = screen_with(40);
        let rows = draw(&screen, 120, 20);
        let before = screen.camera;
        let hint = click_label(&rows, "hjkl pan", "fit");
        let action = screen.handle_mouse_input(hint);
        assert!(
            matches!(action, Some(Action::PressKey(key)) if key == click::char_key('f')),
            "{action:?}"
        );

        for column in [hint.column + 3, hint.column + 9] {
            screen.handle_mouse_input(mouse(MouseEventKind::Drag(MouseButton::Left), column, 2));
        }

        assert_eq!(screen.camera, before);
        assert!(screen.press.is_none(), "a hint tap starts no drag");
    }

    #[test]
    fn brackets_walk_the_devices_and_enter_opens_the_one_under_the_cursor() {
        let mut screen = screen_with(3);
        draw(&screen, 100, 30);

        // With no cursor yet, Enter places it on the root.
        let placed = press(&mut screen, KeyCode::Enter);
        assert!(matches!(placed, Some(Action::Invalidate)), "{placed:?}");
        assert_eq!(screen.selected, Some(screen.devices[0].id.clone()));
        assert!(draw(&screen, 100, 30)[0].contains("gw"));

        press(&mut screen, KeyCode::Char(']'));
        assert_eq!(screen.selected, Some(screen.devices[1].id.clone()));
        press(&mut screen, KeyCode::Char('['));
        press(&mut screen, KeyCode::Char('['));
        assert_eq!(
            screen.selected,
            Some(screen.devices[4].id.clone()),
            "the walk wraps to the last device"
        );

        let opened = press(&mut screen, KeyCode::Enter);
        assert!(
            matches!(&opened, Some(Action::ShowDevice(id)) if *id == screen.devices[4].id),
            "{opened:?}"
        );

        let cleared = press(&mut screen, KeyCode::Esc);
        assert!(cleared.is_some() && screen.selected.is_none());
        assert!(
            press(&mut screen, KeyCode::Esc).is_none(),
            "Esc is the app's"
        );
    }

    #[test]
    fn the_cursor_brings_an_off_screen_device_into_view() {
        let mut screen = screen_with(40);
        draw(&screen, 100, 30);
        press(&mut screen, KeyCode::Char('r'));

        press(&mut screen, KeyCode::Char('['));

        let scene = screen.last_scene();
        let index = screen.selected_index().expect("a device is selected");
        assert_eq!(index, *scene.reading_order().last().expect("devices"));
        assert!(scene.shows(index), "the last device is on screen");
    }

    #[test]
    fn a_tap_selects_a_device_and_a_second_tap_opens_it() {
        let mut screen = screen_with(3);
        let rows = draw(&screen, 100, 30);
        let tap = click_label(&rows, "ap-01", "ap-01");
        let release = mouse(MouseEventKind::Up(MouseButton::Left), tap.column, tap.row);

        screen.handle_mouse_input(tap);
        let selected = screen.handle_mouse_input(release);
        assert!(matches!(selected, Some(Action::Invalidate)), "{selected:?}");
        assert_eq!(screen.selected, Some(screen.devices[3].id.clone()));

        screen.handle_mouse_input(tap);
        let opened = screen.handle_mouse_input(release);
        assert!(
            matches!(&opened, Some(Action::ShowDevice(id)) if *id == screen.devices[3].id),
            "{opened:?}"
        );

        // A drag that starts on a device pans and selects nothing new.
        let mut dragging = screen_with(3);
        let rows = draw(&dragging, 100, 30);
        let down = click_label(&rows, "ap-02", "ap-02");
        dragging.handle_mouse_input(down);
        dragging.handle_mouse_input(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            down.column + 3,
            down.row,
        ));
        let released = dragging.handle_mouse_input(mouse(
            MouseEventKind::Up(MouseButton::Left),
            down.column + 3,
            down.row,
        ));
        assert!(released.is_none() && dragging.selected.is_none());
    }
}
