#Requires -Version 5.1
<#
.SYNOPSIS
    netdx installer for Windows.

.DESCRIPTION
    Downloads the latest netdx release zip for the detected architecture,
    extracts netdx.exe into $env:LOCALAPPDATA\netdx\bin, adds that directory
    to the user PATH if needed, and verifies the install.

.EXAMPLE
    irm https://github.com/duncanunderwood/netdx/releases/latest/download/install.ps1 | iex
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Write-NetdxInfo {
    param([string]$Message)
    Write-Host "netdx: $Message"
}

function Write-NetdxError {
    param([string]$Message)
    Write-Error "netdx: error: $Message"
    exit 1
}

$Repo = 'duncanunderwood/netdx'
$InstallDir = Join-Path $env:LOCALAPPDATA 'netdx\bin'

# --- detect architecture -----------------------------------------------------

$archRaw = $env:PROCESSOR_ARCHITECTURE
if ([Environment]::Is64BitOperatingSystem -and $archRaw -eq 'ARM64') {
    $archRaw = 'ARM64'
}

switch ($archRaw) {
    'AMD64' { $target = 'x86_64-pc-windows-msvc' }
    'ARM64' { $target = 'aarch64-pc-windows-msvc' }
    default {
        Write-NetdxError "unsupported architecture '$archRaw'. netdx ships prebuilt binaries for x64 and arm64 Windows only. Try: cargo install netdx"
    }
}

$asset = "netdx-$target.zip"
$url = "https://github.com/$Repo/releases/latest/download/$asset"

Write-NetdxInfo "detected platform: $target"
Write-NetdxInfo "downloading $url"

# --- download and extract ---------------------------------------------------

$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $tmpDir | Out-Null
$zipPath = Join-Path $tmpDir $asset

try {
    try {
        Invoke-WebRequest -Uri $url -OutFile $zipPath -UseBasicParsing
    } catch {
        Write-NetdxError "download failed. Does a release exist for $target? ($url)`n$_"
    }

    $extractDir = Join-Path $tmpDir 'extracted'
    try {
        Expand-Archive -Path $zipPath -DestinationPath $extractDir -Force
    } catch {
        Write-NetdxError "failed to extract $asset`n$_"
    }

    $exe = Get-ChildItem -Path $extractDir -Filter 'netdx.exe' -Recurse | Select-Object -First 1
    if (-not $exe) {
        Write-NetdxError "archive did not contain a 'netdx.exe' binary"
    }

    # --- install -------------------------------------------------------------

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    $destExe = Join-Path $InstallDir 'netdx.exe'
    Copy-Item -Path $exe.FullName -Destination $destExe -Force

    Write-NetdxInfo "installed to $destExe"

    # --- update PATH (persisted, user scope) ----------------------------------

    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $pathEntries = @()
    if ($userPath) {
        $pathEntries = $userPath.Split(';') | Where-Object { $_ -ne '' }
    }

    if ($pathEntries -notcontains $InstallDir) {
        $newUserPath = if ($userPath) { "$userPath;$InstallDir" } else { $InstallDir }
        [Environment]::SetEnvironmentVariable('Path', $newUserPath, 'User')
        # make it usable in the current session too, without requiring a restart
        $env:Path = "$env:Path;$InstallDir"
        Write-NetdxInfo "added $InstallDir to your user PATH (open a new terminal for it to take effect everywhere)"
    } else {
        Write-NetdxInfo "$InstallDir is already on your PATH"
    }

    # --- verify ----------------------------------------------------------------

    try {
        $version = & $destExe --version
        Write-NetdxInfo "install verified: $version"
    } catch {
        Write-NetdxError "installed binary at $destExe failed to run 'netdx --version'`n$_"
    }

    Write-NetdxInfo "done. run 'netdx --help' to get started."
} finally {
    Remove-Item -Path $tmpDir -Recurse -Force -ErrorAction SilentlyContinue
}
