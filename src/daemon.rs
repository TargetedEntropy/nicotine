use crate::cycle_state::CycleState;
use crate::x11_manager::X11Manager;
use anyhow::Result;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::path::Path;

const SOCKET_PATH: &str = "/tmp/eve-multibox.sock";

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
    x11: Arc<X11Manager>,
    state: Arc<Mutex<CycleState>>,
}

impl Daemon {
    pub fn new(x11: Arc<X11Manager>) -> Self {
        let state = Arc::new(Mutex::new(CycleState::new()));

        // Initialize windows
        if let Ok(windows) = x11.get_eve_windows() {
            state.lock().unwrap().update_windows(windows);
        }

        Self { x11, state }
    }

    pub fn run(&mut self) -> Result<()> {
        // Remove old socket if it exists
        let _ = fs::remove_file(SOCKET_PATH);

        let listener = UnixListener::bind(SOCKET_PATH)?;
        println!("EVE Multibox daemon listening on {}", SOCKET_PATH);

        // Refresh window list periodically in background
        let x11_clone = Arc::clone(&self.x11);
        let state_clone = Arc::clone(&self.state);
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_millis(500));
                if let Ok(windows) = x11_clone.get_eve_windows() {
                    state_clone.lock().unwrap().update_windows(windows);
                }
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
                    if let Ok(active) = self.x11.get_active_window() {
                        state.sync_with_active(active);
                    }

                    state.cycle_forward(&self.x11)?;
                }
                Command::Backward => {
                    let mut state = self.state.lock().unwrap();

                    // Sync with active window first
                    if let Ok(active) = self.x11.get_active_window() {
                        state.sync_with_active(active);
                    }

                    state.cycle_backward(&self.x11)?;
                }
                Command::Refresh => {
                    let windows = self.x11.get_eve_windows()?;
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
