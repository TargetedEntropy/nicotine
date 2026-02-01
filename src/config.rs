use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub display_width: u32,
    pub display_height: u32,
    pub panel_height: u32,
    pub eve_width: u32,
    pub eve_height: u32,
    pub overlay_x: f32,
    pub overlay_y: f32,
    #[serde(default = "default_enable_mouse")]
    pub enable_mouse_buttons: bool,
    #[serde(default = "default_forward_button")]
    pub forward_button: u16, // BTN_SIDE (mouse button 9)
    #[serde(default = "default_backward_button")]
    pub backward_button: u16, // BTN_EXTRA (mouse button 8)
    #[serde(default = "default_enable_keyboard")]
    pub enable_keyboard_buttons: bool,
    #[serde(default = "default_forward_key")]
    pub forward_key: u16, // KEY_TAB (15) - Tab for forward, Shift+Tab for backward
    #[serde(default = "default_backward_key")]
    pub backward_key: u16, // KEY_TAB (15) - Track SHIFT modifier internally
    #[serde(default = "default_show_overlay")]
    pub show_overlay: bool,
    #[serde(default = "default_mouse_device_name")]
    pub mouse_device_name: Option<String>,
    #[serde(default = "default_mouse_device_path")]
    pub mouse_device_path: Option<String>,
    #[serde(default = "default_minimize_inactive")]
    pub minimize_inactive: bool,
    #[serde(default = "default_keyboard_device_path")]
    pub keyboard_device_path: Option<String>,
    #[serde(default = "default_modifier_key")]
    pub modifier_key: Option<u16>,
    #[serde(default)]
    pub primary_character: Option<String>,
    #[serde(default)]
    pub primary_monitor: Option<String>,
    #[serde(default)]
    pub fullscreen_stack: bool,
}

fn default_enable_mouse() -> bool {
    true
}

fn default_forward_button() -> u16 {
    276 // BTN_SIDE (forward button, mouse button 9)
}

fn default_backward_button() -> u16 {
    275 // BTN_EXTRA (backward button, mouse button 8)
}

fn default_enable_keyboard() -> bool {
    false // Disabled by default to avoid conflicts
}

fn default_forward_key() -> u16 {
    15 // KEY_TAB
}

fn default_backward_key() -> u16 {
    15 // KEY_TAB (Modifier applied if set)
}

fn default_show_overlay() -> bool {
    true
}

fn default_mouse_device_name() -> Option<String> {
    None
}

fn default_mouse_device_path() -> Option<String> {
    None
}

fn default_minimize_inactive() -> bool {
    false
}

fn default_keyboard_device_path() -> Option<String> {
    None
}

fn default_modifier_key() -> Option<u16> {
    None // No modifier for backward shifting by default
}

impl Config {
    fn config_dir() -> PathBuf {
        let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("nicotine");
        path
    }

    fn config_path() -> PathBuf {
        let mut path = Self::config_dir();
        path.push("config.toml");
        path
    }

    /// Load character order from characters.txt
    /// Each line is a character name (without "EVE - " prefix)
    /// Returns None if file doesn't exist
    pub fn load_characters() -> Option<Vec<String>> {
        let mut path = Self::config_dir();
        path.push("characters.txt");

        if !path.exists() {
            return None;
        }

        fs::read_to_string(&path).ok().map(|contents| {
            contents
                .lines()
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .collect()
        })
    }

    fn detect_display_size() -> (u32, u32) {
        // Try xrandr first (works on X11 and some XWayland setups)
        if let Some(size) = Self::detect_via_xrandr() {
            println!("Display detected via xrandr");
            return size;
        }

        // Try swaymsg for Sway compositor
        if let Some(size) = Self::detect_via_swaymsg() {
            println!("Display detected via swaymsg");
            return size;
        }

        // Try hyprctl for Hyprland compositor
        if let Some(size) = Self::detect_via_hyprctl() {
            println!("Display detected via hyprctl");
            return size;
        }

        // Try wlr-randr for wlroots-based compositors
        if let Some(size) = Self::detect_via_wlr_randr() {
            println!("Display detected via wlr-randr");
            return size;
        }

        // Fallback to common resolution
        eprintln!("Warning: Could not detect display size, using default 1920x1080");
        eprintln!(
            "Edit ~/.config/nicotine/config.toml to set correct display_width and display_height"
        );
        (1920, 1080)
    }

