# TiffLocator - Comprehensive Test Plan

## Test Environment Setup

### Prerequisites
- TiffLocator application built in release mode
- Test directory with sample TIFF files
- Test CSV file with household IDs
- Clean database (delete `cache.db` before testing)

### Test Data Structure
```
test_data/
├── tiff_files/
│   ├── HH001_document.tif
│   ├── household_HH002_scan.tiff
│   ├── ABC123-file.tif
│   ├── report_XYZ789.tiff
│   ├── TEST001_data.tif
│   ├── DEMO456_record.tiff
│   ├── random_file_001.tif
│   ├── another_document.tiff
│   ├── HH003_partial_match.tif
│   └── SAMPLE999_test.tif
└── test_ids.csv
```

---

## Test Suite

### Test 1: Reference ID Loading

#### Test 1.1: Load IDs from CSV
**Steps:**
1. Launch TiffLocator
2. Click "📄 Select CSV"
3. Select `sample_ids.csv`
4. Click "📥 Load Reference IDs"
5. Wait for completion

**Expected Results:**
- ✅ Status message: "Loaded X reference IDs"
- ✅ Reference ID count displayed: "(X reference IDs loaded)"
- ✅ No errors displayed

**Verification:**
- Query database: `SELECT COUNT(*) FROM reference_ids;`
- Should match number of IDs in CSV (10 IDs)

#### Test 1.2: Duplicate ID Handling
**Steps:**
1. Click "📥 Load Reference IDs" again with same CSV
2. Observe behavior

**Expected Results:**
- ✅ No duplicate IDs inserted
- ✅ Reference ID count remains the same
- ✅ No errors (duplicates silently skipped)

#### Test 1.3: Invalid CSV Format
**Steps:**
1. Create CSV without `hh_id` column
2. Try to load it

**Expected Results:**
- ✅ Error message: "CSV must contain 'hh_id' column"
- ✅ No IDs loaded

---

### Test 2: Directory Scanning

#### Test 2.1: Scan Test Directory
**Steps:**
1. Click "📁 Select Folder"
2. Select test directory with TIFF files
3. Click "🔍 Scan Directory"
4. Wait for completion

**Expected Results:**
- ✅ Progress bar shows scanning status
- ✅ Status message: "Scanned X files"
- ✅ Files cached in database

**Verification:**
- Query database: `SELECT COUNT(*) FROM files;`
- Should match number of TIFF files in directory

#### Test 2.2: Rescan Same Directory
**Steps:**
1. Click "🔍 Scan Directory" again

**Expected Results:**
- ✅ Duplicate files not inserted (UNIQUE constraint)
- ✅ File count remains consistent

---

### Test 3: Single-ID Search Functionality

#### Test 3.1: Search for Exact Match
**Steps:**
1. Enter "HH001" in search box
2. Click "🔍 Search"

**Expected Results:**
- ✅ Results appear in <1 second
- ✅ File "HH001_document.tif" appears with high similarity (>90%)
- ✅ Results sorted by similarity (highest first)
- ✅ Similarity scores displayed as percentages

#### Test 3.2: Search for Partial Match
**Steps:**
1. Enter "HH002" in search box
2. Click "🔍 Search"

**Expected Results:**
- ✅ File "household_HH002_scan.tiff" appears
- ✅ Similarity score reflects fuzzy matching quality
- ✅ Other files with partial matches may appear (lower scores)

#### Test 3.3: Search for Non-Existent ID
**Steps:**
1. Enter "NOTFOUND999" in search box
2. Click "🔍 Search"

**Expected Results:**
- ✅ Message: "Enter a household ID and click Search to find matching TIFF files."
- ✅ No results displayed
- ✅ No errors

#### Test 3.4: Search with Different Threshold
**Steps:**
1. Set similarity threshold to 90%
2. Search for "HH003"
3. Note number of results
4. Set threshold to 50%
5. Search for "HH003" again

