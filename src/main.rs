mod config;
mod cycle_state;
mod daemon;
mod overlay;
mod x11_manager;

use anyhow::Result;
use config::Config;
use cycle_state::CycleState;
use daemon::Daemon;
use overlay::run_overlay;
use std::env;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::sync::{Arc, Mutex};
use x11_manager::X11Manager;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let command = args.get(1).map(|s| s.as_str()).unwrap_or("overlay");

    let config = Config::load()?;
    let x11 = Arc::new(X11Manager::new()?);

    match command {
        "daemon" => {
            println!("Starting EVE Multibox daemon...");
            let mut daemon = Daemon::new(x11);
            daemon.run()?;
        }

        "overlay" => {
            println!("Starting EVE Multibox Overlay...");
            let state = Arc::new(Mutex::new(CycleState::new()));

            // Initialize windows
            if let Ok(windows) = x11.get_eve_windows() {
                state.lock().unwrap().update_windows(windows);
            }

            if let Err(e) = run_overlay(x11, state, config.overlay_x, config.overlay_y) {
                eprintln!("Overlay error: {}", e);
                std::process::exit(1);
            }
        }

        "stack" => {
            println!("Stacking EVE windows...");
            let windows = x11.get_eve_windows()?;

            println!(
                "Centering {} EVE clients ({}x{}) on {}x{} display",
                windows.len(),
                config.eve_width,
                config.eve_height_adjusted(),
                config.display_width,
                config.display_height
            );

            x11.stack_windows(
                &windows,
                config.eve_x(),
                config.eve_y(),
                config.eve_width,
                config.eve_height_adjusted(),
            )?;

            println!("✓ Stacked {} windows", windows.len());
        }

        "cycle-forward" | "forward" | "f" => {
            // Try daemon first
            if daemon::send_command("forward").is_ok() {
                return Ok(());
            }

            // Fallback to direct mode

            // Try to acquire lock, exit immediately if already running
            let lock_file = "/tmp/eve-multibox-cycle.lock";
            let mut file = match OpenOptions::new()
                .write(true)
                .create(true)
                .mode(0o644)
                .open(lock_file)
            {
                Ok(f) => f,
                Err(_) => return Ok(()), // Can't get lock, skip
            };

            // Try to lock (non-blocking)
            use std::os::unix::io::AsRawFd;
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return Ok(()); // Already running, skip this cycle
            }

            let mut state = CycleState::new();
            let windows = x11.get_eve_windows()?;

            if windows.is_empty() {
                return Ok(());
            }

            state.update_windows(windows);

            // Sync with current active window
            if let Ok(active) = x11.get_active_window() {
                state.sync_with_active(active);
            }

            state.cycle_forward(&x11)?;

            // Lock is automatically released when file is dropped
        }

        "cycle-backward" | "backward" | "b" => {
            // Try daemon first
            if daemon::send_command("backward").is_ok() {
                return Ok(());
            }

            // Fallback to direct mode

            // Try to acquire lock, exit immediately if already running
            let lock_file = "/tmp/eve-multibox-cycle.lock";
            let mut file = match OpenOptions::new()
                .write(true)
                .create(true)
                .mode(0o644)
                .open(lock_file)
            {
                Ok(f) => f,
                Err(_) => return Ok(()), // Can't get lock, skip
            };

            // Try to lock (non-blocking)
            use std::os::unix::io::AsRawFd;
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return Ok(()); // Already running, skip this cycle
            }

            let mut state = CycleState::new();
            let windows = x11.get_eve_windows()?;

            if windows.is_empty() {
                return Ok(());
            }

            state.update_windows(windows);

            // Sync with current active window
            if let Ok(active) = x11.get_active_window() {
                state.sync_with_active(active);
            }

            state.cycle_backward(&x11)?;

            // Lock is automatically released when file is dropped
        }

        "init-config" => {
            Config::save_default()?;
        }

        _ => {
            println!("EVE Multibox - Rust Edition");
            println!();
            println!("Usage:");
            println!("  eve-multibox daemon        - Start background daemon (recommended)");
            println!("  eve-multibox overlay       - Start the overlay");
            println!("  eve-multibox stack         - Stack all EVE windows");
            println!("  eve-multibox forward       - Cycle forward");
            println!("  eve-multibox backward      - Cycle backward");
            println!("  eve-multibox init-config   - Create default config.toml");
            println!();
            println!("Note: For best performance, start the daemon first:");
            println!("  eve-multibox daemon &");
        }
    }

    Ok(())
}
