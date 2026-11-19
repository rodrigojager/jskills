# Exercise the unpacked public binary with Bun, Node, Python and native harnesses
# absent from PATH. Uses an existing Git checkout; never mutates Git or deletes data.
# pwsh -File scripts/smoke.ps1 -Binary <unpacked-jtasks> -Root <checkout> -FixtureDir <new-data>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Binary,
    [Parameter(Mandatory = $true)][string]$Root,
    [Parameter(Mandatory = $true)][string]$FixtureDir
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$Binary = (Resolve-Path -LiteralPath $Binary).Path
$Root = (Resolve-Path -LiteralPath $Root).Path
$FixtureDir = [System.IO.Path]::GetFullPath($FixtureDir)
if ($FixtureDir -eq $Root -or $FixtureDir.StartsWith($Root + [System.IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Smoke fixture must be outside the product checkout'
}
if (Test-Path -LiteralPath $FixtureDir) { throw 'Smoke fixture must be a new path' }
$git = (Get-Command git -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
$windowsHost = [System.Environment]::OSVersion.Platform -eq 'Win32NT'
New-Item -ItemType Directory -Path $FixtureDir | Out-Null
$oldPath = $env:PATH
$oldLanguage = $env:JTASKS_LANG
function Invoke-Public([string[]]$Arguments) {
    $output = @(& $Binary --lang en --root $Root --data-dir (Join-Path $FixtureDir 'runtime') @Arguments)
    if ($LASTEXITCODE -ne 0) { throw "Public command failed ($LASTEXITCODE): $($Arguments -join ' ')" }
    Write-Host ($Arguments -join ' ')
    $output -join "`n"
}
try {
    if ($windowsHost) {
        $env:PATH = (Split-Path -Parent $git) + ';' + (Join-Path $env:SystemRoot 'System32')
    } else {
        $tools = Join-Path $FixtureDir 'tools'
        New-Item -ItemType Directory -Path $tools | Out-Null
        [System.IO.File]::CreateSymbolicLink((Join-Path $tools 'git'), $git) | Out-Null
        $env:PATH = $tools
    }
    $env:JTASKS_LANG = 'en'
    foreach ($absent in @('bun', 'node', 'python', 'python3', 'claude', 'codex', 'opencode', 'jlock', 'jproject')) {
        if (Get-Command $absent -CommandType Application -ErrorAction SilentlyContinue) { throw "$absent remains available in isolated PATH" }
    }
    $isolatedGit = (Get-Command git -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
    & $isolatedGit -C $Root rev-parse --absolute-git-dir | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Git could not resolve the test checkout on the isolated PATH' }
    Write-Host "Isolated runtime PATH: $env:PATH"
    $version = Invoke-Public @('--version')
    $manifestVersion = Select-String -LiteralPath (Join-Path $PSScriptRoot '../Cargo.toml') -Pattern '^version = "([^"]+)"$' |
        Select-Object -First 1
    if (!$manifestVersion) { throw 'Package version is missing from Cargo.toml' }
    $expectedVersion = 'jtasks ' + $manifestVersion.Matches[0].Groups[1].Value
    if ($version -ne $expectedVersion) { throw "Expected $expectedVersion, got $version" }
    $null = Invoke-Public @('--help')
    $planPath = Join-Path $FixtureDir 'plan.json'
    @{
        schema_version = 1; completion = 'judgment'; operation_placement = 'none'
        tasks = @(
            @{id = 'A'; title = 'Packaged public smoke'; executor = @{kind = 'fixture'; outcome = 'completed'; summary = 'Native packaged task completed'}},
            @{id = 'B'; title = 'Dependent packaged task'; depends_on = @('A'); executor = @{kind = 'fixture'; outcome = 'completed'; summary = 'Dependency result accepted'}}
        )
    } | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $planPath -Encoding utf8
    $validation = Invoke-Public @('validate', $planPath) | ConvertFrom-Json
    if ($validation.tasks -ne 2) { throw 'Plan validation did not find two tasks' }
    $null = Invoke-Public @('operations', 'preview', $planPath)
    $null = Invoke-Public @('import', $planPath, '--run', 'packaged-smoke')
    $null = Invoke-Public @('run', '--run', 'packaged-smoke')
    $statusText = Invoke-Public @('status', '--run', 'packaged-smoke')
    $status = $statusText | ConvertFrom-Json
    # Print the public shape before inspecting its fields; assertions then use the
    # current projection rather than an internal SQLite contract.
    Write-Host "Status keys: $($status.PSObject.Properties.Name -join ', ')"
    Write-Host $statusText
    if ($status.state -ne 'completed' -or $status.tasks.Count -ne 2) { throw 'Packaged run did not complete both tasks' }
    foreach ($task in $status.tasks) {
        if ($task.state -ne 'completed' -or $task.attempts.Count -ne 1 -or -not $task.attempts[0].summary) { throw 'Missing designated task result or duplicate attempt' }
    }
    $null = Invoke-Public @('run', '--run', 'packaged-smoke')
    $null = Invoke-Public @('watch', '--run', 'packaged-smoke', '--once', '--json')
    $null = Invoke-Public @('operations', 'status', '--run', 'packaged-smoke')
    $null = Invoke-Public @('runs')
    $doctorText = Invoke-Public @('doctor')
    $doctor = $doctorText | ConvertFrom-Json
    Write-Host "Doctor keys: $($doctor.PSObject.Properties.Name -join ', ')"
    if (-not $doctor.read_only -or -not $doctor.runtime_database_exists) { throw 'Doctor did not return pure existing-runtime diagnostics' }
    $after = Invoke-Public @('status', '--run', 'packaged-smoke') | ConvertFrom-Json
    if (@($after.tasks | Where-Object { $_.attempts.Count -ne 1 }).Count) { throw 'Completed rerun launched an extra attempt' }
    Write-Host 'PASS: unpacked native binary, two dependent designated outcomes, idempotent rerun and read surfaces; no Bun/Node/Python/harness/jlock/jproject on PATH'
} finally {
    $env:PATH = $oldPath
    $env:JTASKS_LANG = $oldLanguage
}
