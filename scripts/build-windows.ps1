# Build Portal Windows artifacts: portable zip + NSIS setup.exe.
#
# Preconditions (CI installs these; see .github/workflows/release.yml):
#   - cargo build --release --target <Target> already run
#   - makensis on PATH (choco install nsis)
#
# Usage (repo root):
#   ./scripts/build-windows.ps1 -Target x86_64-pc-windows-msvc -Arch x86_64 -Version 0.19.0
#
# Outputs (repo root):
#   Portal-windows-<Arch>-v<Version>.zip
#   Portal-windows-<Arch>-v<Version>-setup.exe
param(
    [Parameter(Mandatory = $true)][string]$Target,
    [Parameter(Mandatory = $true)][string]$Arch,
    [string]$Version = $env:VERSION
)

$ErrorActionPreference = "Stop"

if (-not $Version) {
    # Local fallback: derive from the newest tag ("v0.19.0" -> "0.19.0").
    $Version = ((git describe --tags --abbrev=0) -replace '^v', '')
}

$exe = "target/$Target/release/portal.exe"
if (-not (Test-Path $exe)) {
    throw "missing $exe — run 'cargo build --release --target $Target' first"
}

# ── 1. Portable zip (same convention as the Linux tar.gz: exe + README) ────
$stage = "target/win-zip-staging"
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item $exe "$stage/portal.exe"
Copy-Item ROADMAP.md "$stage/README.md"
$zip = "Portal-windows-$Arch-v$Version.zip"
Compress-Archive -Path "$stage/*" -DestinationPath $zip -Force
Write-Host "built $zip"

# ── 2. NSIS installer ──────────────────────────────────────────────────────
makensis /DARCH=$Arch /DVERSION=$Version /DTARGET=$Target installer/portal.nsi
$installer = "Portal-windows-$Arch-v$Version-setup.exe"
if (-not (Test-Path $installer)) { throw "makensis did not produce $installer" }
Write-Host "built $installer"
