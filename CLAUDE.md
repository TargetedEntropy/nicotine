# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build and Test Commands

```bash
cargo build --release          # Build release binary (target/release/nicotine)
cargo test                     # Run all tests
cargo test cycle_state         # Run tests for specific module
cargo fmt                      # Format code
cargo clippy -- -D warnings    # Lint with strict warnings
./install-local.sh             # Build and install to ~/.local/bin
```

## Architecture Overview

Nicotine is a Rust-based EVE Online multiboxing tool for Linux. It detects EVE client windows (titles starting with "EVE - ") and provides instant cycling between them.

### Core Design: Daemon + IPC

The application uses a daemon architecture for low-latency window switching:

1. **`nicotine start`** daemonizes and spawns:
   - Unix socket listener at `/tmp/nicotine.sock` (~2ms command latency)
   - Mouse/keyboard evdev listeners (direct `/dev/input` reading)
   - Background thread refreshing window list every 500ms
   - Overlay UI thread (if enabled)

2. **Commands** (`forward`, `backward`, `1-N`) connect to the socket and send commands to the daemon

3. **State sync**: Current window index stored in `/tmp/nicotine-index` for overlay/daemon coordination

### Key Modules

- **`main.rs`**: CLI dispatcher, daemon spawning, process management
- **`daemon.rs`**: Unix socket IPC server, command handling, spawns input listeners
- **`cycle_state.rs`**: Window list state machine with forward/backward/targeted cycling
- **`window_manager.rs`**: Trait defining window operations (`get_eve_windows`, `activate_window`, `stack_windows`)
- **`x11_manager.rs`**: X11 implementation using x11rb
- **`wayland_backends.rs`**: KDE (wmctrl), Sway (swaymsg), Hyprland (hyprctl) implementations
- **`mouse_listener.rs` / `keyboard_listener.rs`**: Direct evdev input reading
- **`overlay.rs`**: egui-based always-on-top UI
- **`config.rs`**: TOML config at `~/.config/nicotine/config.toml`

### Display Server Detection

At runtime, checks environment variables (`XDG_SESSION_TYPE`, `WAYLAND_DISPLAY`, `XDG_CURRENT_DESKTOP`) to select the appropriate `WindowManager` implementation.

### Input Handling Pattern

Mouse/keyboard bypass X11/Wayland input APIs by reading directly from `/dev/input/eventX` devices (requires `input` group membership). This provides universal support across all display servers without window grabs.

### Configuration

Auto-generated on first run with display detection (tries xrandr, swaymsg, hyprctl, wlr-randr in order). Config file: `~/.config/nicotine/config.toml`. Optional `~/.config/nicotine/characters.txt` maps `nicotine 1/2/3` to specific character names.

## Dependencies

- **wmctrl**: Required for X11 and KDE Plasma Wayland
- **evtest**: Useful for finding mouse/keyboard device paths and button codes
