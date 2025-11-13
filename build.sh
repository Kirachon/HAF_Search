#!/bin/bash

echo "========================================"
echo "TiffLocator Build Script (Linux/macOS)"
echo "========================================"
echo ""

echo "Checking Rust installation..."
if ! command -v cargo &> /dev/null; then
    echo "ERROR: Rust/Cargo not found!"
    echo "Please install Rust from: https://rustup.rs/"
    exit 1
fi

echo ""
echo "Building TiffLocator in release mode..."
echo "This may take a few minutes on first build..."
echo ""

cargo build --release

if [ $? -ne 0 ]; then
    echo ""
    echo "========================================"
    echo "BUILD FAILED!"
    echo "========================================"
    exit 1
fi

echo ""
echo "========================================"
echo "BUILD SUCCESSFUL!"
echo "========================================"
echo ""
echo "Executable location:"
echo "  target/release/tiff_locator"
echo ""
echo "To run the application:"
echo "  ./target/release/tiff_locator"
echo ""
echo "========================================"

