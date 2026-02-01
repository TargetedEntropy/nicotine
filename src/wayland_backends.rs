use crate::config::Config;
use crate::window_manager::{EveWindow, WindowManager};
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
}

impl WindowManager for KWinManager {
    fn get_eve_windows(&self) -> Result<Vec<EveWindow>> {
        let windows = self.get_all_windows()?;
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
                    eve_windows.push(EveWindow {
                        id,
                        title: title.trim_start_matches("EVE - ").to_string(),
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
        let x = ((config.display_width - config.eve_width) / 2) as i32;
        let y = 0;
        let width = config.eve_width;
        let height = config.display_height - config.panel_height;

        for window in windows {
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

    fn get_all_windows(&self) -> Result<Vec<Value>> {
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
        Self::extract_windows(&tree, &mut windows);

        Ok(windows)
    }

    fn extract_windows(node: &Value, windows: &mut Vec<Value>) {
        if let Some(node_type) = node.get("type").and_then(|t| t.as_str()) {
            if node_type == "con" || node_type == "floating_con" {
                if let Some(app_id) = node.get("app_id") {
                    if !app_id.is_null() {
                        windows.push(node.clone());
                    }
                } else if let Some(window_properties) = node.get("window_properties") {
                    if !window_properties.is_null() {
                        windows.push(node.clone());
                    }
                }
            }
        }

        if let Some(nodes) = node.get("nodes").and_then(|n| n.as_array()) {
            for child in nodes {
                Self::extract_windows(child, windows);
            }
        }

        if let Some(floating_nodes) = node.get("floating_nodes").and_then(|n| n.as_array()) {
            for child in floating_nodes {
                Self::extract_windows(child, windows);
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

        for window in windows {
            if let Some(title) = Self::get_window_title(&window) {
                if title.starts_with("EVE - ") && !title.contains("Launcher") {
                    if let Some(id) = Self::get_window_id(&window) {
                        eve_windows.push(EveWindow {
                            id,
                            title: title.trim_start_matches("EVE - ").to_string(),
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
        let x = ((config.display_width - config.eve_width) / 2) as i32;
        let y = 0;
        let width = config.eve_width as i32;
        let height = (config.display_height - config.panel_height) as i32;

        for window in windows {
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

    fn get_active_window(&self) -> Result<u64> {
        let windows = self.get_all_windows()?;

        for window in windows {
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

        for window in windows {
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

                        eve_windows.push(EveWindow {
                            id,
                            title: title.trim_start_matches("EVE - ").to_string(),
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
        let x = ((config.display_width - config.eve_width) / 2) as i32;
        let y = 0;
        let width = config.eve_width as i32;
        let height = (config.display_height - config.panel_height) as i32;

        for window in windows {
            let address = format!("0x{:x}", window.id);

            // Enable floating
            let output = Command::new("hyprctl")
                .arg("dispatch")
                .arg("togglefloating")
                .arg(format!("address:{}", address))
                .output()
                .context("Failed to execute hyprctl")?;

            if !output.status.success() {
                anyhow::bail!(
                    "hyprctl failed to toggle floating for window {}: {}",
                    address,
                    String::from_utf8_lossy(&output.stderr)
                );
            }

            // Move window
            let output = Command::new("hyprctl")
                .arg("dispatch")
                .arg("movewindowpixel")
                .arg(format!("exact {} {},address:{}", x, y, address))
                .output()
                .context("Failed to execute hyprctl")?;

            if !output.status.success() {
                anyhow::bail!(
                    "hyprctl failed to move window {}: {}",
                    address,
                    String::from_utf8_lossy(&output.stderr)
                );
            }

            // Resize window
            let output = Command::new("hyprctl")
                .arg("dispatch")
                .arg("resizewindowpixel")
                .arg(format!("exact {} {},address:{}", width, height, address))
                .output()
                .context("Failed to execute hyprctl")?;

            if !output.status.success() {
                anyhow::bail!(
                    "hyprctl failed to resize window {}: {}",
                    address,
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }

        Ok(())
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
