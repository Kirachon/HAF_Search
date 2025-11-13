@echo off
echo ========================================
echo TiffLocator Build Script (Windows)
echo ========================================
echo.

echo Checking Rust installation...
cargo --version >nul 2>&1
if errorlevel 1 (
    echo ERROR: Rust/Cargo not found!
    echo Please install Rust from: https://rustup.rs/
    pause
    exit /b 1
)

echo.
echo Building TiffLocator in release mode...
echo This may take a few minutes on first build...
echo.

cargo build --release

if errorlevel 1 (
    echo.
    echo ========================================
    echo BUILD FAILED!
    echo ========================================
    pause
    exit /b 1
)

echo.
echo ========================================
echo BUILD SUCCESSFUL!
echo ========================================
echo.
echo Executable location:
echo   target\release\tiff_locator.exe
echo.
echo To run the application:
echo   .\target\release\tiff_locator.exe
echo.
echo Or double-click: target\release\tiff_locator.exe
echo ========================================
pause

