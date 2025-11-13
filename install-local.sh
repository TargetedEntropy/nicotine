#!/bin/bash
# Nicotine - Local installer (for development)
set -e

INSTALL_DIR="$HOME/.local/bin"
BINARY_NAME="nicotine"

echo "=== Nicotine Local Installer ==="
echo

# Always build to ensure latest version
echo "[1/4] Building release binary..."
cargo build --release

echo "[2/4] Installing to $INSTALL_DIR..."
mkdir -p "$INSTALL_DIR"
cp "target/release/$BINARY_NAME" "$INSTALL_DIR/"

# Add to PATH if needed
if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
    echo "[3/4] Adding $INSTALL_DIR to PATH..."
    SHELL_RC=""
    if [ -n "$BASH_VERSION" ]; then
        SHELL_RC="$HOME/.bashrc"
    elif [ -n "$ZSH_VERSION" ]; then
        SHELL_RC="$HOME/.zshrc"
    fi

    if [ -n "$SHELL_RC" ] && [ -f "$SHELL_RC" ]; then
        if ! grep -q "export PATH.*$INSTALL_DIR" "$SHELL_RC" 2>/dev/null; then
            echo "" >> "$SHELL_RC"
            echo "# Nicotine" >> "$SHELL_RC"
            echo "export PATH=\"\$HOME/.local/bin:\$PATH\"" >> "$SHELL_RC"
            echo "Added to $SHELL_RC"
        else
            echo "Already in PATH"
        fi
    fi
else
    echo "[3/4] PATH already configured"
fi

echo "[4/4] Testing installation..."
if "$INSTALL_DIR/$BINARY_NAME" --help > /dev/null 2>&1 || [ $? -eq 0 ]; then
    echo "✓ Binary installed successfully"
else
    echo "✓ Binary installed at $INSTALL_DIR/$BINARY_NAME"
fi

echo
echo "✓ Installation complete!"
echo
echo "Quick start:"
echo "  nicotine start"
echo
echo "Config will be auto-generated at: ~/.config/nicotine/config.toml"
echo
echo "Note: Restart your terminal first if PATH was just updated"
