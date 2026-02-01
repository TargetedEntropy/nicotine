use crate::config::Config;
use crate::window_manager::{EveWindow, WindowManager};
use anyhow::{Context, Result};
use std::sync::Arc;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;

pub struct X11Manager {
    conn: Arc<RustConnection>,
    screen_num: usize,
    net_active_window_atom: Atom,
}

impl X11Manager {
    pub fn new() -> Result<Self> {
        let (conn, screen_num) =
            RustConnection::connect(None).context("Failed to connect to X11 server")?;

        let conn = Arc::new(conn);

        // Pre-cache the _NET_ACTIVE_WINDOW atom (do roundtrip once at startup)
        let net_active_window_atom = conn
            .intern_atom(false, b"_NET_ACTIVE_WINDOW")?
            .reply()?
            .atom;

        Ok(Self {
            conn,
            screen_num,
            net_active_window_atom,
        })
    }

    pub fn get_eve_windows(&self) -> Result<Vec<EveWindow>> {
        let screen = &self.conn.setup().roots[self.screen_num];
        let root = screen.root;

        // Get _NET_CLIENT_LIST atom
        let net_client_list = self
            .conn
            .intern_atom(false, b"_NET_CLIENT_LIST")?
            .reply()?
            .atom;

        // Get list of all windows
        let client_list_reply = self
            .conn
            .get_property(false, root, net_client_list, AtomEnum::WINDOW, 0, u32::MAX)?
            .reply()?;

        let windows: Vec<u32> = client_list_reply
            .value32()
            .ok_or_else(|| anyhow::anyhow!("Failed to get window list"))?
            .collect();

        let mut eve_windows = Vec::new();

        for &window in &windows {
            if let Ok(title) = self.get_window_title(window) {
                // Filter for EVE windows (steam_app_8500) and exclude launcher
                if title.starts_with("EVE - ") && !title.contains("Launcher") {
                    // Determine which monitor this window is on based on its geometry
                    let monitor = self.get_window_monitor(window);
                    eve_windows.push(EveWindow {
                        id: window as u64,
                        title: title.trim_start_matches("EVE - ").to_string(),
                        monitor,
                    });
                }
            }
        }

        Ok(eve_windows)
    }

    pub fn get_active_window(&self) -> Result<u64> {
        let screen = &self.conn.setup().roots[self.screen_num];
        let root = screen.root;

        let net_active_window = self
            .conn
            .intern_atom(false, b"_NET_ACTIVE_WINDOW")?
            .reply()?
            .atom;

        let reply = self
            .conn
            .get_property(false, root, net_active_window, AtomEnum::WINDOW, 0, 1)?
            .reply()?;

        let active: Vec<u32> = reply
            .value32()
            .ok_or_else(|| anyhow::anyhow!("Failed to get active window"))?
            .collect();

        Ok(*active.first().unwrap_or(&0) as u64)
    }

    pub fn activate_window(&self, window_id: u64) -> Result<()> {
        let screen = &self.conn.setup().roots[self.screen_num];
        let root = screen.root;
        let window_id_u32 = window_id as u32;

        let current_active = self.get_active_window().unwrap_or(0) as u32;

        let event = ClientMessageEvent {
            response_type: CLIENT_MESSAGE_EVENT,
            format: 32,
            sequence: 0,
            window: window_id_u32,
            type_: self.net_active_window_atom,
            data: ClientMessageData::from([2, x11rb::CURRENT_TIME, current_active, 0, 0]),
        };

        self.conn.send_event(
            false,
            root,
            EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT,
            event,
        )?;

        self.conn
            .set_input_focus(InputFocus::PARENT, window_id_u32, x11rb::CURRENT_TIME)?;

        self.conn.flush()?;
        Ok(())
    }

