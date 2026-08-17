# Windows Release build helper
#
# gpui 0.2.2 requires fxc.exe (HLSL shader compiler, bundled with the Windows SDK)
# when building in release mode. gpui's build.rs searches for fxc.exe in this order:
#   1. GPUI_FXC_PATH environment variable
#   2. fxc.exe on PATH (where.exe)
#   3. Hardcoded path 10.0.26100.0 (may not match the locally installed SDK version)
#
# This script auto-detects the local fxc.exe, sets GPUI_FXC_PATH, then runs cargo build --release.
#
# Usage:
#   ./scripts/build-windows.ps1              # cargo build --release
#   ./scripts/build-windows.ps1 test         # cargo test
#   ./scripts/build-windows.ps1 clippy       # cargo clippy --all-targets -- -D warnings

param(
    [Parameter(Position = 0)]
    [ValidateSet("build", "test", "clippy")]
    [string]$Command = "build"
)

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$rootDir = Split-Path -Parent $scriptDir
Set-Location $rootDir

# --- Locate fxc.exe ---
# Returns the full path to fxc.exe, or throws if not found.
function Find-FxcCompiler {
    # 1. Already set via env var and the path exists
    $existing = $env:GPUI_FXC_PATH
    if ($existing -and (Test-Path $existing)) {
        Write-Host "Using existing GPUI_FXC_PATH: $existing" -ForegroundColor DarkGray
        return $existing
    }

    # 2. Available on PATH
    $found = Get-Command fxc.exe -ErrorAction SilentlyContinue
    if ($found -and $found.Source -and (Test-Path $found.Source)) {
        Write-Host "Found fxc.exe on PATH: $($found.Source)" -ForegroundColor DarkGray
        return $found.Source
    }

    # 3. Search Windows Kits for the newest x64 fxc.exe across all SDK versions
    $kitsBin = "${env:ProgramFiles(x86)}\Windows Kits\10\bin"
    if (-not (Test-Path $kitsBin)) {
        $kitsBin = "$env:ProgramFiles\Windows Kits\10\bin"
    }
    if (-not (Test-Path $kitsBin)) {
        throw "Windows Kits directory not found. Please install the Windows SDK (includes fxc.exe) and retry."
    }

    # Relax ErrorActionPreference during the recursive search so that
    # access-denied errors on subdirectories don't abort the pipeline.
    $prevEAP = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $fxc = Get-ChildItem -Path $kitsBin -Filter "fxc.exe" -Recurse -ErrorAction SilentlyContinue |
            Where-Object { $_.Directory.Name -eq "x64" } |
            Sort-Object FullName -Descending |
            Select-Object -First 1
    } finally {
        $ErrorActionPreference = $prevEAP
    }

    if (-not $fxc -or -not $fxc.FullName) {
        throw "No x64 fxc.exe found under $kitsBin. Install the 'Desktop development with C++' workload via Visual Studio Installer (includes Windows SDK)."
    }

    Write-Host "Auto-detected fxc.exe: $($fxc.FullName)" -ForegroundColor Green
    return $fxc.FullName
}

# Only release builds need fxc.exe (debug mode skips shader compilation in gpui)
if ($Command -eq "build") {
    $fxcPath = Find-FxcCompiler
    $env:GPUI_FXC_PATH = $fxcPath
    Write-Host "==> cargo build --release" -ForegroundColor Cyan
    cargo build --release
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
elseif ($Command -eq "test") {
    Write-Host "==> cargo test" -ForegroundColor Cyan
    cargo test
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
elseif ($Command -eq "clippy") {
    Write-Host "==> cargo clippy --all-targets -- -D warnings" -ForegroundColor Cyan
    cargo clippy --all-targets -- -D warnings
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
