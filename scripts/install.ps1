# codanna installer for Windows
# Usage: irm https://codanna.dev/install.ps1 | iex

$ErrorActionPreference = "Stop"

$Repo = "bartolli/codanna"
$InstallDir = if ($env:CODANNA_INSTALL_DIR) { $env:CODANNA_INSTALL_DIR } else { "$env:USERPROFILE\.local\bin" }

function Say($msg) { Write-Host "codanna: $msg" }
function Err($msg) { Write-Host "codanna: ERROR: $msg" -ForegroundColor Red; exit 1 }

# Detect platform
function Get-Platform {
    if ($env:PROCESSOR_ARCHITECTURE -eq "AMD64") {
        return "windows-x64"
    }
    # Fallback check for 64-bit PowerShell
    if ([Environment]::Is64BitOperatingSystem) {
        return "windows-x64"
    }
    
    Err "unsupported architecture: $env:PROCESSOR_ARCHITECTURE (only x64 is supported)"
}

# Get latest release tag
function Get-LatestVersion {
    $response = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
    return $response.tag_name
}

# Main
function Main {
    $platform = Get-Platform
    $version = if ($env:CODANNA_VERSION) { $env:CODANNA_VERSION } else { Get-LatestVersion }

    Say "installing codanna $version for $platform"

    # Fetch manifest
    $manifestUrl = "https://github.com/$Repo/releases/download/$version/dist-manifest.json"
    try {
        $manifest = Invoke-RestMethod -Uri $manifestUrl
    } catch {
        Err "failed to fetch manifest from $manifestUrl"
    }

    # Find matching artifact
    $artifact = $manifest.artifacts | Where-Object {
        $_.platform -eq $platform
    } | Select-Object -First 1

    if (-not $artifact) {
        Err "no artifact found for $platform"
    }

    $url = $artifact.url
    $sha256 = $artifact.sha256
    $filename = $artifact.name

    # Download
    $tmpDir = New-Item -ItemType Directory -Path "$env:TEMP\codanna_install_$([System.Guid]::NewGuid().ToString('N').Substring(0,8))"
    $downloadPath = Join-Path $tmpDir $filename

    Say "downloading $filename"
    try {
        Invoke-WebRequest -Uri $url -OutFile $downloadPath -UseBasicParsing
    } catch {
        Err "download failed: $_"
    }

    # Verify checksum
    Say "verifying checksum"
    $actualHash = (Get-FileHash -Path $downloadPath -Algorithm SHA256).Hash.ToLower()
    if ($actualHash -ne $sha256) {
        Err "checksum mismatch: expected $sha256, got $actualHash"
    }

    # Extract
    Say "extracting"
    $extractDir = Join-Path $tmpDir "extracted"
    Expand-Archive -Path $downloadPath -DestinationPath $extractDir -Force

    # Install
    if (-not (Test-Path $InstallDir)) {
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    }

    $binary = Get-ChildItem -Path $extractDir -Recurse -Filter "codanna.exe" | Select-Object -First 1
    if (-not $binary) {
        Err "codanna.exe not found in archive"
    }

    Copy-Item -Path $binary.FullName -Destination $InstallDir -Force
    Say "installed to $InstallDir\codanna.exe"

    # Cleanup
    Remove-Item -Path $tmpDir -Recurse -Force

    # PATH check
    $currentPath = [Environment]::GetEnvironmentVariable("PATH", "User")
    if ($currentPath -notlike "*$InstallDir*") {
        Say ""
        Say "To add codanna to your PATH, run:"
        Say ""
        Say "  `$env:PATH = `"$InstallDir;`$env:PATH`""
        Say ""
        Say "Or permanently (requires restart):"
        Say ""
        Say "  [Environment]::SetEnvironmentVariable('PATH', `"$InstallDir;`$env:PATH`", 'User')"
    }
}

Main
