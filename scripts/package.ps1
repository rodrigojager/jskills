# Build a local locked release and distributable archive. Run with PowerShell 7 on
# Windows or Linux: pwsh -File scripts/package.ps1 -TargetDir <build> -OutputDir <artifacts>.
# Never installs, publishes, removes files or starts a provider/account session.
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$TargetDir,
    [Parameter(Mandatory = $true)][string]$OutputDir,
    [string]$Target
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$product = Split-Path -Parent $PSScriptRoot
$windowsHost = [System.Environment]::OSVersion.Platform -eq 'Win32NT'
if (-not $Target) {
    $hostLine = (& rustc -Vv | Select-String '^host: ').ToString()
    if ($LASTEXITCODE -ne 0) { throw 'rustc host inspection failed' }
    $Target = $hostLine.Substring(6)
}
if ($Target -notin @('x86_64-pc-windows-msvc', 'x86_64-unknown-linux-gnu')) {
    throw 'Packaging currently supports x86_64 Windows MSVC and Linux GNU only'
}
if ($windowsHost -ne ($Target -eq 'x86_64-pc-windows-msvc')) {
    throw 'Use a native host for packaging; cross-platform execution is not verified'
}
$TargetDir = [System.IO.Path]::GetFullPath($TargetDir)
$OutputDir = [System.IO.Path]::GetFullPath($OutputDir)
foreach ($destination in @($TargetDir, $OutputDir)) {
    if ($destination -eq $product -or $destination.StartsWith($product + [System.IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Build and archive outputs must be outside the product checkout'
    }
}
$manifest = Join-Path $product 'Cargo.toml'
$oldFlags = $env:RUSTFLAGS
try {
    # Link the Windows C runtime into the executable; OS DLLs remain required.
    if ($windowsHost) { $env:RUSTFLAGS = ($oldFlags + ' -C target-feature=+crt-static').Trim() }
    & cargo build --locked --release --bin jtasks --manifest-path $manifest --target $Target --target-dir $TargetDir
    if ($LASTEXITCODE -ne 0) { throw 'Locked optimized build failed' }
    $metadataText = & cargo metadata --locked --offline --format-version 1 --filter-platform $Target --manifest-path $manifest
    if ($LASTEXITCODE -ne 0) { throw 'Locked dependency inspection failed' }
    $metadata = $metadataText | ConvertFrom-Json
    $package = @($metadata.packages | Where-Object { $_.name -eq 'jtasks' -and -not $_.source })[0]
    $name = 'jtasks-' + $package.version + '-' + $Target
    $stage = Join-Path $OutputDir $name
    $extension = if ($windowsHost) { '.zip' } else { '.tar.gz' }
    $archive = Join-Path $OutputDir ($name + $extension)
    if ((Test-Path -LiteralPath $stage) -or (Test-Path -LiteralPath $archive)) {
        throw 'Output already exists; inspect it before choosing a fresh output directory'
    }
    New-Item -ItemType Directory -Path $stage -Force | Out-Null
    $binaryName = if ($windowsHost) { 'jtasks.exe' } else { 'jtasks' }
    Copy-Item -LiteralPath (Join-Path $TargetDir "$Target/release/$binaryName") -Destination (Join-Path $stage $binaryName)
    foreach ($file in @('README.md', 'LICENSE', 'Cargo.toml', 'Cargo.lock')) {
        Copy-Item -LiteralPath (Join-Path $product $file) -Destination $stage
    }
    Copy-Item -LiteralPath (Join-Path $product 'skills') -Destination $stage -Recurse
    Copy-Item -LiteralPath (Join-Path $product 'scripts') -Destination $stage -Recurse
    $notices = Join-Path $stage 'licenses'
    New-Item -ItemType Directory -Path $notices | Out-Null
    Copy-Item -LiteralPath (Join-Path $product 'locales/LICENSE') -Destination (Join-Path $notices 'legacy-translations-LICENSE')
    $resolved = @($metadata.resolve.nodes | ForEach-Object { $_.id })
    $dependencies = @($metadata.packages | Where-Object { $_.source -and $_.id -in $resolved } | Sort-Object name, version)
    foreach ($dependency in $dependencies) {
        $noticeDir = Join-Path $notices ($dependency.name + '-' + $dependency.version)
        New-Item -ItemType Directory -Path $noticeDir | Out-Null
        $sourceDir = Split-Path -Parent $dependency.manifest_path
        $licenseFiles = @(Get-ChildItem -LiteralPath $sourceDir -File | Where-Object { $_.Name -match '^(LICENSE|LICENCE|COPYING|NOTICE)([-.]|$)' })
        if ($dependency.license_file) {
            $licenseFiles += Get-Item -LiteralPath (Join-Path $sourceDir $dependency.license_file)
        }
        foreach ($notice in ($licenseFiles | Sort-Object FullName -Unique)) {
            Copy-Item -LiteralPath $notice.FullName -Destination $noticeDir
        }
        if (-not $licenseFiles.Count) { throw "No license file found for $($dependency.name)" }
    }
    $dependencyEvidence = @($dependencies | Select-Object name, version, license, rust_version)
    $dependencyEvidence | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $stage 'dependencies.json') -Encoding utf8
    $evidence = [ordered]@{
        package = $package.name; version = $package.version; target = $Target
        rustc = @(& rustc -Vv); cargo = (& cargo -V)
        declared_rust_minimum = $package.rust_version
        minimum_toolchain_tested = $false
        release_profile = 'Cargo default release (optimized)'
        windows_static_crt = $windowsHost
        cargo_lock_sha256 = (Get-FileHash -LiteralPath (Join-Path $product 'Cargo.lock') -Algorithm SHA256).Hash.ToLowerInvariant()
        binary_sha256 = (Get-FileHash -LiteralPath (Join-Path $stage $binaryName) -Algorithm SHA256).Hash.ToLowerInvariant()
        source_revision = (& git -C $product rev-parse HEAD)
        source_dirty = [bool](& git -C $product status --porcelain)
        dependency_count = $dependencies.Count
        runtime = 'Native OS plus git for workspace commands; selected harness/commands remain external'
        accounts_tested = $false
    }
    $evidence | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $stage 'build-info.json') -Encoding utf8
    if ($windowsHost) {
        Compress-Archive -LiteralPath $stage -DestinationPath $archive -CompressionLevel Optimal
    } else {
        & tar -czf $archive -C $OutputDir $name
        if ($LASTEXITCODE -ne 0) { throw 'Archive creation failed' }
    }
    $archiveHash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    "$archiveHash  $([System.IO.Path]::GetFileName($archive))" | Set-Content -LiteralPath ($archive + '.sha256') -Encoding ascii
    # Self-update consumes one native file, not an extracted source/archive tree.
    $rawName = $name + $(if ($windowsHost) { '.exe' } else { '.bin' })
    $raw = Join-Path $OutputDir $rawName
    if (Test-Path -LiteralPath $raw) { throw 'Raw update asset already exists' }
    Copy-Item -LiteralPath (Join-Path $stage $binaryName) -Destination $raw
    $rawHash = (Get-FileHash -LiteralPath $raw -Algorithm SHA256).Hash.ToLowerInvariant()
    "$rawHash  $rawName" | Set-Content -LiteralPath ($raw + '.sha256') -Encoding ascii
    # This creates a local source archive only, never a registry upload. The queue
    # commits after validation, so the exact working source is intentionally allowed.
    & cargo package --locked --offline --allow-dirty --no-verify --manifest-path $manifest --target-dir $TargetDir
    if ($LASTEXITCODE -ne 0) { throw 'Local Cargo source package failed' }
    Write-Output "archive=$archive"
    Write-Output "sha256=$archiveHash"
    Write-Output "source_package=$(Join-Path $TargetDir "package/jtasks-$($package.version).crate")"
} finally {
    $env:RUSTFLAGS = $oldFlags
}
