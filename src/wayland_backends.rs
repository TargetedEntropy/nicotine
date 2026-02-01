use crate::config::Config;
use crate::window_manager::{EveWindow, Monitor, WindowManager};
use anyhow::{Context, Result};
use serde_json::Value;
use std::process::Command;

// ============================================================================
// KDE Plasma / KWin Backend (via wmctrl through XWayland)
// ============================================================================

pub struct KWinManager;

impl KWinManager {
    pub fn new() -> Result<Self> {
        Command::new("wmctrl")
            .arg("-m")
            .output()
            .context("wmctrl not found. Install wmctrl package")?;

        Ok(Self)
    }

    fn get_all_windows(&self) -> Result<Vec<(String, String)>> {
        let output = Command::new("wmctrl")
            .arg("-l")
            .output()
            .context("Failed to execute wmctrl")?;

        if !output.status.success() {
            anyhow::bail!("wmctrl failed: {}", String::from_utf8_lossy(&output.stderr));
        }

        let mut windows = Vec::new();
        let lines = String::from_utf8_lossy(&output.stdout);

        for line in lines.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                let window_id = parts[0];
                let title = parts[3..].join(" ");
                windows.push((window_id.to_string(), title));
            }
        }

        Ok(windows)
    }

    fn get_window_title_by_id(&self, hex_id: &str) -> Option<String> {
        let output = Command::new("wmctrl").arg("-l").output().ok()?;
        if !output.status.success() {
            return None;
        }

        let lines = String::from_utf8_lossy(&output.stdout);
        for line in lines.lines() {
            if line.starts_with(hex_id) {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 4 {
                    return Some(parts[3..].join(" "));
                }
            }
        }
        None
    }

    /// Get monitor geometry using xrandr (works through XWayland)
    fn get_monitors_internal(&self) -> Result<Vec<Monitor>> {
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
                                        monitors.push(Monitor {
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

    /// Determine which monitor a window is on using wmctrl -lG
    fn get_window_monitor(&self, hex_id: &str, monitors: &[Monitor]) -> Option<String> {
        let output = Command::new("wmctrl").args(["-l", "-G"]).output().ok()?;
        if !output.status.success() {
            return None;
        }

        let lines = String::from_utf8_lossy(&output.stdout);
        for line in lines.lines() {
            if line.starts_with(hex_id) {
                // Format: 0x... desktop x y width height hostname title
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 6 {
                    let x: i32 = parts[2].parse().ok()?;
                    let y: i32 = parts[3].parse().ok()?;
                    let w: i32 = parts[4].parse().ok()?;
                    let h: i32 = parts[5].parse().ok()?;

                    // Window center
                    let center_x = x + w / 2;
                    let center_y = y + h / 2;

                    // Find containing monitor
                    for mon in monitors {
                        if center_x >= mon.x
                            && center_x < mon.x + mon.width as i32
                            && center_y >= mon.y
                            && center_y < mon.y + mon.height as i32
                        {
                            return Some(mon.name.clone());
                        }
                    }
                }
            }
        }

        monitors.first().map(|m| m.name.clone())
    }
}

impl WindowManager for KWinManager {
    fn get_eve_windows(&self) -> Result<Vec<EveWindow>> {
        let windows = self.get_all_windows()?;
        let monitors = self.get_monitors().unwrap_or_default();
        let mut eve_windows = Vec::new();

        for (id_str, title) in windows {
            if title.starts_with("EVE - ") && !title.contains("Launcher") {
                // Parse hex window ID (e.g., "0x06e00008") to u64
                let id = if let Some(hex) = id_str.strip_prefix("0x") {
                    u64::from_str_radix(hex, 16).unwrap_or(0)
                } else {
                    id_str.parse::<u64>().unwrap_or(0)
                };

                if id != 0 {
                    // Determine which monitor the window is on based on its geometry
                    let monitor = self.get_window_monitor(&id_str, &monitors);
                    eve_windows.push(EveWindow {
                        id,
                        title: title.trim_start_matches("EVE - ").to_string(),
                        monitor,
                    });
                }
            }
        }

        Ok(eve_windows)
    }

    fn activate_window(&self, window_id: u64) -> Result<()> {
        let hex_id = format!("0x{:08x}", window_id);

        if let Some(title) = self.get_window_title_by_id(&hex_id) {
            if Command::new("kdotool")
                .args(["search", "--name", &title, "windowactivate"])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
            {
                return Ok(());
            }
        }

        Command::new("wmctrl")
            .args(["-i", "-a", &hex_id])
            .output()
            .context("Failed to activate window")?;

        Ok(())
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

            // Convert u32 to hex format for wmctrl
            let hex_id = format!("0x{:08x}", window.id);

            // Move and resize window using wmctrl
            let output = Command::new("wmctrl")
                .arg("-i")
                .arg("-r")
                .arg(&hex_id)
                .arg("-e")
                .arg(format!("0,{},{},{},{}", x, y, width, height))
                .output()
                .context("Failed to execute wmctrl")?;

            if !output.status.success() {
                anyhow::bail!(
                    "wmctrl failed to stack window {}: {}",
                    hex_id,
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }

        Ok(())
    }

    fn get_monitors(&self) -> Result<Vec<Monitor>> {
        self.get_monitors_internal()
    }

    fn get_active_window(&self) -> Result<u64> {
        // Use xdotool to get active window (works through XWayland)
        let output = Command::new("xdotool")
            .arg("getactivewindow")
            .output()
            .context("Failed to get active window")?;

        let window_id = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<u64>()
            .context("Failed to parse active window ID")?;

        Ok(window_id)
    }

    fn find_window_by_title(&self, title: &str) -> Result<Option<u64>> {
        let windows = self.get_all_windows()?;

        for (id_str, window_title) in windows {
            if window_title == title {
                // Parse hex window ID (e.g., "0x06e00008") to u64
                let id = if let Some(hex) = id_str.strip_prefix("0x") {
                    u64::from_str_radix(hex, 16).unwrap_or(0)
                } else {
                    id_str.parse::<u64>().unwrap_or(0)
                };

                if id != 0 {
                    return Ok(Some(id));
                }
            }
        }

        Ok(None)
    }

    fn minimize_window(&self, window_id: u64) -> Result<()> {
        let hex_id = format!("0x{:08x}", window_id);
        Command::new("xdotool")
            .args(["windowminimize", &hex_id])
            .output()
            .context("Failed to minimize window")?;
        Ok(())
    }

    fn restore_window(&self, window_id: u64) -> Result<()> {
        let hex_id = format!("0x{:08x}", window_id);
        // wmctrl -i -a activates and restores from minimized state
        Command::new("wmctrl")
            .args(["-i", "-a", &hex_id])
            .output()
            .context("Failed to restore window")?;
        Ok(())
    }
}

// ============================================================================
// Sway Backend (via swaymsg)
// ============================================================================

pub struct SwayManager;

impl SwayManager {
    pub fn new() -> Result<Self> {
        // Verify swaymsg is available
        Command::new("swaymsg")
            .arg("--version")
            .output()
            .context("swaymsg not found. Make sure you're running Sway")?;

        Ok(Self)
    }

    fn get_all_windows(&self) -> Result<Vec<(Value, Option<String>)>> {
        let output = Command::new("swaymsg")
            .arg("-t")
            .arg("get_tree")
            .output()
            .context("Failed to execute swaymsg")?;

        if !output.status.success() {
            anyhow::bail!(
                "swaymsg failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let tree: Value =
            serde_json::from_slice(&output.stdout).context("Failed to parse swaymsg output")?;

        let mut windows = Vec::new();
        Self::extract_windows(&tree, &mut windows, None);

        Ok(windows)
    }

    fn get_monitors_internal(&self) -> Result<Vec<Monitor>> {
        let output = Command::new("swaymsg")
            .args(["-t", "get_outputs"])
            .output()
            .context("Failed to execute swaymsg")?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let outputs: Vec<Value> =
            serde_json::from_slice(&output.stdout).context("Failed to parse swaymsg output")?;

        let mut monitors = Vec::new();
        for output in outputs {
            if let (Some(name), Some(rect)) = (
                output.get("name").and_then(|n| n.as_str()),
                output.get("rect"),
            ) {
                if let (Some(x), Some(y), Some(width), Some(height)) = (
                    rect.get("x").and_then(|v| v.as_i64()),
                    rect.get("y").and_then(|v| v.as_i64()),
                    rect.get("width").and_then(|v| v.as_u64()),
                    rect.get("height").and_then(|v| v.as_u64()),
                ) {
                    monitors.push(Monitor {
                        name: name.to_string(),
                        x: x as i32,
                        y: y as i32,
                        width: width as u32,
                        height: height as u32,
                    });
                }
            }
        }

        Ok(monitors)
    }

    fn extract_windows(
        node: &Value,
        windows: &mut Vec<(Value, Option<String>)>,
        current_output: Option<&str>,
    ) {
        let node_type = node.get("type").and_then(|t| t.as_str());

        // Track output name when we encounter an output node
        let output_name = if node_type == Some("output") {
            node.get("name").and_then(|n| n.as_str())
        } else {
            current_output
        };

        if let Some(nt) = node_type {
            if nt == "con" || nt == "floating_con" {
                if let Some(app_id) = node.get("app_id") {
                    if !app_id.is_null() {
                        windows.push((node.clone(), output_name.map(|s| s.to_string())));
                    }
                } else if let Some(window_properties) = node.get("window_properties") {
                    if !window_properties.is_null() {
                        windows.push((node.clone(), output_name.map(|s| s.to_string())));
                    }
                }
            }
        }

        if let Some(nodes) = node.get("nodes").and_then(|n| n.as_array()) {
            for child in nodes {
                Self::extract_windows(child, windows, output_name);
            }
        }

        if let Some(floating_nodes) = node.get("floating_nodes").and_then(|n| n.as_array()) {
            for child in floating_nodes {
                Self::extract_windows(child, windows, output_name);
            }
        }
    }

    fn get_window_title(window: &Value) -> Option<String> {
        window
            .get("name")
            .and_then(|n| n.as_str())
            .map(|s| s.to_string())
    }

    fn get_window_id(window: &Value) -> Option<u64> {
        window.get("id").and_then(|i| i.as_u64())
    }
}

impl WindowManager for SwayManager {
    fn get_eve_windows(&self) -> Result<Vec<EveWindow>> {
        let windows = self.get_all_windows()?;
        let mut eve_windows = Vec::new();

        for (window, output_name) in windows {
            if let Some(title) = Self::get_window_title(&window) {
                if title.starts_with("EVE - ") && !title.contains("Launcher") {
                    if let Some(id) = Self::get_window_id(&window) {
                        eve_windows.push(EveWindow {
                            id,
                            title: title.trim_start_matches("EVE - ").to_string(),
                            monitor: output_name,
                        });
                    }
                }
            }
        }

        Ok(eve_windows)
    }

    fn activate_window(&self, window_id: u64) -> Result<()> {
        let output = Command::new("swaymsg")
            .arg(format!("[con_id={}] focus", window_id))
            .output()
            .context("Failed to activate window")?;

        if !output.status.success() {
            anyhow::bail!(
                "Failed to activate window: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
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
                    (mon.x, mon.y, mon.width as i32, height as i32)
                } else {
                    // Centered with eve_width
                    let eve_w = config.eve_width.min(mon.width);
                    let x = mon.x + ((mon.width - eve_w) / 2) as i32;
                    let height = mon.height.saturating_sub(config.panel_height);
                    (x, mon.y, eve_w as i32, height as i32)
                }
            } else {
                // Fallback to global config
                let x = ((config.display_width - config.eve_width) / 2) as i32;
                let height = (config.display_height - config.panel_height) as i32;
                (x, 0, config.eve_width as i32, height)
            };

            // Sway uses floating mode for positioning
            let output = Command::new("swaymsg")
                .arg(format!("[con_id={}] floating enable", window.id))
                .output()
                .context("Failed to execute swaymsg")?;

            if !output.status.success() {
                anyhow::bail!(
                    "swaymsg failed to enable floating for window {}: {}",
                    window.id,
                    String::from_utf8_lossy(&output.stderr)
                );
            }

            let output = Command::new("swaymsg")
                .arg(format!("[con_id={}] move position {} {}", window.id, x, y))
                .output()
                .context("Failed to execute swaymsg")?;

            if !output.status.success() {
                anyhow::bail!(
                    "swaymsg failed to move window {}: {}",
                    window.id,
                    String::from_utf8_lossy(&output.stderr)
                );
            }

            let output = Command::new("swaymsg")
                .arg(format!(
                    "[con_id={}] resize set {} {}",
                    window.id, width, height
                ))
                .output()
                .context("Failed to execute swaymsg")?;

            if !output.status.success() {
                anyhow::bail!(
                    "swaymsg failed to resize window {}: {}",
                    window.id,
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }

        Ok(())
    }

    fn get_monitors(&self) -> Result<Vec<Monitor>> {
        self.get_monitors_internal()
    }

    fn get_active_window(&self) -> Result<u64> {
        let windows = self.get_all_windows()?;

        for (window, _output) in windows {
            if let Some(focused) = window.get("focused").and_then(|f| f.as_bool()) {
                if focused {
                    if let Some(id) = Self::get_window_id(&window) {
                        return Ok(id);
                    }
                }
            }
        }

        anyhow::bail!("No active window found")
    }

    fn find_window_by_title(&self, title: &str) -> Result<Option<u64>> {
        let windows = self.get_all_windows()?;

        for (window, _output) in windows {
            if let Some(window_title) = Self::get_window_title(&window) {
                if window_title == title {
                    if let Some(id) = Self::get_window_id(&window) {
                        return Ok(Some(id));
                    }
                }
            }
        }

        Ok(None)
    }

    fn minimize_window(&self, window_id: u64) -> Result<()> {
        Command::new("swaymsg")
            .arg(format!("[con_id={}] move scratchpad", window_id))
            .output()
            .context("Failed to minimize window")?;
        Ok(())
    }

    fn restore_window(&self, window_id: u64) -> Result<()> {
        // Show from scratchpad restores it
        Command::new("swaymsg")
            .arg(format!("[con_id={}] scratchpad show", window_id))
            .output()
            .context("Failed to restore window")?;
        Ok(())
    }
}

// ============================================================================
// Hyprland Backend (via hyprctl)
// ============================================================================

pub struct HyprlandManager;

impl HyprlandManager {
    pub fn new() -> Result<Self> {
        // Verify hyprctl is available
        Command::new("hyprctl")
            .arg("version")
            .output()
            .context("hyprctl not found. Make sure you're running Hyprland")?;

        Ok(Self)
    }

    fn get_all_windows(&self) -> Result<Vec<Value>> {
        let output = Command::new("hyprctl")
            .arg("clients")
            .arg("-j")
            .output()
            .context("Failed to execute hyprctl")?;

        if !output.status.success() {
            anyhow::bail!(
                "hyprctl failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let windows: Vec<Value> =
            serde_json::from_slice(&output.stdout).context("Failed to parse hyprctl output")?;

        Ok(windows)
    }

    fn get_monitors_internal(&self) -> Result<Vec<Monitor>> {
        let output = Command::new("hyprctl")
            .args(["monitors", "-j"])
            .output()
            .context("Failed to execute hyprctl")?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let monitors_json: Vec<Value> =
            serde_json::from_slice(&output.stdout).context("Failed to parse hyprctl output")?;

        let mut monitors = Vec::new();
        for mon in monitors_json {
            if let (Some(name), Some(x), Some(y), Some(width), Some(height)) = (
                mon.get("name").and_then(|n| n.as_str()),
                mon.get("x").and_then(|v| v.as_i64()),
                mon.get("y").and_then(|v| v.as_i64()),
                mon.get("width").and_then(|v| v.as_u64()),
                mon.get("height").and_then(|v| v.as_u64()),
            ) {
                monitors.push(Monitor {
                    name: name.to_string(),
                    x: x as i32,
                    y: y as i32,
                    width: width as u32,
                    height: height as u32,
                });
            }
        }

        Ok(monitors)
    }
}

impl WindowManager for HyprlandManager {
    fn get_eve_windows(&self) -> Result<Vec<EveWindow>> {
        let windows = self.get_all_windows()?;
        let mut eve_windows = Vec::new();

        for window in windows {
            if let Some(title) = window.get("title").and_then(|t| t.as_str()) {
                if title.starts_with("EVE - ") && !title.contains("Launcher") {
                    // Hyprland uses hex addresses - must use u64 to avoid truncation
                    if let Some(address) = window.get("address").and_then(|a| a.as_str()) {
                        // Convert hex address like "0x55ade765da10" to u64
                        let id = if let Some(hex) = address.strip_prefix("0x") {
                            u64::from_str_radix(hex, 16).unwrap_or(0)
                        } else {
                            0
                        };

                        // Hyprland clients JSON has a "monitor" field with monitor ID
                        // We need to map this to the monitor name
                        let monitor =
                            window
                                .get("monitor")
                                .and_then(|m| m.as_i64())
                                .and_then(|mon_id| {
                                    // Get monitors to find name by ID
                                    self.get_monitors_internal().ok().and_then(|monitors| {
                                        // Monitor ID in clients corresponds to the order in monitors list
                                        monitors.get(mon_id as usize).map(|m| m.name.clone())
                                    })
                                });

                        eve_windows.push(EveWindow {
                            id,
                            title: title.trim_start_matches("EVE - ").to_string(),
                            monitor,
                        });
                    }
                }
            }
        }

        Ok(eve_windows)
    }

    fn activate_window(&self, window_id: u64) -> Result<()> {
        // Convert u64 back to hex address
        let address = format!("0x{:x}", window_id);

        let output = Command::new("hyprctl")
            .arg("dispatch")
            .arg("focuswindow")
            .arg(format!("address:{}", address))
            .output()
            .context("Failed to activate window")?;

        if !output.status.success() {
            anyhow::bail!(
                "Failed to activate window: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
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
                    (mon.x, mon.y, mon.width as i32, height as i32)
                } else {
                    // Centered with eve_width
                    let eve_w = config.eve_width.min(mon.width);
                    let x = mon.x + ((mon.width - eve_w) / 2) as i32;
                    let height = mon.height.saturating_sub(config.panel_height);
                    (x, mon.y, eve_w as i32, height as i32)
                }
            } else {
                // Fallback to global config
                let x = ((config.display_width - config.eve_width) / 2) as i32;
                let height = (config.display_height - config.panel_height) as i32;
                (x, 0, config.eve_width as i32, height)
            };

            let address = format!("0x{:x}", window.id);

            // Enable floating (setfloating 1 = always float, unlike togglefloating)
            let _ = Command::new("hyprctl")
                .arg("dispatch")
                .arg("setfloating")
                .arg(format!("address:{}", address))
                .output();

            // Try to move window - if fullscreen, exit fullscreen and retry
            let output = Command::new("hyprctl")
                .arg("dispatch")
                .arg("movewindowpixel")
                .arg(format!("exact {} {},address:{}", x, y, address))
                .output()
                .context("Failed to execute hyprctl")?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.contains("Window is fullscreen") {
                // Exit fullscreen: focus window, use fullscreen 0 to exit, then retry move
                let _ = Command::new("hyprctl")
                    .arg("dispatch")
                    .arg("focuswindow")
                    .arg(format!("address:{}", address))
                    .output();
                let _ = Command::new("hyprctl")
                    .arg("dispatch")
                    .arg("fullscreen")
                    .arg("0")
                    .output();
                let _ = Command::new("hyprctl")
                    .arg("dispatch")
                    .arg("movewindowpixel")
                    .arg(format!("exact {} {},address:{}", x, y, address))
                    .output();
            }

            // Resize window (also retry if fullscreen)
            let output = Command::new("hyprctl")
                .arg("dispatch")
                .arg("resizewindowpixel")
                .arg(format!("exact {} {},address:{}", width, height, address))
                .output()
                .context("Failed to execute hyprctl")?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.contains("Window is fullscreen") {
                // Already exited fullscreen above, just retry
                let _ = Command::new("hyprctl")
                    .arg("dispatch")
                    .arg("resizewindowpixel")
                    .arg(format!("exact {} {},address:{}", width, height, address))
                    .output();
            }
        }

        Ok(())
    }

    fn get_monitors(&self) -> Result<Vec<Monitor>> {
        self.get_monitors_internal()
    }

    fn get_active_window(&self) -> Result<u64> {
        let output = Command::new("hyprctl")
            .arg("activewindow")
            .arg("-j")
            .output()
            .context("Failed to get active window")?;

        let window: Value =
            serde_json::from_slice(&output.stdout).context("Failed to parse hyprctl output")?;

        if let Some(address) = window.get("address").and_then(|a| a.as_str()) {
            let id = if let Some(hex) = address.strip_prefix("0x") {
                u64::from_str_radix(hex, 16).unwrap_or(0)
            } else {
                0
            };
            return Ok(id);
        }

        anyhow::bail!("Failed to get active window ID")
    }

    fn find_window_by_title(&self, title: &str) -> Result<Option<u64>> {
        let windows = self.get_all_windows()?;

        for window in windows {
            if let Some(window_title) = window.get("title").and_then(|t| t.as_str()) {
                if window_title == title {
                    if let Some(address) = window.get("address").and_then(|a| a.as_str()) {
                        let id = if let Some(hex) = address.strip_prefix("0x") {
                            u64::from_str_radix(hex, 16).unwrap_or(0)
                        } else {
                            0
                        };
                        return Ok(Some(id));
                    }
                }
            }
        }

        Ok(None)
    }

    fn minimize_window(&self, window_id: u64) -> Result<()> {
        let address = format!("0x{:x}", window_id);
        Command::new("hyprctl")
            .args([
                "dispatch",
                "movetoworkspacesilent",
                &format!("special,address:{}", address),
            ])
            .output()
            .context("Failed to minimize window")?;
        Ok(())
    }

    fn restore_window(&self, window_id: u64) -> Result<()> {
        let address = format!("0x{:x}", window_id);
        // Move back to current workspace
        Command::new("hyprctl")
            .args([
                "dispatch",
                "movetoworkspace",
                &format!("e+0,address:{}", address),
            ])
            .output()
            .context("Failed to restore window")?;
        Ok(())
    }
}