    fn get_window_title(&self, window: u32) -> Result<String> {
        // Try _NET_WM_NAME first (UTF-8)
        let net_wm_name = self.conn.intern_atom(false, b"_NET_WM_NAME")?.reply()?.atom;

        let utf8_string = self.conn.intern_atom(false, b"UTF8_STRING")?.reply()?.atom;

        if let Ok(reply) = self
            .conn
            .get_property(false, window, net_wm_name, utf8_string, 0, 1024)?
            .reply()
        {
            if !reply.value.is_empty() {
                if let Ok(title) = String::from_utf8(reply.value.clone()) {
                    return Ok(title);
                }
            }
        }

        // Fall back to WM_NAME
        if let Ok(reply) = self
            .conn
            .get_property(false, window, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 1024)?
            .reply()
        {
            if !reply.value.is_empty() {
                return Ok(String::from_utf8_lossy(&reply.value).to_string());
            }
        }

        Ok(String::new())
    }

    pub fn find_window_by_title(&self, title: &str) -> Result<Option<u64>> {
        let screen = &self.conn.setup().roots[self.screen_num];
        let root = screen.root;

        let net_client_list = self
            .conn
            .intern_atom(false, b"_NET_CLIENT_LIST")?
            .reply()?
            .atom;

        let client_list_reply = self
            .conn
            .get_property(false, root, net_client_list, AtomEnum::WINDOW, 0, u32::MAX)?
            .reply()?;

        let windows: Vec<u32> = client_list_reply
            .value32()
            .ok_or_else(|| anyhow::anyhow!("Failed to get window list"))?
            .collect();

        for &window in &windows {
            if let Ok(window_title) = self.get_window_title(window) {
                if window_title == title {
                    return Ok(Some(window as u64));
                }
            }
        }

        Ok(None)
    }

    pub fn move_window(&self, window_id: u64, x: i32, y: i32) -> Result<()> {
        let values = ConfigureWindowAux::new().x(x).y(y);
        self.conn.configure_window(window_id as u32, &values)?;
        self.conn.flush()?;
        Ok(())
    }

    pub fn minimize_window(&self, window_id: u64) -> Result<()> {
        // Use WM_CHANGE_STATE with IconicState to minimize
        let wm_change_state = self
            .conn
            .intern_atom(false, b"WM_CHANGE_STATE")?
            .reply()?
            .atom;

        let screen = &self.conn.setup().roots[self.screen_num];
        let root = screen.root;
        let window_id_u32 = window_id as u32;

        // IconicState = 3
        let event = ClientMessageEvent {
            response_type: CLIENT_MESSAGE_EVENT,
            format: 32,
            sequence: 0,
            window: window_id_u32,
            type_: wm_change_state,
            data: ClientMessageData::from([3u32, 0, 0, 0, 0]),
        };

        self.conn.send_event(
            false,
            root,
            EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT,
            event,
        )?;

        self.conn.flush()?;
        Ok(())
    }

    pub fn restore_window(&self, window_id: u64) -> Result<()> {
        // Map the window to restore it from minimized state
        self.conn.map_window(window_id as u32)?;
        self.conn.flush()?;
        Ok(())
    }

