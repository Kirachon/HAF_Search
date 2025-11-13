use std::path::Path;
use std::process::Command;

/// Opens the file location in the system's default file explorer
/// Cross-platform support for Windows, macOS, and Linux
pub fn open_file_location(file_path: &str) -> Result<(), String> {
    let path = Path::new(file_path);

    if !path.exists() {
        return Err(format!("File does not exist: {}", file_path));
    }

    // Get the parent directory
    let _dir = path
        .parent()
        .ok_or_else(|| format!("Could not get parent directory for: {}", file_path))?;

    #[cfg(target_os = "windows")]
    {
        // On Windows, use explorer.exe with /select flag to highlight the file
        let result = Command::new("explorer")
            .args(["/select,", &file_path])
            .spawn();

        match result {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Failed to open file location: {}", e)),
        }
    }

    #[cfg(target_os = "macos")]
    {
        // On macOS, use 'open' command with -R flag to reveal in Finder
        let result = Command::new("open").args(["-R", file_path]).spawn();

        match result {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Failed to open file location: {}", e)),
        }
    }

    #[cfg(target_os = "linux")]
    {
        // On Linux, try different file managers
        // First try xdg-open on the directory
        let dir_str = _dir.to_string_lossy();

        // Try xdg-open first (most common)
        if let Ok(_) = Command::new("xdg-open").arg(&*dir_str).spawn() {
            return Ok(());
        }

        // Try nautilus (GNOME)
        if let Ok(_) = Command::new("nautilus").arg(&*dir_str).spawn() {
            return Ok(());
        }

        // Try dolphin (KDE)
        if let Ok(_) = Command::new("dolphin").arg(&*dir_str).spawn() {
            return Ok(());
        }

        // Try thunar (XFCE)
        if let Ok(_) = Command::new("thunar").arg(&*dir_str).spawn() {
            return Ok(());
        }

        // Try nemo (Cinnamon)
        if let Ok(_) = Command::new("nemo").arg(&*dir_str).spawn() {
            return Ok(());
        }

        Err("Could not find a suitable file manager on Linux".to_string())
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        // Fallback for other platforms - just open the directory
        open::that(_dir).map_err(|e| format!("Failed to open directory: {}", e))
    }
}

/// Simple wrapper to open a directory (not selecting a specific file)
pub fn open_directory(dir_path: &str) -> Result<(), String> {
    open::that(dir_path).map_err(|e| format!("Failed to open directory: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nonexistent_file() {
        let result = open_file_location("/nonexistent/path/file.tif");
        assert!(result.is_err());
    }
}
