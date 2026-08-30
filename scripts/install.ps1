<#
install.ps1  --  remote installer for the Akkhara ("akk") interpreter.

```
Usage:
    irm https://raw.githubusercontent.com/XhaStudio/akkhara/main/scripts/install.ps1 | iex

What this does:
  1. Detects your CPU architecture.
  2. Downloads the matching prebuilt "akk.exe" from the latest
     GitHub release.
  3. Installs it to $AkkInstallDir
     (default: %LOCALAPPDATA%\Akkhara\bin).
  4. Adds that folder to your User PATH if it isn't already there.
  5. Runs a quick smoke test.

Env vars you can override before piping into iex:
    $env:AKK_REPO        "owner/repo"
                         (default: XhaStudio/akkhara)
    $env:AKK_VERSION     a release tag
                         (default: latest)
    $env:AKK_INSTALL_DIR install directory
                         (default: %LOCALAPPDATA%\Akkhara\bin)

NOTE:
This script downloads a prebuilt binary.
```

#>

$ErrorActionPreference = "Stop"

try {
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
} catch {}

function Write-Step {
param([string]$Text)
Write-Host ""
Write-Host "==> $Text" -ForegroundColor Cyan
}

function Write-Ok {
param([string]$Text)
Write-Host "    [OK] $Text" -ForegroundColor Green
}

function Write-Warn2 {
param([string]$Text)
Write-Host "    [!] $Text" -ForegroundColor Yellow
}

function Write-Fail {
param([string]$Text)
Write-Host "    [FAILED] $Text" -ForegroundColor Red
exit 1
}

# ---------------------------------------------------------------------

# Config

# ---------------------------------------------------------------------

$Repo = if ($env:AKK_REPO) {
$env:AKK_REPO
} else {
"XhaStudio/Akkhara-Programming-Language"
}

$Version = if ($env:AKK_VERSION) {
$env:AKK_VERSION
} else {
"latest"
}

$InstallDir = if ($env:AKK_INSTALL_DIR) {
$env:AKK_INSTALL_DIR
} else {
Join-Path $env:LOCALAPPDATA "Akkhara\bin"
}

$BinName = "akk.exe"

Write-Host "Akkhara installer (Windows)" -ForegroundColor Magenta

# ---------------------------------------------------------------------

# 1. Detect architecture

# ---------------------------------------------------------------------

Write-Step "Detecting platform"

$arch = $env:PROCESSOR_ARCHITECTURE

switch ($arch) {
"AMD64" {
$target = "x86_64-pc-windows-msvc"
}

```
"ARM64" {
    Write-Fail "arm64 Windows builds aren't published yet."
}

default {
    Write-Fail "unsupported architecture: $arch"
}
```

}

Write-Ok "Detected $target"

# ---------------------------------------------------------------------

# 2. Resolve download URL from GitHub Releases

# ---------------------------------------------------------------------

Write-Step "Looking up release"

$apiUrl = if ($Version -eq "latest") {
"https://api.github.com/repos/$Repo/releases/latest"
} else {
"https://api.github.com/repos/$Repo/releases/tags/$Version"
}

try {
$release = Invoke-RestMethod `        -Uri $apiUrl`
-Headers @{ "User-Agent" = "akkhara-installer" }
} catch {
Write-Fail "could not reach $apiUrl : $_"
}

$assetName = "akk-$target.zip"

$asset = $release.assets |
Where-Object { $_.name -eq $assetName } |
Select-Object -First 1

if (-not $asset) {
Write-Fail "no asset named '$assetName' found in release '$($release.tag_name)'. Check https://github.com/$Repo/releases"
}

$downloadUrl = $asset.browser_download_url

Write-Ok "Found $assetName ($($release.tag_name))"

# ---------------------------------------------------------------------

# 3. Download, extract, install

# ---------------------------------------------------------------------

Write-Step "Downloading akk"

$tmpDir = Join-Path `    $env:TEMP`
("akkhara_install_" + [System.Guid]::NewGuid().ToString("N"))

New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null

$zipPath = Join-Path $tmpDir $assetName

try {
Invoke-WebRequest `        -Uri $downloadUrl`
-OutFile $zipPath `
-UseBasicParsing
} catch {
Write-Fail "download failed: $_"
}

Write-Ok "Downloaded $assetName"

Write-Step "Extracting and installing to $InstallDir"

Expand-Archive `    -Path $zipPath`
-DestinationPath $tmpDir `
-Force

$exeSource = Join-Path $tmpDir $BinName

if (-not (Test-Path $exeSource)) {
Write-Fail "extracted archive did not contain $BinName"
}

New-Item `    -ItemType Directory`
-Path $InstallDir `
-Force | Out-Null

Copy-Item `    -Path $exeSource`
-Destination (Join-Path $InstallDir $BinName) `
-Force

Remove-Item `    -Path $tmpDir`
-Recurse `    -Force`
-ErrorAction SilentlyContinue

Write-Ok "Installed to $InstallDir$BinName"

# ---------------------------------------------------------------------

# 4. Add to User PATH if needed

# ---------------------------------------------------------------------

Write-Step "Checking PATH"

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")

if ($userPath -split ";" | Where-Object { $_ -eq $InstallDir }) {

```
Write-Ok "$InstallDir is already on your PATH"
```

} else {

```
$newPath = if ($userPath) {
    "$userPath;$InstallDir"
} else {
    $InstallDir
}

[Environment]::SetEnvironmentVariable(
    "Path",
    $newPath,
    "User"
)

$env:Path += ";$InstallDir"

Write-Ok "Added $InstallDir to your User PATH"

Write-Warn2 "Restart your terminal for this to apply everywhere"
```

}

# ---------------------------------------------------------------------

# 5. Smoke test

# ---------------------------------------------------------------------

Write-Step "Verifying install"

try {

```
& (Join-Path $InstallDir $BinName) --version | Out-Null

Write-Ok "akk runs correctly"
```

} catch {

```
Write-Warn2 "installed but 'akk --version' didn't run cleanly -- check manually"
```

}

# ---------------------------------------------------------------------

# Complete

# ---------------------------------------------------------------------

Write-Step "Install complete"

Write-Host "    Run:  akk myprogram.akk" -ForegroundColor Green
Write-Host "    (Open a new terminal first if PATH was just updated.)"
