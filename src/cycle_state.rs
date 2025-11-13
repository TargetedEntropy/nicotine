use crate::x11_manager::{EveWindow, X11Manager};
use anyhow::Result;
use std::sync::{Arc, Mutex};

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
