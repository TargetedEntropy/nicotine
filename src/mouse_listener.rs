use crate::config::Config;
use crate::cycle_state::CycleState;
use crate::window_manager::WindowManager;
use anyhow::{Context, Result};
use evdev::{Device, InputEventKind, Key};
use std::path::Path;
use std::sync::{Arc, Mutex};

pub struct MouseListener {
    config: Config,
}

impl MouseListener {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Find mouse device by looking for devices with BTN_SIDE or BTN_EXTRA capabilities
    fn find_mouse_device() -> Result<Device> {
        let devices_path = Path::new("/dev/input");

        for entry in std::fs::read_dir(devices_path)? {
            let entry = entry?;
            let path = entry.path();

            if let Some(filename) = path.file_name() {
                if let Some(name) = filename.to_str() {
                    if name.starts_with("event") {
                        if let Ok(device) = Device::open(&path) {
                            // Check if device has mouse side buttons
                            if device.supported_keys().map_or(false, |keys| {
                                keys.contains(Key::BTN_SIDE) || keys.contains(Key::BTN_EXTRA)
                            }) {
                                println!(
                                    "Found mouse device: {} ({})",
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

        anyhow::bail!("No mouse device with side buttons found in /dev/input")
    }

    /// Run the mouse event listener in a background thread
    pub fn spawn(
        &self,
        wm: Arc<dyn WindowManager>,
        state: Arc<Mutex<CycleState>>,
    ) -> Result<std::thread::JoinHandle<()>> {
        if !self.config.enable_mouse_buttons {
            anyhow::bail!("Mouse buttons are disabled in config");
        }

        let forward_button = self.config.forward_button;
        let backward_button = self.config.backward_button;

        let handle = std::thread::spawn(move || {
            match Self::run_listener(wm, state, forward_button, backward_button) {
                Ok(_) => println!("Mouse listener stopped"),
                Err(e) => eprintln!("Mouse listener error: {}", e),
            }
        });

        Ok(handle)
    }

    fn run_listener(
        wm: Arc<dyn WindowManager>,
        state: Arc<Mutex<CycleState>>,
        forward_button: u16,
        backward_button: u16,
    ) -> Result<()> {
        let mut device = Self::find_mouse_device().context(
            "Failed to find mouse device. Make sure you have permission to read /dev/input/event*",
        )?;

        // DON'T grab the device - we only want to passively listen to events
        // Grabbing would prevent normal mouse usage!

        println!(
            "Listening for mouse buttons: forward={}, backward={}",
            forward_button, backward_button
        );

        loop {
            for event in device.fetch_events()? {
                if let InputEventKind::Key(key) = event.kind() {
                    let code = key.code();

                    // Only handle button press (value 1), ignore release (value 0)
                    if event.value() == 1 {
                        if code == forward_button {
                            println!("Forward button pressed");
                            if let Err(e) = Self::cycle_forward(&wm, &state) {
                                eprintln!("Failed to cycle forward: {}", e);
                            }
                        } else if code == backward_button {
                            println!("Backward button pressed");
                            if let Err(e) = Self::cycle_backward(&wm, &state) {
                                eprintln!("Failed to cycle backward: {}", e);
                            }
                        }
                    }
                }
            }
        }
    }

    fn cycle_forward(wm: &Arc<dyn WindowManager>, state: &Arc<Mutex<CycleState>>) -> Result<()> {
        let mut state = state.lock().unwrap();

        // Sync with active window first
        if let Ok(active) = wm.get_active_window() {
            state.sync_with_active(active);
        }

        state.cycle_forward(&**wm)?;
        Ok(())
    }

    fn cycle_backward(wm: &Arc<dyn WindowManager>, state: &Arc<Mutex<CycleState>>) -> Result<()> {
        let mut state = state.lock().unwrap();

        // Sync with active window first
        if let Ok(active) = wm.get_active_window() {
            state.sync_with_active(active);
        }

        state.cycle_backward(&**wm)?;
        Ok(())
    }
}
