# ──────────────────────────────────────────────────────────────
# onchainos installer / updater (Windows)
#
# Usage:
#   irm https://raw.githubusercontent.com/okx/onchainos-skills/main/install.ps1 | iex
#
# Behavior:
#   - Fresh install: detect platform, download latest binary, verify, install.
#   - Already installed: skip if the same version was verified within the
#     last 12 hours. Otherwise, compare the local version with the latest
#     GitHub release and upgrade if needed.
#
# Supported platforms:
#   Windows : x86_64 (AMD64), x86 (i686), ARM64
# ──────────────────────────────────────────────────────────────

$ErrorActionPreference = "Stop"

$REPO = "okx/onchainos-skills"
$BINARY = "onchainos"
$INSTALL_DIR = "$env:USERPROFILE\.local\bin"
$CACHE_DIR = "$env:USERPROFILE\.onchainos"
$CACHE_FILE = "$CACHE_DIR\last_check"
$CACHE_TTL = 43200  # 12 hours in seconds

function Get-Target {
    $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
    switch ($arch) {
        "X64"   { return "x86_64-pc-windows-msvc" }
        "X86"   { return "i686-pc-windows-msvc" }
        "Arm64" { return "aarch64-pc-windows-msvc" }
        default {
            Write-Host "Unsupported architecture: $arch" -ForegroundColor Red
            exit 1
        }
    }
}

function Test-CacheFresh {
    if (-not (Test-Path $CACHE_FILE)) { return $false }
    try {
        $cachedTs = [long](Get-Content $CACHE_FILE -First 1)
        $now = [long](Get-Date -UFormat %s)
        return ($now - $cachedTs) -lt $CACHE_TTL
    } catch {
        return $false
    }
}

function Write-Cache {
    New-Item -ItemType Directory -Path $CACHE_DIR -Force | Out-Null
    [long](Get-Date -UFormat %s) | Out-File -FilePath $CACHE_FILE -Encoding ascii
}

function Get-LocalVersion {
    $exePath = Join-Path $INSTALL_DIR "$BINARY.exe"
    if (Test-Path $exePath) {
        $output = & $exePath --version 2>$null
        if ($output -match '(\d+\.\d+\.\d+)') {
            return $Matches[1]
        }
    }
    return $null
}

function Normalize-Tag {
    param([string]$tag)
    return $tag -replace '^v', ''
}

function Install-Binary {
    param([string]$tag)

    $target = Get-Target
    $binaryName = "${BINARY}-${target}.exe"
    $url = "https://github.com/${REPO}/releases/download/${tag}/${binaryName}"
    $checksumsUrl = "https://github.com/${REPO}/releases/download/${tag}/checksums.txt"

    Write-Host "Installing ${BINARY} ${tag} (${target})..."

    $tmpDir = Join-Path $env:TEMP "onchainos-install-$(Get-Random)"
    New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null

    try {
        # Download binary and checksums
        Invoke-WebRequest -Uri $url -OutFile "$tmpDir\$binaryName" -UseBasicParsing
        Invoke-WebRequest -Uri $checksumsUrl -OutFile "$tmpDir\checksums.txt" -UseBasicParsing

        # Verify checksum
        $checksumLine = Get-Content "$tmpDir\checksums.txt" | Where-Object { $_ -match $binaryName }
        if (-not $checksumLine) {
            Write-Host "Error: no checksum found for $binaryName" -ForegroundColor Red
            exit 1
        }
        $expectedHash = ($checksumLine -split '\s+')[0]
        $actualHash = (Get-FileHash "$tmpDir\$binaryName" -Algorithm SHA256).Hash.ToLower()

        if ($actualHash -ne $expectedHash) {
            Write-Host "Error: checksum mismatch!" -ForegroundColor Red
            Write-Host "  Expected: $expectedHash"
            Write-Host "  Got:      $actualHash"
            Write-Host "The downloaded file may have been tampered with. Aborting." -ForegroundColor Red
            exit 1
        }

        Write-Host "Checksum verified."

        # Install
        New-Item -ItemType Directory -Path $INSTALL_DIR -Force | Out-Null
        Copy-Item "$tmpDir\$binaryName" (Join-Path $INSTALL_DIR "$BINARY.exe") -Force

        Write-Host "Installed ${BINARY} ${tag} to ${INSTALL_DIR}\${BINARY}.exe"
    } finally {
        Remove-Item -Recurse -Force $tmpDir -ErrorAction SilentlyContinue
    }
}

function Ensure-InPath {
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($userPath -split ';' | Where-Object { $_ -eq $INSTALL_DIR }) {
        return
    }

    [Environment]::SetEnvironmentVariable("Path", "$INSTALL_DIR;$userPath", "User")
    $env:Path = "$INSTALL_DIR;$env:Path"

    Write-Host ""
    Write-Host "Added $INSTALL_DIR to your PATH."
    Write-Host "To start using '${BINARY}' now, open a new terminal window."
    Write-Host "Or run: `$env:Path = '$INSTALL_DIR;' + `$env:Path"
}

# ── Main ──
$localVer = Get-LocalVersion

# Fast path: already installed and checked recently
if ($localVer -and (Test-CacheFresh)) {
    Write-Host "${BINARY} ${localVer} is already installed (update check skipped, checked recently)."
    exit 0
}

# Fetch latest release tag
try {
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/${REPO}/releases/latest" -UseBasicParsing
    $tag = $release.tag_name
} catch {
    Write-Host "Error: could not determine latest release" -ForegroundColor Red
    exit 1
}

if (-not $tag) {
    Write-Host "Error: could not determine latest release" -ForegroundColor Red
    exit 1
}

$latestVer = Normalize-Tag $tag

if ($localVer -and ($localVer -eq $latestVer)) {
    Write-Host "${BINARY} ${localVer} is already up to date."
    Write-Cache
    exit 0
}

if ($localVer) {
    Write-Host "Upgrading ${BINARY} from ${localVer} to ${latestVer}..."
}

Install-Binary $tag
Write-Cache
Ensure-InPath
Write-Host "Run '${BINARY} --help' to get started."
