# ─────────────────────────────────────────────────────────────────
#  todo — Windows installer (PowerShell)
#  Usage:
#    irm https://raw.githubusercontent.com/Yasar195/mcply/release/install.ps1 | iex
#
#  With options (save locally first, then run):
#    .\install.ps1 -Version v1.2.3 -InstallDir "$env:LOCALAPPDATA\Programs\todo"
# ─────────────────────────────────────────────────────────────────
param(
    [string]$Version    = "",
    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\mcply"
)

$Repo   = "Yasar195/mcply"   # ← change this
$Binary = "mcply"
$ErrorActionPreference = "Stop"

function Info  { param($msg) Write-Host "[info]  $msg" -ForegroundColor Cyan }
function Ok    { param($msg) Write-Host "[ ok ]  $msg" -ForegroundColor Green }
function Err   { param($msg) Write-Host "[err ]  $msg" -ForegroundColor Red; exit 1 }

# ── resolve version ───────────────────────────
if (-not $Version) {
    Info "Fetching latest release tag…"
    try {
        $resp    = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest"
        $Version = $resp.tag_name
    } catch {
        Err "Could not determine latest version. $_"
    }
}

Info "Installing $Binary $Version (windows-amd64)"

# ── build URL ────────────────────────────────
$AssetName = "$Binary-windows-amd64.exe.zip"
$Url       = "https://github.com/$Repo/releases/download/$Version/$AssetName"

# ── download ─────────────────────────────────
$TmpDir = Join-Path $env:TEMP "todo-install-$([System.IO.Path]::GetRandomFileName())"
New-Item -ItemType Directory -Path $TmpDir | Out-Null

$ZipPath = Join-Path $TmpDir $AssetName
Info "Downloading $Url"
try {
    Invoke-WebRequest -Uri $Url -OutFile $ZipPath -UseBasicParsing
} catch {
    Err "Download failed: $_"
}

# ── extract ───────────────────────────────────
Info "Extracting…"
Expand-Archive -Path $ZipPath -DestinationPath $TmpDir -Force

# ── install ───────────────────────────────────
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir | Out-Null
}

$ExeSrc  = Join-Path $TmpDir "$Binary.exe"
$ExeDest = Join-Path $InstallDir "$Binary.exe"
Move-Item -Path $ExeSrc -Destination $ExeDest -Force

# ── clean up ──────────────────────────────────
Remove-Item -Recurse -Force $TmpDir

Ok "$Binary installed → $ExeDest"

# ── PATH management ───────────────────────────
$UserPath = [System.Environment]::GetEnvironmentVariable("PATH", "User")
if ($UserPath -notlike "*$InstallDir*") {
    Info "Adding $InstallDir to your user PATH…"
    [System.Environment]::SetEnvironmentVariable(
        "PATH",
        "$UserPath;$InstallDir",
        "User"
    )
    $env:PATH += ";$InstallDir"
    Info "PATH updated. You may need to restart your terminal."
} else {
    Info "$InstallDir is already in your PATH."
}

Info "Run '$Binary --help' to get started."
