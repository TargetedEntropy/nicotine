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
                    eve_windows.push(EveWindow {
                        id: window,
                        title: title.trim_start_matches("EVE - ").to_string(),
                    });
                }
            }
        }

        Ok(eve_windows)
    }

    pub fn get_active_window(&self) -> Result<u32> {
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

        Ok(*active.first().unwrap_or(&0))
    }

    pub fn activate_window(&self, window_id: u32) -> Result<()> {
        let screen = &self.conn.setup().roots[self.screen_num];
        let root = screen.root;

        let current_active = self.get_active_window().unwrap_or(0);

        let event = ClientMessageEvent {
            response_type: CLIENT_MESSAGE_EVENT,
            format: 32,
            sequence: 0,
            window: window_id,
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
            .set_input_focus(InputFocus::PARENT, window_id, x11rb::CURRENT_TIME)?;

        self.conn.flush()?;
        Ok(())
    }

    pub fn stack_windows_internal(
        &self,
        windows: &[EveWindow],
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<()> {
        for window in windows {
            // Move and resize window
            let values = ConfigureWindowAux::new()
                .x(x)
                .y(y)
                .width(width)
                .height(height);

            self.conn.configure_window(window.id, &values)?;
        }

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

    pub fn find_window_by_title(&self, title: &str) -> Result<Option<u32>> {
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
                    return Ok(Some(window));
                }
            }
        }

        Ok(None)
    }

    pub fn move_window(&self, window_id: u32, x: i32, y: i32) -> Result<()> {
        let values = ConfigureWindowAux::new().x(x).y(y);
        self.conn.configure_window(window_id, &values)?;
        self.conn.flush()?;
        Ok(())
    }
}

impl WindowManager for X11Manager {
    fn get_eve_windows(&self) -> Result<Vec<EveWindow>> {
        self.get_eve_windows()
    }

    fn activate_window(&self, window_id: u32) -> Result<()> {
        self.activate_window(window_id)
    }

    fn stack_windows(&self, windows: &[EveWindow], config: &Config) -> Result<()> {
        let x = ((config.display_width - config.eve_width) / 2) as i32;
        let y = 0;
        let width = config.eve_width;
        let height = config.display_height - config.panel_height;

        self.stack_windows_internal(windows, x, y, width, height)
    }

    fn get_active_window(&self) -> Result<u32> {
        self.get_active_window()
    }

    fn find_window_by_title(&self, title: &str) -> Result<Option<u32>> {
        self.find_window_by_title(title)
    }

    fn move_window(&self, window_id: u32, x: i32, y: i32) -> Result<()> {
        self.move_window(window_id, x, y)
    }
}
