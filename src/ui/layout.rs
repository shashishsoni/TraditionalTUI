//! Responsive workbench layout calculation.

use ratatui::layout::Rect;

use crate::config::Config;

/// Visibility and sizing state for the five workbench zones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutState {
    pub left_visible: bool,
    pub right_visible: bool,
    pub bottom_visible: bool,
    pub zoom: bool,
    pub left_width: u16,
    pub right_width: u16,
    pub bottom_height: u16,
}

impl Default for LayoutState {
    fn default() -> Self {
        Self {
            left_visible: true,
            right_visible: true,
            bottom_visible: true,
            zoom: false,
            left_width: 28,
            right_width: 38,
            bottom_height: 12,
        }
    }
}

impl LayoutState {
    pub fn from_config(config: &Config) -> Self {
        Self {
            left_visible: config.ui.show_left_sidebar,
            right_visible: config.ui.show_right_sidebar,
            bottom_visible: config.ui.show_bottom_panel,
            zoom: false,
            left_width: config.ui.left_width,
            right_width: config.ui.right_width,
            bottom_height: config.ui.bottom_height,
        }
    }

    pub fn toggle_explorer(&mut self) {
        self.left_visible = !self.left_visible;
    }

    pub fn toggle_bottom(&mut self) {
        self.bottom_visible = !self.bottom_visible;
    }

    pub fn toggle_ai(&mut self) {
        self.right_visible = !self.right_visible;
    }

    pub fn toggle_zoom(&mut self) {
        self.zoom = !self.zoom;
    }
}

/// Computed screen rectangles for every visible zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Regions {
    pub explorer: Option<Rect>,
    pub tab_bar: Rect,
    pub editor: Rect,
    pub bottom: Option<Rect>,
    pub ai: Option<Rect>,
    pub status: Rect,
}

/// Compute the workbench layout with responsive auto-collapse.
pub fn compute(area: Rect, state: &LayoutState) -> Regions {
    if area.width == 0 || area.height == 0 {
        let zero = Rect::new(area.x, area.y, 0, 0);
        return Regions {
            explorer: None,
            tab_bar: zero,
            editor: zero,
            bottom: None,
            ai: None,
            status: zero,
        };
    }

    let status_height = 1.min(area.height);
    let body = Rect::new(
        area.x,
        area.y,
        area.width,
        area.height.saturating_sub(status_height),
    );
    let status = Rect::new(area.x, area.y + body.height, area.width, status_height);

    if state.zoom || body.width < 50 || body.height == 0 {
        let tab_height = 1.min(body.height);
        return Regions {
            explorer: None,
            tab_bar: Rect::new(body.x, body.y, body.width, tab_height),
            editor: Rect::new(
                body.x,
                body.y + tab_height,
                body.width,
                body.height.saturating_sub(tab_height),
            ),
            bottom: None,
            ai: None,
            status,
        };
    }

    let left_width = if state.left_visible && body.width >= 75 {
        state.left_width.min(body.width / 4).max(18)
    } else {
        0
    };
    let right_width = if state.right_visible && body.width >= 100 {
        state.right_width.min(body.width / 3).max(26)
    } else {
        0
    };
    let center_width = body.width.saturating_sub(left_width + right_width);
    let center_x = body.x + left_width;

    let bottom_height = if state.bottom_visible && body.height >= 14 {
        state
            .bottom_height
            .min(body.height.saturating_sub(6))
            .max(6)
    } else {
        0
    };
    let center_height = body.height.saturating_sub(bottom_height);
    let tab_height = 1.min(center_height);

    Regions {
        explorer: (left_width > 0).then(|| Rect::new(body.x, body.y, left_width, body.height)),
        tab_bar: Rect::new(center_x, body.y, center_width, tab_height),
        editor: Rect::new(
            center_x,
            body.y + tab_height,
            center_width,
            center_height.saturating_sub(tab_height),
        ),
        bottom: (bottom_height > 0).then(|| {
            Rect::new(
                center_x,
                body.y + center_height,
                center_width,
                bottom_height,
            )
        }),
        ai: (right_width > 0)
            .then(|| Rect::new(center_x + center_width, body.y, right_width, body.height)),
        status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_layout_shows_all_zones() {
        let regions = compute(Rect::new(0, 0, 200, 60), &LayoutState::default());
        assert!(regions.explorer.is_some());
        assert!(regions.ai.is_some());
        assert!(regions.bottom.is_some());
        assert!(regions.editor.width >= 50);
        assert_eq!(regions.status.y, 59);
    }

    #[test]
    fn narrow_layout_collapses_right_then_left() {
        let medium = compute(Rect::new(0, 0, 90, 40), &LayoutState::default());
        assert!(medium.ai.is_none());
        assert!(medium.explorer.is_some());

        let narrow = compute(Rect::new(0, 0, 70, 40), &LayoutState::default());
        assert!(narrow.explorer.is_none());
        assert!(narrow.ai.is_none());
        assert!(narrow.editor.width >= 50);
    }

    #[test]
    fn zoom_hides_every_panel() {
        let mut state = LayoutState::default();
        state.toggle_zoom();
        let regions = compute(Rect::new(0, 0, 160, 50), &state);
        assert!(regions.explorer.is_none());
        assert!(regions.ai.is_none());
        assert!(regions.bottom.is_none());
        assert_eq!(regions.editor.width, 160);
    }

    #[test]
    fn short_terminal_hides_bottom_panel() {
        let regions = compute(Rect::new(0, 0, 140, 12), &LayoutState::default());
        assert!(regions.bottom.is_none());
    }

    #[test]
    fn zero_sized_area_is_safe() {
        let regions = compute(Rect::new(0, 0, 0, 0), &LayoutState::default());
        assert_eq!(regions.editor.width, 0);
        assert_eq!(regions.status.height, 0);
    }
}
