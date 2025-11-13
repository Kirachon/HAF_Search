# Automated Test Script for TiffLocator

Write-Host "========================================"
Write-Host "TiffLocator Automated Test Suite"
Write-Host "========================================`n"

# Test 1: Check if test data exists
Write-Host "[Test 1] Verifying Test Data Setup"
$testDir = "test_data\tiff_files"
if (Test-Path $testDir) {
    $fileCount = (Get-ChildItem $testDir -Filter *.tif*).Count
    Write-Host "  [PASS] Test directory exists: $testDir"
    Write-Host "  [PASS] TIFF files found: $fileCount"
} else {
    Write-Host "  [FAIL] Test directory not found: $testDir"
    Write-Host "  Run create_test_files.ps1 first"
    exit 1
}

# Test 2: Check if sample CSV exists
Write-Host "`n[Test 2] Verifying Sample CSV"
if (Test-Path "sample_ids.csv") {
    $csvContent = Get-Content "sample_ids.csv"
    $idCount = ($csvContent | Select-Object -Skip 1 | Where-Object { $_.Trim() -ne "" }).Count
    Write-Host "  [PASS] sample_ids.csv exists"
    Write-Host "  [PASS] Household IDs in CSV: $idCount"
} else {
    Write-Host "  [FAIL] sample_ids.csv not found"
    exit 1
}

# Test 3: Check if application binary exists
Write-Host "`n[Test 3] Verifying Application Binary"
$binaryPath = "target\release\tiff_locator.exe"
if (Test-Path $binaryPath) {
    $binarySize = (Get-Item $binaryPath).Length / 1MB
    Write-Host "  [PASS] Application binary exists: $binaryPath"
    Write-Host "  [PASS] Binary size: $([math]::Round($binarySize, 2)) MB"
} else {
    Write-Host "  [FAIL] Application binary not found: $binaryPath"
    Write-Host "  Run 'cargo build --release' first"
    exit 1
}

# Test 4: Database verification (if cache.db exists)
Write-Host "`n[Test 4] Database Verification"
if (Test-Path "cache.db") {
    Write-Host "  [PASS] cache.db exists"
    Write-Host "  [INFO] Run the application to populate the database"
} else {
    Write-Host "  [INFO] cache.db not found (will be created on first run)"
}

Write-Host "`n========================================"
Write-Host "Pre-Test Checks Complete"
Write-Host "========================================`n"

Write-Host "MANUAL TESTING INSTRUCTIONS:`n"
Write-Host "1. Launch: .\target\release\tiff_locator.exe`n"
Write-Host "2. Load Test Data:"
Write-Host "   a) Select Folder -> test_data\tiff_files"
Write-Host "   b) Scan Directory -> Wait for completion"
Write-Host "   c) Select CSV -> sample_ids.csv"
Write-Host "   d) Load Reference IDs -> Wait for completion`n"
Write-Host "3. Test Searches:"
Write-Host "   a) Search 'HH001' -> Should find HH001_document.tif"
Write-Host "   b) Search 'ABC123' -> Should find 2 files"
Write-Host "   c) Search 'XYZ789' -> Should find 2 files"
Write-Host "   d) Search 'NOTFOUND' -> No results`n"
Write-Host "4. Test Threshold:"
Write-Host "   a) Set to 90% -> Search 'HH003'"
Write-Host "   b) Set to 50% -> Search 'HH003' again"
Write-Host "   c) Verify more results with lower threshold`n"
Write-Host "5. Test Export:"
Write-Host "   a) After search, click Export Results"
Write-Host "   b) Verify CSV file created`n"
Write-Host "6. Test Persistence:"
Write-Host "   a) Close application"
Write-Host "   b) Reopen application"
Write-Host "   c) Search 'HH001' WITHOUT rescanning"
Write-Host "   d) Verify results appear`n"

Write-Host "========================================`n"