    fn detect_via_xrandr() -> Option<(u32, u32)> {
        let output = std::process::Command::new("xrandr")
            .args(["--current"])
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8(output.stdout).ok()?;
        for line in stdout.lines() {
            if line.contains("*") && line.contains("x") {
                // Parse line like: "7680x2160     60.00*+"
                if let Some(resolution) = line.split_whitespace().next() {
                    if let Some((w, h)) = resolution.split_once('x') {
                        if let (Ok(width), Ok(height)) = (w.parse(), h.parse()) {
                            return Some((width, height));
                        }
                    }
                }
            }
        }
        None
    }

    fn detect_via_swaymsg() -> Option<(u32, u32)> {
        let output = std::process::Command::new("swaymsg")
            .args(["-t", "get_outputs"])
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8(output.stdout).ok()?;
        let outputs: serde_json::Value = serde_json::from_str(&stdout).ok()?;

        // Find the focused output or first active one
        if let Some(outputs_array) = outputs.as_array() {
            for output in outputs_array {
                let active = output
                    .get("active")
                    .and_then(|a| a.as_bool())
                    .unwrap_or(false);
                if active {
                    if let Some(rect) = output.get("rect") {
                        let width = rect.get("width").and_then(|w| w.as_u64())? as u32;
                        let height = rect.get("height").and_then(|h| h.as_u64())? as u32;
                        return Some((width, height));
                    }
                }
            }
        }
        None
    }

    fn detect_via_hyprctl() -> Option<(u32, u32)> {
        let output = std::process::Command::new("hyprctl")
            .args(["monitors", "-j"])
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8(output.stdout).ok()?;
        let monitors: serde_json::Value = serde_json::from_str(&stdout).ok()?;

        // Find the focused monitor or first one
        if let Some(monitors_array) = monitors.as_array() {
            for monitor in monitors_array {
                let focused = monitor
                    .get("focused")
                    .and_then(|f| f.as_bool())
                    .unwrap_or(false);
                if focused || monitors_array.len() == 1 {
                    let width = monitor.get("width").and_then(|w| w.as_u64())? as u32;
                    let height = monitor.get("height").and_then(|h| h.as_u64())? as u32;
                    return Some((width, height));
                }
            }
            // Fallback to first monitor if none focused
            if let Some(monitor) = monitors_array.first() {
                let width = monitor.get("width").and_then(|w| w.as_u64())? as u32;
                let height = monitor.get("height").and_then(|h| h.as_u64())? as u32;
                return Some((width, height));
            }
        }
        None
    }

    fn detect_via_wlr_randr() -> Option<(u32, u32)> {
        let output = std::process::Command::new("wlr-randr").output().ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8(output.stdout).ok()?;
        // Parse output like:
        // DP-1 "..."
        //   Enabled: yes
        //   Modes:
        //     3840x2160 px, 60.000000 Hz (preferred, current)
        for line in stdout.lines() {
            let trimmed = line.trim();
            if trimmed.contains("current") && trimmed.contains("x") && trimmed.contains("px") {
                // Parse line like: "3840x2160 px, 60.000000 Hz (preferred, current)"
                if let Some(resolution) = trimmed.split_whitespace().next() {
                    if let Some((w, h)) = resolution.split_once('x') {
                        if let (Ok(width), Ok(height)) = (w.parse(), h.parse()) {
                            return Some((width, height));
                        }
                    }
                }
            }
        }
        None
    }

    pub fn load() -> Result<Self> {
        let config_path = Self::config_path();

        // Try to load existing config
        if let Ok(contents) = fs::read_to_string(&config_path) {
            return toml::from_str(&contents).context("Failed to parse config.toml");
        }

        // Auto-generate config based on detected display
        println!("Generating config based on your display...");
        let (display_width, display_height) = Self::detect_display_size();
        println!("Detected display: {}x{}", display_width, display_height);

        let config = Self {
            display_width,
            display_height,
            panel_height: 0, // Assume no panel by default
            eve_width: (display_width as f32 * 0.54) as u32, // ~54% of width
            eve_height: display_height,
            overlay_x: 10.0,
            overlay_y: 10.0,
            enable_mouse_buttons: true,
            forward_button: 276,  // BTN_SIDE (button 9)
            backward_button: 275, // BTN_EXTRA (button 8)
            enable_keyboard_buttons: false,
            forward_key: 15,  // KEY_TAB
            backward_key: 15, // KEY_TAB (with Shift)
            show_overlay: true,
            mouse_device_name: None,
            mouse_device_path: None,
            minimize_inactive: false,
            keyboard_device_path: None,
            modifier_key: None,
            primary_character: None,
            primary_monitor: None,
            fullscreen_stack: false,
        };

        // Save the generated config
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let contents = toml::to_string_pretty(&config)?;
        fs::write(&config_path, contents)?;
        println!("Created config: {}", config_path.display());
        println!("Edit it to customize window sizes and positions");

        Ok(config)
    }

