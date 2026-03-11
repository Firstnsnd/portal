#!/bin/bash
# Test script for Rust Terminus

echo "=== Rust Terminus Test Script ==="
echo ""

# Check if terminus is built
if [ ! -f "./target/release/terminus" ]; then
    echo "Building terminus..."
    cargo build --release
fi

echo "✓ Binary built: ./target/release/terminus"
echo ""

# Show binary info
echo "Binary info:"
ls -lh ./target/release/terminus
echo ""

# Show keyboard shortcuts
echo "=== Keyboard Shortcuts ==="
echo "Main:"
echo "  Ctrl+Q  - Quit"
echo "  Ctrl+S  - Toggle sidebar"
echo "  Ctrl+T  - New tab"
echo "  Ctrl+W  - Close tab"
echo "  Ctrl+L/H - Next/Prev tab"
echo "  Ctrl+\\  - Vertical split"
echo "  Ctrl+-  - Horizontal split"
echo "  Ctrl+?  - Toggle help"
echo ""
echo "Sidebar:"
echo "  J/K or ↑/↓ - Navigate"
echo "  Enter      - Open connection"
echo "  Esc        - Close sidebar"
echo ""

# Run the program
echo "=== Starting Terminus ==="
echo "Press Ctrl+Q to quit"
echo ""

./target/release/terminus
