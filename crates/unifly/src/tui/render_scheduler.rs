//! Redraw readiness decisions for the TUI event loop.

pub(super) fn should_draw(needs_redraw: bool, effects_active: bool) -> bool {
    needs_redraw || effects_active
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_clean_frame_does_not_draw() {
        assert!(!should_draw(false, false));
    }

    #[test]
    fn redraw_request_draws() {
        assert!(should_draw(true, false));
    }

    #[test]
    fn active_effect_draws() {
        assert!(should_draw(false, true));
    }
}
