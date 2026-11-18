# Archive one complete package for GitHub Releases. No install or publication.
[CmdletBinding()]
param([Parameter(Mandatory=$true)][string]$PackageDir,[Parameter(Mandatory=$true)][string]$OutputDir)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$PackageDir = [IO.Path]::GetFullPath($PackageDir)
$OutputDir = [IO.Path]::GetFullPath($OutputDir)
$product = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if ($OutputDir -eq $product -or $OutputDir.StartsWith($product + [IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)) { throw 'Artifacts must be outside the product repository' }
if ((Split-Path -Leaf $PackageDir) -ne 'package') { throw 'Name the complete folder package so the ZIP has the canonical package/ root' }
$info = Get-Content -LiteralPath (Join-Path $PackageDir 'package-info.json') -Raw | ConvertFrom-Json
$runtime = Get-Content -LiteralPath (Join-Path $PackageDir 'runtime.json') -Raw | ConvertFrom-Json
$manifest = Get-Content -LiteralPath (Join-Path $PackageDir 'orca-plugin.json') -Raw | ConvertFrom-Json
if ($info.version -notmatch '^\d+\.\d+\.\d+$' -or $info.version -ne $runtime.cliVersion -or $info.version -ne $manifest.version) { throw 'Package versions do not match' }
if ($info.platform -ne 'win32' -or $info.arch -ne 'x64') { throw 'Verified complete update archives currently support Windows x64 only' }
$artifact = $runtime.artifacts.'win32-x64'
$binary = [IO.Path]::GetFullPath((Join-Path $PackageDir $artifact.file))
if (-not $binary.StartsWith($PackageDir + [IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)) { throw 'Native artifact escapes package' }
if ((Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash.ToLowerInvariant() -ne $artifact.sha256) { throw 'Native package hash mismatch' }
$archive = Join-Path $OutputDir ('jtasks-orca-' + $info.version + '-win32-x64.zip')
if ((Test-Path -LiteralPath $archive) -or (Test-Path -LiteralPath ($archive + '.sha256'))) { throw 'Archive output already exists; inspect it before selecting another output folder' }
New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
Compress-Archive -LiteralPath $PackageDir -DestinationPath $archive -CompressionLevel Optimal
$hash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
"$hash  $([IO.Path]::GetFileName($archive))" | Set-Content -LiteralPath ($archive + '.sha256') -Encoding ascii
Write-Output "archive=$archive"
Write-Output "sha256=$hash"
