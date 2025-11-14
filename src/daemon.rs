use crate::config::Config;
use crate::cycle_state::CycleState;
use crate::mouse_listener::MouseListener;
use crate::window_manager::WindowManager;
use anyhow::Result;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::{Arc, Mutex};

const SOCKET_PATH: &str = "/tmp/nicotine.sock";

#[derive(Debug)]
pub enum Command {
    Forward,
    Backward,
    Refresh,
    Quit,
}

impl Command {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim() {
            "forward" => Some(Command::Forward),
            "backward" => Some(Command::Backward),
            "refresh" => Some(Command::Refresh),
            "quit" => Some(Command::Quit),
            _ => None,
        }
    }
}

pub struct Daemon {
    wm: Arc<dyn WindowManager>,
    state: Arc<Mutex<CycleState>>,
    config: Config,
}

impl Daemon {
    pub fn new(wm: Arc<dyn WindowManager>, config: Config) -> Self {
        let state = Arc::new(Mutex::new(CycleState::new()));

        // Initialize windows
        if let Ok(windows) = wm.get_eve_windows() {
            state.lock().unwrap().update_windows(windows);
        }

        Self { wm, state, config }
    }

    pub fn run(&mut self) -> Result<()> {
        // Remove old socket if it exists
        let _ = fs::remove_file(SOCKET_PATH);

        let listener = UnixListener::bind(SOCKET_PATH)?;
        println!("EVE Multibox daemon listening on {}", SOCKET_PATH);

        // Start mouse event listener if enabled
        if self.config.enable_mouse_buttons {
            let mouse_listener = MouseListener::new(self.config.clone());
            let wm_clone = Arc::clone(&self.wm);
            let state_clone = Arc::clone(&self.state);

            match mouse_listener.spawn(wm_clone, state_clone) {
                Ok(_) => println!("Mouse button listener started"),
                Err(e) => {
                    eprintln!("Warning: Could not start mouse listener: {}", e);
                    eprintln!(
                        "Mouse buttons will not work. You can disable this warning by setting"
                    );
                    eprintln!("'enable_mouse_buttons = false' in ~/.config/nicotine/config.toml");
                }
            }
        }

        // Refresh window list periodically in background
        let wm_clone = Arc::clone(&self.wm);
        let state_clone = Arc::clone(&self.state);
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_millis(500));
            if let Ok(windows) = wm_clone.get_eve_windows() {
                state_clone.lock().unwrap().update_windows(windows);
            }
        });

        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    if let Err(e) = self.handle_client(stream) {
                        eprintln!("Error handling client: {}", e);
                    }
                }
                Err(e) => {
                    eprintln!("Connection error: {}", e);
                }
            }
        }

        Ok(())
    }

    fn handle_client(&mut self, stream: UnixStream) -> Result<()> {
        let mut reader = BufReader::new(&stream);
        let mut line = String::new();
        reader.read_line(&mut line)?;

        if let Some(command) = Command::from_str(&line) {
            match command {
                Command::Forward => {
                    let mut state = self.state.lock().unwrap();

                    // Sync with active window first
                    if let Ok(active) = self.wm.get_active_window() {
                        state.sync_with_active(active);
                    }

                    state.cycle_forward(&*self.wm)?;
                }
                Command::Backward => {
                    let mut state = self.state.lock().unwrap();

                    // Sync with active window first
                    if let Ok(active) = self.wm.get_active_window() {
                        state.sync_with_active(active);
                    }

                    state.cycle_backward(&*self.wm)?;
                }
                Command::Refresh => {
                    let windows = self.wm.get_eve_windows()?;
                    self.state.lock().unwrap().update_windows(windows);
                }
                Command::Quit => {
                    std::process::exit(0);
                }
            }
        }

        Ok(())
    }
}

pub fn send_command(command: &str) -> Result<()> {
    if !Path::new(SOCKET_PATH).exists() {
        anyhow::bail!("Daemon not running. Start with: eve-multibox daemon");
    }

    let mut stream = UnixStream::connect(SOCKET_PATH)?;
    writeln!(stream, "{}", command)?;
    stream.flush()?;
    Ok(())
}