    pub fn save_default() -> Result<()> {
        let config_path = Self::config_path();
        let (display_width, display_height) = Self::detect_display_size();

        let config = Self {
            display_width,
            display_height,
            panel_height: 0,
            eve_width: (display_width as f32 * 0.54) as u32,
            eve_height: display_height,
            overlay_x: 10.0,
            overlay_y: 10.0,
            enable_mouse_buttons: true,
            forward_button: 276,
            backward_button: 275,
            enable_keyboard_buttons: false,
            forward_key: 15,
            backward_key: 15,
            show_overlay: true,
            mouse_device_name: None,
            mouse_device_path: None,
            minimize_inactive: false,
            keyboard_device_path: None,
            modifier_key: None,
            primary_character: None,
            primary_monitor: None,
            fullscreen_stack: false,
        };

        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let contents = toml::to_string_pretty(&config)?;
        fs::write(&config_path, contents)?;
        println!("Created config: {}", config_path.display());
        Ok(())
    }

    pub fn eve_height_adjusted(&self) -> u32 {
        self.display_height - self.panel_height
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eve_height_adjusted_with_panel() {
        let config = Config {
            display_width: 1920,
            display_height: 1080,
            panel_height: 40,
            eve_width: 1000,
            eve_height: 1080,
            overlay_x: 10.0,
            overlay_y: 10.0,
            enable_mouse_buttons: true,
            forward_button: 276,
            backward_button: 275,
            enable_keyboard_buttons: false,
            forward_key: 15,
            backward_key: 15,
            show_overlay: true,
            mouse_device_name: None,
            mouse_device_path: None,
            minimize_inactive: false,
            keyboard_device_path: None,
            modifier_key: None,
            primary_character: None,
            primary_monitor: None,
            fullscreen_stack: false,
        };

        // Height should be: 1080 - 40 = 1040
        assert_eq!(config.eve_height_adjusted(), 1040);
    }

    #[test]
    fn test_eve_height_adjusted_without_panel() {
        let config = Config {
            display_width: 1920,
            display_height: 1080,
            panel_height: 0,
            eve_width: 1000,
            eve_height: 1080,
            overlay_x: 10.0,
            overlay_y: 10.0,
            enable_mouse_buttons: true,
            forward_button: 276,
            backward_button: 275,
            enable_keyboard_buttons: false,
            forward_key: 15,
            backward_key: 15,
            show_overlay: true,
            mouse_device_name: None,
            mouse_device_path: None,
            minimize_inactive: false,
            keyboard_device_path: None,
            modifier_key: None,
            primary_character: None,
            primary_monitor: None,
            fullscreen_stack: false,
        };

        assert_eq!(config.eve_height_adjusted(), 1080);
    }

    #[test]
    fn test_config_serialization() {
        let config = Config {
            display_width: 7680,
            display_height: 2160,
            panel_height: 0,
            eve_width: 4147,
            eve_height: 2160,
            overlay_x: 10.0,
            overlay_y: 10.0,
            enable_mouse_buttons: true,
            forward_button: 276,
            backward_button: 275,
            enable_keyboard_buttons: false,
            forward_key: 15,
            backward_key: 15,
            show_overlay: true,
            mouse_device_name: None,
            mouse_device_path: None,
            minimize_inactive: false,
            keyboard_device_path: None,
            modifier_key: None,
            primary_character: None,
            primary_monitor: None,
            fullscreen_stack: false,
        };

        let toml_str = toml::to_string(&config).unwrap();
        let deserialized: Config = toml::from_str(&toml_str).unwrap();

        assert_eq!(deserialized.display_width, 7680);
        assert_eq!(deserialized.display_height, 2160);
        assert_eq!(deserialized.eve_width, 4147);
    }
}
