# Create test TIFF files for TiffLocator testing

$testFiles = @(
    'HH001_document.tif',
    'household_HH002_scan.tiff',
    'ABC123-file.tif',
    'report_XYZ789.tiff',
    'TEST001_data.tif',
    'DEMO456_record.tiff',
    'random_file_001.tif',
    'another_document.tiff',
    'HH003_partial_match.tif',
    'SAMPLE999_test.tif',
    'HH004_invoice.tif',
    'HH005_receipt.tiff',
    'unrelated_file.tif',
    'document_ABC123.tiff',
    'XYZ789_report.tif'
)

$targetDir = "test_data\tiff_files"

Write-Host "Creating test TIFF files in $targetDir..."

foreach ($file in $testFiles) {
    $filePath = Join-Path $targetDir $file
    New-Item -Path $filePath -ItemType File -Force | Out-Null
    Write-Host "  Created: $file"
}

$count = (Get-ChildItem $targetDir).Count
Write-Host "`nTotal files created: $count"