**Expected Results:**
- ✅ Higher threshold (90%) = fewer results
- ✅ Lower threshold (50%) = more results
- ✅ Results filtered correctly based on threshold

#### Test 3.5: Multiple Sequential Searches
**Steps:**
1. Search for "HH001"
2. Search for "ABC123"
3. Search for "XYZ789"
4. Search for "TEST001"

**Expected Results:**
- ✅ Each search completes in <1 second
- ✅ Results update correctly for each search
- ✅ Previous results are replaced (not appended)
- ✅ GUI remains responsive

---

### Test 4: Database Persistence

#### Test 4.1: Verify Scanned Files Persist
**Steps:**
1. Close TiffLocator
2. Reopen TiffLocator
3. Perform a search without rescanning

**Expected Results:**
- ✅ Search works immediately
- ✅ No need to rescan directory
- ✅ Results match previous session

#### Test 4.2: Verify Reference IDs Persist
**Steps:**
1. Close TiffLocator
2. Delete `sample_ids.csv` (simulate file removal)
3. Reopen TiffLocator
4. Perform a search

**Expected Results:**
- ✅ Search still works (IDs stored in database)
- ✅ Reference ID count still displayed
- ✅ No need to reload CSV

---

### Test 5: Search Results Display

#### Test 5.1: Results Table Format
**Steps:**
1. Perform any search with results

**Expected Results:**
- ✅ Table shows: File Name, Similarity, Action
- ✅ NO "HH ID" column (removed in redesign)
- ✅ Similarity displayed as percentage (e.g., "85.3%")
- ✅ "📂 Open Location" button for each result

#### Test 5.2: Pagination (if >500 results)
**Steps:**
1. Create directory with 1,000+ TIFF files
2. Search for common pattern

**Expected Results:**
- ✅ Results paginated (500 per page)
- ✅ "Previous" and "Next" buttons work
- ✅ Page indicator shows current page
- ✅ Smooth scrolling within page

---

### Test 6: Export Functionality

#### Test 6.1: Export Search Results
**Steps:**
1. Perform a search with results
2. Click "📤 Export Results"
3. Save to file

**Expected Results:**
- ✅ CSV file created
- ✅ Contains current search results only
- ✅ Columns: file_name, file_path, similarity_score
- ✅ File opens correctly in Excel/spreadsheet app

---

### Test 7: Performance Testing

#### Test 7.1: Search Speed
**Steps:**
1. Measure time from clicking "Search" to results displayed
2. Test with various IDs

**Expected Results:**
- ✅ Search completes in <1 second
- ✅ Consistent performance across multiple searches

#### Test 7.2: Large Directory Handling
**Steps:**
1. Scan directory with 1,000+ TIFF files
2. Load 100+ reference IDs
3. Perform searches

**Expected Results:**
- ✅ Scanning completes successfully
- ✅ Searches remain fast (<1 second)
- ✅ GUI remains responsive (60 FPS)
- ✅ No memory issues

---

## Test Execution Checklist

- [ ] Test 1.1: Load IDs from CSV
- [ ] Test 1.2: Duplicate ID Handling
- [ ] Test 1.3: Invalid CSV Format
- [ ] Test 2.1: Scan Test Directory
- [ ] Test 2.2: Rescan Same Directory
- [ ] Test 3.1: Search for Exact Match
- [ ] Test 3.2: Search for Partial Match
- [ ] Test 3.3: Search for Non-Existent ID
- [ ] Test 3.4: Search with Different Threshold
- [ ] Test 3.5: Multiple Sequential Searches
- [ ] Test 4.1: Verify Scanned Files Persist
- [ ] Test 4.2: Verify Reference IDs Persist
- [ ] Test 5.1: Results Table Format
- [ ] Test 5.2: Pagination
- [ ] Test 6.1: Export Search Results
- [ ] Test 7.1: Search Speed
- [ ] Test 7.2: Large Directory Handling

---

## Bug Tracking Template

| Test ID | Issue Description | Severity | Status |
|---------|------------------|----------|--------|
| | | | |