    /// Get monitor geometry using xrandr
    pub fn get_monitors_internal(&self) -> Result<Vec<crate::window_manager::Monitor>> {
        use std::process::Command;

        let output = Command::new("xrandr")
            .arg("--query")
            .output()
            .context("Failed to execute xrandr")?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut monitors = Vec::new();

        // Parse xrandr output: "DP-1 connected primary 2560x1440+0+0 ..."
        for line in stdout.lines() {
            if line.contains(" connected") {
                // Find geometry pattern: WIDTHxHEIGHT+X+Y
                let parts: Vec<&str> = line.split_whitespace().collect();
                let name = parts.first().map(|s| s.to_string()).unwrap_or_default();

                for part in &parts {
                    // Match pattern like "2560x1440+0+0"
                    if part.contains('x') && part.contains('+') {
                        if let Some((res, pos)) = part.split_once('+') {
                            if let Some((width_str, height_str)) = res.split_once('x') {
                                let pos_parts: Vec<&str> = pos.split('+').collect();
                                if pos_parts.len() >= 2 {
                                    if let (Ok(width), Ok(height), Ok(x), Ok(y)) = (
                                        width_str.parse::<u32>(),
                                        height_str.parse::<u32>(),
                                        pos_parts[0].parse::<i32>(),
                                        pos_parts[1].parse::<i32>(),
                                    ) {
                                        monitors.push(crate::window_manager::Monitor {
                                            name,
                                            x,
                                            y,
                                            width,
                                            height,
                                        });
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(monitors)
    }

    /// Determine which monitor a window is on based on its geometry
    fn get_window_monitor(&self, window: u32) -> Option<String> {
        let geom = self.conn.get_geometry(window).ok()?.reply().ok()?;
        let monitors = self.get_monitors_internal().ok()?;

        // Window center point
        let win_center_x = geom.x as i32 + (geom.width as i32 / 2);
        let win_center_y = geom.y as i32 + (geom.height as i32 / 2);

        // Find which monitor contains the window center
        for mon in &monitors {
            if win_center_x >= mon.x
                && win_center_x < mon.x + mon.width as i32
                && win_center_y >= mon.y
                && win_center_y < mon.y + mon.height as i32
            {
                return Some(mon.name.clone());
            }
        }

        // Fallback: return first monitor
        monitors.first().map(|m| m.name.clone())
    }
}

impl WindowManager for X11Manager {
    fn get_eve_windows(&self) -> Result<Vec<EveWindow>> {
        self.get_eve_windows()
    }

    fn activate_window(&self, window_id: u64) -> Result<()> {
        self.activate_window(window_id)
    }

    fn stack_windows(&self, windows: &[EveWindow], config: &Config) -> Result<()> {
        let monitors = self.get_monitors()?;

        for window in windows {
            // Determine target monitor:
            // - Primary character goes to primary_monitor
            // - Others stay on their current monitor
            let is_primary = config
                .primary_character
                .as_ref()
                .map(|c| window.title == *c)
                .unwrap_or(false);

            let target_monitor = if is_primary {
                // Primary character goes to primary_monitor
                config
                    .primary_monitor
                    .as_ref()
                    .and_then(|name| monitors.iter().find(|m| &m.name == name))
                    .or_else(|| monitors.first())
            } else {
                // Others stay on current monitor
                window
                    .monitor
                    .as_ref()
                    .and_then(|name| monitors.iter().find(|m| &m.name == name))
                    .or_else(|| monitors.first())
            };

            let (x, y, width, height) = if let Some(mon) = target_monitor {
                if config.fullscreen_stack {
                    // Fullscreen on monitor
                    let height = mon.height.saturating_sub(config.panel_height);
                    (mon.x, mon.y, mon.width, height)
                } else {
                    // Centered with eve_width
                    let eve_w = config.eve_width.min(mon.width);
                    let x = mon.x + ((mon.width - eve_w) / 2) as i32;
                    let height = mon.height.saturating_sub(config.panel_height);
                    (x, mon.y, eve_w, height)
                }
            } else {
                // Fallback to global config
                let x = ((config.display_width - config.eve_width) / 2) as i32;
                let height = config.display_height - config.panel_height;
                (x, 0, config.eve_width, height)
            };

            let values = ConfigureWindowAux::new()
                .x(x)
                .y(y)
                .width(width)
                .height(height);

            self.conn.configure_window(window.id as u32, &values)?;
        }

        self.conn.flush()?;
        Ok(())
    }

    fn get_active_window(&self) -> Result<u64> {
        self.get_active_window()
    }

    fn find_window_by_title(&self, title: &str) -> Result<Option<u64>> {
        self.find_window_by_title(title)
    }

    fn move_window(&self, window_id: u64, x: i32, y: i32) -> Result<()> {
        self.move_window(window_id, x, y)
    }

    fn minimize_window(&self, window_id: u64) -> Result<()> {
        self.minimize_window(window_id)
    }

    fn restore_window(&self, window_id: u64) -> Result<()> {
        self.restore_window(window_id)
    }

    fn get_monitors(&self) -> Result<Vec<crate::window_manager::Monitor>> {
        self.get_monitors_internal()
    }
}
