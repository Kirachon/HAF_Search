# TiffLocator

A high-performance Rust desktop application for **interactive search** of TIFF files based on household IDs.

## Features

- 🔍 **Fast Directory Scanning**: Recursively scans local and network directories for `.tif` and `.tiff` files
- 🔎 **Interactive Search**: Search for individual household IDs with instant results
- 🎯 **Fuzzy Matching**: Intelligent fuzzy string matching between household IDs and TIFF filenames
- 💾 **Persistent Storage**: SQLite database stores scanned files and reference IDs permanently
- 📊 **Interactive GUI**: Built with egui/eframe for a responsive, native desktop experience
- 🚀 **Parallel Processing**: Uses rayon for multi-threaded scanning and searching operations
- 📂 **Cross-Platform File Opening**: Opens file locations in Windows Explorer, macOS Finder, or Linux file managers
- 📤 **CSV Export**: Export search results to CSV for further analysis
- ⚙️ **Adjustable Threshold**: Fine-tune match quality with similarity threshold slider (50%-100%)

## Installation

### Prerequisites

- Rust 1.70 or later
- Cargo (comes with Rust)

### Building from Source

```bash
# Clone the repository
git clone <repository-url>
cd tiff_locator

# Build in release mode for optimal performance
cargo build --release

# The executable will be in target/release/
# Windows: target/release/tiff_locator.exe
# Linux/macOS: target/release/tiff_locator
```

### Running

```bash
# Development mode
cargo run

# Or run the release binary directly
./target/release/tiff_locator  # Linux/macOS
.\target\release\tiff_locator.exe  # Windows
```

## Usage

### Workflow

#### Phase 1: One-Time Setup

1. **Select Folder**: Click "📁 Select Folder" to choose the directory containing TIFF files
   - Supports local directories and network paths (UNC paths on Windows)
   - The scanner will recursively search all subdirectories

2. **Scan Directory**: Click "🔍 Scan Directory" to index all TIFF files
   - Results are cached in `cache.db` SQLite database
   - Progress bar shows scanning status
   - **Only needs to be done once** (or when files change)

3. **Select CSV**: Click "📄 Select CSV" to choose your household ID reference file
   - CSV must contain a column named `hh_id`
   - Example format:
     ```csv
     hh_id
     HH001
     HH002
     ABC123
     ```

4. **Load Reference IDs**: Click "📥 Load Reference IDs" to import household IDs into database
   - IDs are permanently stored in the database
   - Duplicate IDs are automatically skipped
   - **Only needs to be done once** (or when updating the reference list)

#### Phase 2: Interactive Search (Repeated)

5. **Adjust Threshold** (Optional): Use the similarity slider to set match quality (default: 70%)
   - Higher values = stricter matching
   - Lower values = more permissive matching

6. **Search for Household ID**:
   - Type or paste a household ID in the search box
   - Click "🔍 Search" to find matching TIFF files
   - Results appear instantly with similarity scores

7. **View Results**: Browse the search results table showing:
   - File Name
   - Similarity score (percentage)
   - "Open Location" button for each match

8. **Open File Location**: Click "📂 Open Location" to open the file in your system's file explorer
   - Windows: Opens Explorer with file selected
   - macOS: Opens Finder with file revealed
   - Linux: Opens default file manager

9. **Search Another ID**: Clear the search box or enter a new ID and click Search again
   - Results update instantly
   - No need to rescan or reload

10. **Export Results**: Click "📤 Export Results" to save current search results to CSV

### Advanced Features

#### Clear Cache
- Click "🗑 Clear Cache" to remove all cached scan data
- Use this when directory contents have changed significantly
- Forces fresh scan on next operation

#### Updating Reference IDs
- To add new household IDs, select an updated CSV and click "Load Reference IDs" again
- Duplicate IDs are automatically skipped
- Or use "Clear Cache" to start completely fresh

## Technical Details

### Architecture

The application is organized into modular components:

- **`database.rs`**: SQLite operations for persistent storage
  - `files` table: Stores scanned TIFF file metadata
  - `reference_ids` table: Stores household IDs from CSV

- **`scanner.rs`**: Directory scanning with parallel processing
  - Uses `walkdir` for recursive traversal
  - Supports network paths and symbolic links
  - Parallel file filtering with `rayon`

- **`reference_loader.rs`**: CSV import for household IDs
  - Parses CSV files and extracts `hh_id` column
  - Stores IDs permanently in database
  - Prevents duplicate entries

- **`searcher.rs`**: Single-ID fuzzy search engine
  - Uses `fuzzy-matcher` (SkimMatcherV2) for similarity scoring
  - Intelligent ID extraction from filenames
  - Parallel searching with `rayon` for speed

- **`opener.rs`**: Cross-platform file location opener
  - Windows: `explorer.exe /select`
  - macOS: `open -R`
  - Linux: Tries xdg-open, nautilus, dolphin, thunar, nemo

- **`gui.rs`**: egui-based graphical interface
  - Responsive design with progress indicators
  - Real-time status updates via message channels
  - Scrollable results table with pagination

### Database Schema

```sql
CREATE TABLE files (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_path TEXT NOT NULL UNIQUE,
    file_name TEXT NOT NULL,
    scan_date TEXT NOT NULL
);

CREATE TABLE reference_ids (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    hh_id TEXT NOT NULL UNIQUE,
    import_date TEXT NOT NULL
);
```

### Performance

- **Large Directories**: Efficiently handles 10,000+ files
- **Parallel Processing**: Utilizes all CPU cores for scanning and searching
- **Persistent Storage**: Scanned files and reference IDs stored permanently
- **Instant Search**: Single-ID searches complete in <1 second
- **Memory Efficient**: Only stores current search results in memory
- **60 FPS GUI**: Smooth, responsive interface with conditional repainting

### Network Path Support

- **Windows UNC Paths**: `\\server\share\folder`
- **Mapped Network Drives**: `Z:\folder`
- **Linux/macOS Mounted Shares**: `/mnt/share/folder`

## Dependencies

- `walkdir`: Directory traversal
- `csv`: CSV file parsing
- `fuzzy-matcher`: String similarity matching
- `rayon`: Parallel processing
- `rfd`: Native file dialogs
- `eframe` + `egui`: GUI framework
- `open`: Cross-platform file opening
- `rusqlite`: SQLite database
- `chrono`: Timestamp handling

## Troubleshooting

### "No files found in database"
- Make sure you've clicked "🔍 Scan Directory" before matching
- Check that the selected folder contains `.tif` or `.tiff` files

### "CSV must contain 'hh_id' column"
- Ensure your CSV has a header row with a column named exactly `hh_id` (case-insensitive)

### Network path scanning is slow
- Network latency can affect scan speed
- Consider copying files locally for faster processing
- Results are cached, so subsequent operations are instant

### File location won't open
- Ensure the file still exists at the cached path
- On Linux, make sure you have a file manager installed (xdg-open, nautilus, etc.)

## License

[Your License Here]

## Contributing

Contributions are welcome! Please feel free to submit pull requests or open issues.

