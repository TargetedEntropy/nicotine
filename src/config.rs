use serde::{Deserialize, Serialize};
use anyhow::{Context, Result};
use std::fs;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub display_width: u32,
    pub display_height: u32,
    pub panel_height: u32,
    pub eve_width: u32,
    pub eve_height: u32,
    pub overlay_x: f32,
    pub overlay_y: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            display_width: 7680,
            display_height: 2160,
            panel_height: 44,
            eve_width: 4150,
            eve_height: 2116,
            overlay_x: 3715.0,
            overlay_y: 10.0,
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        // Try to load from config.toml first
        if let Ok(contents) = fs::read_to_string("config.toml") {
            return toml::from_str(&contents).context("Failed to parse config.toml");
        }

        // Fall back to default
        println!("No config.toml found, using defaults");
        Ok(Self::default())
    }

    pub fn save_default() -> Result<()> {
        let config = Self::default();
        let contents = toml::to_string_pretty(&config)?;
        fs::write("config.toml", contents)?;
        println!("Created default config.toml");
        Ok(())
    }

    pub fn eve_x(&self) -> i32 {
        ((self.display_width - self.eve_width) / 2) as i32
    }

    pub fn eve_y(&self) -> i32 {
        0
    }

    pub fn eve_height_adjusted(&self) -> u32 {
        self.display_height - self.panel_height
    }
}
