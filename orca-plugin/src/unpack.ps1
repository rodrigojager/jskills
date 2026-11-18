# Extract only a bounded, regular-file package tree into a new directory.
param([Parameter(Mandatory=$true)][string]$Archive,[Parameter(Mandatory=$true)][string]$Destination)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Add-Type -AssemblyName System.IO.Compression.FileSystem
$root = [IO.Path]::GetFullPath($Destination)
if (Test-Path -LiteralPath $root) { throw 'Extraction destination already exists' }
$zip = [IO.Compression.ZipFile]::OpenRead($Archive)
try {
    if ($zip.Entries.Count -gt 4096) { throw 'Too many archive entries' }
    $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    [long]$size = 0
    foreach ($entry in $zip.Entries) {
        $name = $entry.FullName
        $parts = $name.TrimEnd('/').Split('/')
        if ($parts[0] -ne 'package' -or $name.Contains('\') -or $name.Contains(':') -or @($parts | Where-Object { $_ -in @('', '.', '..') }).Count) { throw 'Unsafe archive path' }
        if (-not $seen.Add($name.TrimEnd('/'))) { throw 'Duplicate archive path' }
        if ((($entry.ExternalAttributes -shr 16) -band 0xF000) -eq 0xA000) { throw 'Archive links are not allowed' }
        $size += $entry.Length
        if ($size -gt 536870912) { throw 'Extracted package is too large' }
        $target = [IO.Path]::GetFullPath((Join-Path $root $name))
        if (-not $target.StartsWith($root + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Archive escapes destination' }
    }
    [IO.Directory]::CreateDirectory($root) | Out-Null
    foreach ($entry in $zip.Entries) {
        $target = Join-Path $root $entry.FullName
        if ($entry.FullName.EndsWith('/')) { [IO.Directory]::CreateDirectory($target) | Out-Null; continue }
        [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($target)) | Out-Null
        $inputFile = $entry.Open()
        try {
            $outputFile = [IO.File]::Open($target, [IO.FileMode]::CreateNew)
            try { $inputFile.CopyTo($outputFile) } finally { $outputFile.Dispose() }
        } finally { $inputFile.Dispose() }
    }
} finally { $zip.Dispose() }
