use crate::config::Config;
use crate::cycle_state::CycleState;
use crate::window_manager::WindowManager;
use anyhow::{Context, Result};
use evdev::{Device, InputEventKind, Key};
use std::path::Path;
use std::sync::{Arc, Mutex};

pub struct KeyboardListener {
    config: Config,
}

impl KeyboardListener {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Find keyboard device by looking for devices with standard keyboard keys
    fn find_keyboard_device(configured_path: Option<&str>) -> Result<Device> {
        if let Some(path_str) = configured_path {
            let path = Path::new(path_str);
            match Device::open(path) {
                Ok(device) => {
                    println!(
                        "Using configured keyboard device {} ({})",
                        device.name().unwrap_or("Unknown"),
                        path.display()
                    );
                    return Ok(device);
                }
                Err(e) => {
                    eprintln!(
                        "Warning: Failed to open configured keyboard device '{}': {}",
                        path_str, e
                    );
                    eprintln!("Falling back to automatic device detection...");
                }
            }
        }

        let devices_path = Path::new("/dev/input");
        for entry in std::fs::read_dir(devices_path)? {
            let entry = entry?;
            let path = entry.path();

            if let Some(filename) = path.file_name() {
                if let Some(name) = filename.to_str() {
                    if name.starts_with("event") {
                        if let Ok(device) = Device::open(&path) {
                            if device.supported_keys().is_some_and(|keys| {
                                keys.contains(Key::KEY_TAB)
                                    || keys.contains(Key::KEY_LEFTSHIFT)
                                    || keys.contains(Key::KEY_Z)
                            }) {
                                println!(
                                    "Found keyboard device: {} ({})",
                                    device.name().unwrap_or("Unknown"),
                                    path.display()
                                );
                                return Ok(device);
                            }
                        }
                    }
                }
            }
        }

        anyhow::bail!("No keyboard device found in /dev/input")
    }

    /// Run the keyboard event listener in a background thread
    pub fn spawn(
        &self,
        wm: Arc<dyn WindowManager>,
        state: Arc<Mutex<CycleState>>,
    ) -> Result<std::thread::JoinHandle<()>> {
        if !self.config.enable_keyboard_buttons {
            anyhow::bail!("Keyboard buttons are disabled in config");
        }

        let forward_key = self.config.forward_key;
        let backward_key = self.config.backward_key;
        let modifier_key = self.config.modifier_key;
        let keyboard_device_path = self.config.keyboard_device_path.clone();
        let minimize_inactive = self.config.minimize_inactive;
        let primary_character = self.config.primary_character.clone();

        let handle = std::thread::spawn(move || {
            match Self::run_listener(
                wm,
                state,
                forward_key,
                backward_key,
                modifier_key,
                keyboard_device_path,
                minimize_inactive,
                primary_character,
            ) {
                Ok(_) => println!("Keyboard listener stopped"),
                Err(e) => println!("Keyboard listener error: {}", e),
            }
        });

        Ok(handle)
    }

    #[allow(clippy::too_many_arguments)]
    fn run_listener(
        wm: Arc<dyn WindowManager>,
        state: Arc<Mutex<CycleState>>,
        forward_key: u16,
        backward_key: u16,
        modifier_key: Option<u16>,
        keyboard_device_path: Option<String>,
        minimize_inactive: bool,
        primary_character: Option<String>,
    ) -> Result<()> {
        let mut device = Self::find_keyboard_device(keyboard_device_path.as_deref()).context(
            "Failed to find keyboard device. Make sure you have permission to read /dev/input/event*",
        )?;

        // DON'T grab the device - we only want to passively listen to events
        // Grabbing would prevent normal keyboard usage!

        println!(
            "Listening for keyboard keys: forward={} backward={}",
            forward_key, backward_key
        );
        let mut modifier_pressed = false;

        loop {
            for event in device.fetch_events()? {
                if let InputEventKind::Key(key) = event.kind() {
                    let code = key.code();
                    //let mut modifier_pressed = false;
                    if let Some(mod_key) = modifier_key {
                        if code == mod_key {
                            println!("Modifier Pressed");
                            modifier_pressed = event.value() != 0;
                        }
                    }
                    //print(code);
                    if event.value() != 0 {
                        // Have to check modifier + backwards first, otherwise if backward == forward it ignores the modifier flag
                        if code == backward_key && modifier_pressed {
                            println!("Backward + Modifier button pressed");
                            if let Err(e) = Self::cycle_backward(&wm, &state, minimize_inactive, primary_character.as_deref()) {
                                eprintln!("Failed to cycle backward: {}", e);
                            }
                        } else if code == forward_key {
                            println!("Forward button pressed");
                            if let Err(e) = Self::cycle_forward(&wm, &state, minimize_inactive, primary_character.as_deref()) {
                                eprintln!("Failed to cycle forward: {}", e);
                            }
                        } else if code == backward_key {
                            println!("Backward button pressed");
                            if let Err(e) = Self::cycle_backward(&wm, &state, minimize_inactive, primary_character.as_deref()) {
                                eprintln!("Failed to cycle backward: {}", e);
                            }
                        }
                    }
                }
            }
        }
    }

    fn cycle_forward(
        wm: &Arc<dyn WindowManager>,
        state: &Arc<Mutex<CycleState>>,
        minimize_inactive: bool,
        skip_character: Option<&str>,
    ) -> Result<()> {
        let mut state = state.lock().unwrap();

        // Sync with active window first
        if let Ok(active) = wm.get_active_window() {
            state.sync_with_active(active);
        }

        state.cycle_forward(&**wm, minimize_inactive, skip_character)?;
        Ok(())
    }

    fn cycle_backward(
        wm: &Arc<dyn WindowManager>,
        state: &Arc<Mutex<CycleState>>,
        minimize_inactive: bool,
        skip_character: Option<&str>,
    ) -> Result<()> {
        let mut state = state.lock().unwrap();

        // Sync with active window first
        if let Ok(active) = wm.get_active_window() {
            state.sync_with_active(active);
        }

        state.cycle_backward(&**wm, minimize_inactive, skip_character)?;
        Ok(())
    }
}
