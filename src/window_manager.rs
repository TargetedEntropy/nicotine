use crate::config::Config;
use anyhow::Result;

#[derive(Debug, Clone)]
pub struct EveWindow {
    pub id: u64,
    pub title: String,
}

/// Trait for window management across different display servers and compositors
pub trait WindowManager: Send + Sync {
    /// Get all EVE Online client windows
    fn get_eve_windows(&self) -> Result<Vec<EveWindow>>;

    /// Activate/focus a specific window by ID
    fn activate_window(&self, window_id: u64) -> Result<()>;

    /// Stack all EVE windows at the same position (centered)
    fn stack_windows(&self, windows: &[EveWindow], config: &Config) -> Result<()>;

    /// Get the currently active window ID
    fn get_active_window(&self) -> Result<u64>;

    /// Find a window by its title (returns window ID if found)
    fn find_window_by_title(&self, title: &str) -> Result<Option<u64>>;

    /// Move a window to a specific position (X11 only, no-op on Wayland)
    fn move_window(&self, window_id: u64, x: i32, y: i32) -> Result<()> {
        // Default implementation: no-op (Wayland doesn't allow arbitrary window positioning)
        let _ = (window_id, x, y);
        Ok(())
    }

    /// Minimize a window
    fn minimize_window(&self, window_id: u64) -> Result<()>;

    /// Restore a minimized window
    fn restore_window(&self, window_id: u64) -> Result<()>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayServer {
    X11,
    Wayland,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaylandCompositor {
    Kde,      // KDE Plasma / KWin
    Sway,     // Sway (wlroots)
    Hyprland, // Hyprland
    Gnome,    // GNOME Shell
    Other,    // Other/unknown compositor
}

/// Detect which display server is running
pub fn detect_display_server() -> DisplayServer {
    if let Ok(session_type) = std::env::var("XDG_SESSION_TYPE") {
        if session_type == "wayland" {
            return DisplayServer::Wayland;
        }
    }

    // Fallback: check if WAYLAND_DISPLAY is set
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        return DisplayServer::Wayland;
    }

    // Default to X11
    DisplayServer::X11
}

/// Detect which Wayland compositor is running
pub fn detect_wayland_compositor() -> WaylandCompositor {
    // Check XDG_CURRENT_DESKTOP first
    if let Ok(desktop) = std::env::var("XDG_CURRENT_DESKTOP") {
        let desktop_lower = desktop.to_lowercase();
        if desktop_lower.contains("kde") {
            return WaylandCompositor::Kde;
        }
        if desktop_lower.contains("gnome") {
            return WaylandCompositor::Gnome;
        }
        if desktop_lower.contains("sway") {
            return WaylandCompositor::Sway;
        }
        if desktop_lower.contains("hyprland") {
            return WaylandCompositor::Hyprland;
        }
    }

    // Check for compositor-specific environment variables
    if std::env::var("SWAYSOCK").is_ok() {
        return WaylandCompositor::Sway;
    }

    if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
        return WaylandCompositor::Hyprland;
    }

    WaylandCompositor::Other
}
