# TiffLocator - Quick Start Guide

## 🚀 Getting Started in 5 Minutes

### Step 1: Build the Application

```bash
# Navigate to the project directory
cd tiff_locator

# Build in release mode (optimized for performance)
cargo build --release
```

The executable will be created at:
- **Windows**: `target\release\tiff_locator.exe`
- **Linux/macOS**: `target/release/tiff_locator`

### Step 2: Run the Application

```bash
# Windows
.\target\release\tiff_locator.exe

# Linux/macOS
./target/release/tiff_locator
```

### Step 3: Initial Setup (One-Time)

1. **Prepare Your CSV File**
   - Create a CSV file with a column named `hh_id`
   - Example (`ids.csv`):
     ```csv
     hh_id
     HH001
     HH002
     ABC123
     ```
   - A sample file `sample_ids.csv` is included in the project

2. **Select Your Folder**
   - Click "📁 Select Folder"
   - Choose the directory containing your TIFF files
   - Can be a local folder or network path

3. **Scan the Directory**
   - Click "🔍 Scan Directory"
   - Wait for the scan to complete
   - Results are permanently stored in `cache.db`
   - **Only needs to be done once** (or when files change)

4. **Load Reference IDs**
   - Click "📄 Select CSV"
   - Choose your CSV file with household IDs
   - Click "📥 Load Reference IDs"
   - IDs are permanently stored in the database
   - **Only needs to be done once** (or when updating IDs)

### Step 4: Search for Household IDs (Repeated)

5. **Adjust Similarity Threshold** (Optional)
   - Use the slider to set match quality (50%-100%)
   - Default is 70%
   - Higher = stricter matching

6. **Search for an ID**
   - Type or paste a household ID in the search box
   - Click "🔍 Search"
   - Results appear instantly with similarity scores

7. **View and Open Files**
   - Browse the search results table
   - Click "📂 Open Location" to open the file in your file explorer
   - The file will be highlighted/selected

8. **Search Another ID**
   - Clear the search box or enter a new ID
   - Click "🔍 Search" again
   - Results update instantly

9. **Export Results** (Optional)
   - Click "📤 Export Results"
   - Save the current search results to a CSV file

## 💡 Tips & Tricks

### Instant Searches
- After initial setup, searches complete in <1 second
- No need to rescan or reload between searches
- Respects your current similarity threshold setting

### Clear Cache
- Click "🗑 Clear Cache" if directory contents have changed
- Forces a fresh scan on next operation

### Network Paths
- **Windows UNC**: `\\server\share\folder`
- **Mapped Drives**: `Z:\folder`
- **Linux/macOS**: `/mnt/share/folder`

### Performance
- The app uses all CPU cores for scanning and searching
- Can handle 10,000+ files efficiently
- First scan may take time, but results are permanently cached
- Searches complete in <1 second after initial setup

### Updating Reference IDs
- To add new IDs, select an updated CSV and click "Load Reference IDs" again
- Duplicate IDs are automatically skipped
- Or use "Clear Cache" to start completely fresh

## 🔧 Troubleshooting

### "No files found in database"
→ Click "🔍 Scan Directory" before searching

### "CSV must contain 'hh_id' column"
→ Ensure your CSV has a header row with `hh_id` (case-insensitive)

### "No reference IDs loaded"
→ Click "Load Reference IDs" after selecting your CSV

### Slow network scanning
→ Results are cached permanently, so subsequent searches are instant

### File location won't open
→ Ensure the file still exists at the cached path

## 📊 Example Workflow

```
1. Select folder: C:\Documents\TiffFiles
2. Scan directory: Found 5,432 TIFF files (one-time setup)
3. Load CSV: ids.csv (100 household IDs loaded into database)
4. Set threshold: 70%
5. Search for "HH12345": Found 3 matches in <1 second
6. Open location for best match
7. Search for "ABC789": Found 5 matches in <1 second
8. Export results to: search_results.csv
```

## 🎯 What Gets Matched?

The fuzzy matcher intelligently extracts IDs from filenames:

- `HH001.tif` → matches `HH001`
- `scan_HH001_final.tiff` → matches `HH001`
- `HH-001-document.tif` → matches `HH001`
- `household_HH001.tif` → matches `HH001`

The matcher removes:
- File extensions (`.tif`, `.tiff`)
- Common separators (`_`, `-`, `.`, spaces)
- Prefixes and suffixes

## 📁 Project Structure

```
tiff_locator/
├── src/
│   ├── main.rs          # Application entry point
│   ├── gui.rs           # GUI implementation
│   ├── database.rs      # SQLite operations
│   ├── scanner.rs       # Directory scanning
│   ├── matcher.rs       # Fuzzy matching
│   └── opener.rs        # File location opener
├── Cargo.toml           # Dependencies
├── cache.db             # SQLite cache (created on first run)
├── sample_ids.csv       # Sample CSV file
└── README.md            # Full documentation
```

## 🚀 Next Steps

- Read the full [README.md](README.md) for detailed documentation
- Customize the similarity threshold for your use case
- Set up automated workflows with the cached results

---

**Need Help?** Check the [README.md](README.md) for detailed troubleshooting and technical information.

