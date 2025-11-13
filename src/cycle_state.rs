use crate::x11_manager::{EveWindow, X11Manager};
use anyhow::Result;

pub struct CycleState {
    current_index: usize,
    windows: Vec<EveWindow>,
}

impl CycleState {
    pub fn new() -> Self {
        Self {
            current_index: 0,
            windows: Vec::new(),
        }
    }

    pub fn update_windows(&mut self, windows: Vec<EveWindow>) {
        self.windows = windows;
        // Clamp current index
        if self.current_index >= self.windows.len() && !self.windows.is_empty() {
            self.current_index = 0;
        }
    }

    pub fn cycle_forward(&mut self, x11: &X11Manager) -> Result<()> {
        if self.windows.is_empty() {
            return Ok(());
        }

        self.current_index = (self.current_index + 1) % self.windows.len();
        let window_id = self.windows[self.current_index].id;
        x11.activate_window(window_id)?;
        Ok(())
    }

    pub fn cycle_backward(&mut self, x11: &X11Manager) -> Result<()> {
        if self.windows.is_empty() {
            return Ok(());
        }

        if self.current_index == 0 {
            self.current_index = self.windows.len() - 1;
        } else {
            self.current_index -= 1;
        }

        let window_id = self.windows[self.current_index].id;
        x11.activate_window(window_id)?;
        Ok(())
    }

    pub fn get_windows(&self) -> &[EveWindow] {
        &self.windows
    }

    pub fn get_current_index(&self) -> usize {
        self.current_index
    }

    pub fn sync_with_active(&mut self, active_window: u32) {
        // Find which window is active and update current_index
        for (i, window) in self.windows.iter().enumerate() {
            if window.id == active_window {
                self.current_index = i;
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_window(id: u32, title: &str) -> EveWindow {
        EveWindow {
            id,
            title: title.to_string(),
        }
    }

    #[test]
    fn test_new_cycle_state_is_empty() {
        let state = CycleState::new();
        assert_eq!(state.get_current_index(), 0);
        assert_eq!(state.get_windows().len(), 0);
    }

    #[test]
    fn test_update_windows() {
        let mut state = CycleState::new();
        let windows = vec![
            create_test_window(1, "EVE - Character 1"),
            create_test_window(2, "EVE - Character 2"),
            create_test_window(3, "EVE - Character 3"),
        ];

        state.update_windows(windows);
        assert_eq!(state.get_windows().len(), 3);
        assert_eq!(state.get_current_index(), 0);
    }

    #[test]
    fn test_update_windows_clamps_index() {
        let mut state = CycleState::new();

        // Set up with 5 windows and move to index 4
        let windows = vec![
            create_test_window(1, "EVE - Character 1"),
            create_test_window(2, "EVE - Character 2"),
            create_test_window(3, "EVE - Character 3"),
            create_test_window(4, "EVE - Character 4"),
            create_test_window(5, "EVE - Character 5"),
        ];
        state.update_windows(windows);
        state.current_index = 4; // Manually set to last index

        // Now update with only 2 windows
        let windows = vec![
            create_test_window(1, "EVE - Character 1"),
            create_test_window(2, "EVE - Character 2"),
        ];
        state.update_windows(windows);

        // Index should be clamped back to 0
        assert_eq!(state.get_current_index(), 0);
    }

    #[test]
    fn test_sync_with_active_updates_index() {
        let mut state = CycleState::new();
        let windows = vec![
            create_test_window(100, "EVE - Character 1"),
            create_test_window(200, "EVE - Character 2"),
            create_test_window(300, "EVE - Character 3"),
        ];
        state.update_windows(windows);

        // Sync with window id 300
        state.sync_with_active(300);
        assert_eq!(state.get_current_index(), 2);

        // Sync with window id 100
        state.sync_with_active(100);
        assert_eq!(state.get_current_index(), 0);
    }

    #[test]
    fn test_sync_with_active_nonexistent_window() {
        let mut state = CycleState::new();
        let windows = vec![
            create_test_window(100, "EVE - Character 1"),
            create_test_window(200, "EVE - Character 2"),
        ];
        state.update_windows(windows);
        state.current_index = 1;

        // Sync with non-existent window - index shouldn't change
        state.sync_with_active(999);
        assert_eq!(state.get_current_index(), 1);
    }

    #[test]
    fn test_get_windows_returns_slice() {
        let mut state = CycleState::new();
        let windows = vec![
            create_test_window(1, "EVE - Character 1"),
            create_test_window(2, "EVE - Character 2"),
        ];
        state.update_windows(windows);

        let returned_windows = state.get_windows();
        assert_eq!(returned_windows.len(), 2);
        assert_eq!(returned_windows[0].id, 1);
        assert_eq!(returned_windows[1].id, 2);
    }

    #[test]
    fn test_empty_windows_stays_at_zero() {
        let mut state = CycleState::new();

        // Update with empty list
        state.update_windows(vec![]);

        assert_eq!(state.get_current_index(), 0);
        assert_eq!(state.get_windows().len(), 0);
    }

    #[test]
    fn test_single_window_behavior() {
        let mut state = CycleState::new();
        let windows = vec![create_test_window(1, "EVE - Single Client")];
        state.update_windows(windows);

        // With a single window, we should stay at index 0
        assert_eq!(state.get_current_index(), 0);

        // Syncing with the only window should work
        state.sync_with_active(1);
        assert_eq!(state.get_current_index(), 0);
    }

    #[test]
    fn test_update_windows_preserves_valid_index() {
        let mut state = CycleState::new();

        // Start with 5 windows, move to index 2
        let windows = vec![
            create_test_window(1, "EVE - Character 1"),
            create_test_window(2, "EVE - Character 2"),
            create_test_window(3, "EVE - Character 3"),
            create_test_window(4, "EVE - Character 4"),
            create_test_window(5, "EVE - Character 5"),
        ];
        state.update_windows(windows);
        state.current_index = 2;

        // Update with 4 windows - index 2 is still valid
        let windows = vec![
            create_test_window(1, "EVE - Character 1"),
            create_test_window(2, "EVE - Character 2"),
            create_test_window(3, "EVE - Character 3"),
            create_test_window(4, "EVE - Character 4"),
        ];
        state.update_windows(windows);

        // Index should stay at 2 since it's still valid
        assert_eq!(state.get_current_index(), 2);
    }
}
