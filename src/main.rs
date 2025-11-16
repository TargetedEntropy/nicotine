mod config;
mod cycle_state;
mod daemon;
mod mouse_listener;
mod overlay;
mod version_check;
mod wayland_backends;
mod window_manager;
mod x11_manager;

use anyhow::Result;
use config::Config;
use cycle_state::CycleState;
use daemon::Daemon;
use daemonize::Daemonize;
#[allow(deprecated)]
use nix::fcntl::{flock, FlockArg};
use overlay::run_overlay;
use std::env;
use std::fs::OpenOptions;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::sync::{Arc, Mutex};
use wayland_backends::{HyprlandManager, KWinManager, SwayManager};
use window_manager::{
    detect_display_server, detect_wayland_compositor, DisplayServer, WaylandCompositor,
    WindowManager,
};
use x11_manager::X11Manager;

fn create_window_manager() -> Result<Arc<dyn WindowManager>> {
    let display_server = detect_display_server();

    match display_server {
        DisplayServer::X11 => {
            println!("Detected X11 display server");
            Ok(Arc::new(X11Manager::new()?))
        }
        DisplayServer::Wayland => {
            let compositor = detect_wayland_compositor();
            println!(
                "Detected Wayland display server with {:?} compositor",
                compositor
            );

            match compositor {
                WaylandCompositor::Kde => {
                    println!("Using KDE/KWin backend");
                    Ok(Arc::new(KWinManager::new()?))
                }
                WaylandCompositor::Sway => {
                    println!("Using Sway backend");
                    Ok(Arc::new(SwayManager::new()?))
                }
                WaylandCompositor::Hyprland => {
                    println!("Using Hyprland backend");
                    Ok(Arc::new(HyprlandManager::new()?))
                }
                WaylandCompositor::Gnome => {
                    anyhow::bail!("GNOME Shell is not yet supported due to restrictive window management APIs")
                }
                WaylandCompositor::Other => {
                    anyhow::bail!(
                        "Unknown Wayland compositor. Supported: KDE Plasma, Sway, Hyprland"
                    )
                }
            }
        }
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let command = args.get(1).map(|s| s.as_str()).unwrap_or("");

    let config = Config::load()?;
    let wm = create_window_manager()?;

    match command {
        "start" => {
            println!("Starting Nicotine 🚬");

            // Check for updates (non-blocking, silent on errors)
            if let Ok(Some((new_version, url))) = version_check::check_for_updates() {
                version_check::print_update_notification(&new_version, &url);
            }

            // Daemonize the process (safe Rust wrapper)
            let daemonize = Daemonize::new().working_directory("/tmp").umask(0o027);

            match daemonize.start() {
                Ok(_) => {
                    // We're now in the daemon process
                    // Start daemon in background thread
                    let wm_daemon = Arc::clone(&wm);
                    let config_daemon = config.clone();
                    let daemon_thread = std::thread::spawn(move || {
                        let mut daemon = Daemon::new(wm_daemon, config_daemon);
                        if let Err(e) = daemon.run() {
                            eprintln!("Daemon error: {}", e);
                        }
                    });

                    // Wait a bit for daemon to initialize
                    std::thread::sleep(std::time::Duration::from_millis(100));

                    if config.show_overlay {
                        // Run overlay in main thread
                        let state = Arc::new(Mutex::new(CycleState::new()));
                        if let Ok(windows) = wm.get_eve_windows() {
                            state.lock().unwrap().update_windows(windows);
                        }

                        if let Err(e) =
                            run_overlay(wm, state, config.overlay_x, config.overlay_y, config)
                        {
                            eprintln!("Overlay error: {}", e);
                            std::process::exit(1);
                        }
                    } else {
                        // No overlay - just keep daemon running
                        println!("Overlay disabled - daemon running in background");
                        daemon_thread.join().unwrap();
                    }
                }
                Err(e) => {
                    eprintln!("Failed to daemonize: {}", e);
                    std::process::exit(1);
                }
            }
        }

        "daemon" => {
            println!("Starting EVE Multibox daemon...");
            let mut daemon = Daemon::new(wm, config);
            daemon.run()?;
        }

        "overlay" => {
            println!("Starting EVE Multibox Overlay...");
            let state = Arc::new(Mutex::new(CycleState::new()));

            // Initialize windows
            if let Ok(windows) = wm.get_eve_windows() {
                state.lock().unwrap().update_windows(windows);
            }

            if let Err(e) = run_overlay(wm, state, config.overlay_x, config.overlay_y, config) {
                eprintln!("Overlay error: {}", e);
                std::process::exit(1);
            }
        }

        "stack" => {
            println!("Stacking EVE windows...");
            let windows = wm.get_eve_windows()?;

            println!(
                "Centering {} EVE clients ({}x{}) on {}x{} display",
                windows.len(),
                config.eve_width,
                config.eve_height_adjusted(),
                config.display_width,
                config.display_height
            );

            wm.stack_windows(&windows, &config)?;

            println!("✓ Stacked {} windows", windows.len());
        }

        "cycle-forward" | "forward" | "f" => {
            // Try daemon first
            if daemon::send_command("forward").is_ok() {
                return Ok(());
            }

            // Fallback to direct mode

            // Try to acquire lock, exit immediately if already running
            let lock_file = "/tmp/nicotine-cycle.lock";
            let file = match OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o644)
                .open(lock_file)
            {
                Ok(f) => f,
                Err(_) => return Ok(()), // Can't get lock, skip
            };

            // Try to lock (non-blocking)
            #[allow(deprecated)]
            if flock(file.as_raw_fd(), FlockArg::LockExclusiveNonblock).is_err() {
                return Ok(()); // Already running, skip this cycle
            }

            let mut state = CycleState::new();
            let windows = wm.get_eve_windows()?;

            if windows.is_empty() {
                return Ok(());
            }

            state.update_windows(windows);

            // Sync with current active window
            if let Ok(active) = wm.get_active_window() {
                state.sync_with_active(active);
            }

            state.cycle_forward(&*wm)?;

            // Lock is automatically released when file is dropped
        }

        "cycle-backward" | "backward" | "b" => {
            // Try daemon first
            if daemon::send_command("backward").is_ok() {
                return Ok(());
            }

            // Fallback to direct mode

            // Try to acquire lock, exit immediately if already running
            let lock_file = "/tmp/nicotine-cycle.lock";
            let file = match OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o644)
                .open(lock_file)
            {
                Ok(f) => f,
                Err(_) => return Ok(()), // Can't get lock, skip
            };

            // Try to lock (non-blocking)
            #[allow(deprecated)]
            if flock(file.as_raw_fd(), FlockArg::LockExclusiveNonblock).is_err() {
                return Ok(()); // Already running, skip this cycle
            }

            let mut state = CycleState::new();
            let windows = wm.get_eve_windows()?;

            if windows.is_empty() {
                return Ok(());
            }

            state.update_windows(windows);

            // Sync with current active window
            if let Ok(active) = wm.get_active_window() {
                state.sync_with_active(active);
            }

            state.cycle_backward(&*wm)?;

            // Lock is automatically released when file is dropped
        }

        "stop" => {
            println!("Stopping Nicotine...");

            // Kill all nicotine processes
            let _ = std::process::Command::new("pkill")
                .arg("-9")
                .arg("nicotine")
                .output();

            println!("✓ Nicotine stopped");

            // Clean up socket and lock files
            let _ = std::fs::remove_file("/tmp/nicotine.sock");
            let _ = std::fs::remove_file("/tmp/nicotine-cycle.lock");
        }

        "init-config" => {
            Config::save_default()?;
        }

        _ => {
            println!();
            println!("🚬 N I C O T I N E 🚬");
            println!();
            println!("Questions or suggestions?");
            println!("Reach out to isomerc on Discord or open a Github issue");
            println!();
            println!("Usage:");
            println!("  nicotine start         - Start everything (daemon + overlay)");
            println!("  nicotine stop          - Stop all Nicotine processes");
            println!("  nicotine stack         - Stack all EVE windows");
            println!("  nicotine forward       - Cycle forward");
            println!("  nicotine backward      - Cycle backward");
            println!("  nicotine init-config   - Create default config.toml");
            println!();
            println!("Advanced:");
            println!("  nicotine daemon        - Start daemon only");
            println!("  nicotine overlay       - Start overlay only");
            println!();
            println!("Quick start:");
            println!("  nicotine start         # Starts in background automatically");
        }
    }

    Ok(())
}
