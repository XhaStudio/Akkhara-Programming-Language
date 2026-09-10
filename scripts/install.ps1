<#
install.ps1  --  remote installer for the Akkhara ("akk") interpreter.

Usage:
    irm https://raw.githubusercontent.com/XhaStudio/Akkhara-Programming-Language/main/scripts/install.ps1 | iex

What this does:
  1. Detects your CPU architecture.
  2. Checks for the Visual C++ Redistributable (x64), which the akk.exe
     build requires at runtime, and installs it first if it's missing.
  3. Downloads the matching prebuilt "akk.exe" from the latest
     GitHub release (with a progress bar).
  4. Installs it to $InstallDir
     (default: %LOCALAPPDATA%\Akkhara\bin).
  5. Adds that folder to your User PATH if it isn't already there.
  6. Runs a quick smoke test.

Env vars you can override before piping into iex:
    $env:AKK_REPO        "owner/repo"
                         (default: XhaStudio/Akkhara-Programming-Language)
    $env:AKK_VERSION     a release tag
                         (default: latest)
    $env:AKK_INSTALL_DIR install directory
                         (default: %LOCALAPPDATA%\Akkhara\bin)
    $env:AKK_SKIP_VCREDIST
                         set to "1" to skip the VC++ Redistributable check

NOTE:
This script downloads a prebuilt binary.
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
# Download helper -- shows a live progress bar via Write-Progress.
# ---------------------------------------------------------------------
function Invoke-DownloadWithProgress {
    param(
        [Parameter(Mandatory = $true)][string]$Url,
        [Parameter(Mandatory = $true)][string]$OutFile,
        [string]$Activity = "Downloading"
    )

    $wc = New-Object System.Net.WebClient
    $wc.Headers.Add("User-Agent", "akkhara-installer")

    # Use uniquely-named script-scoped state so nested/sequential calls to
    # this function don't stomp on each other's event handlers.
    $state = [hashtable]::Synchronized(@{ Percent = 0; Done = $false; Error = $null })

    $progressSub = Register-ObjectEvent -InputObject $wc -EventName DownloadProgressChanged -MessageData $state -Action {
        $Event.MessageData.Percent = $EventArgs.ProgressPercentage
    }
    $completedSub = Register-ObjectEvent -InputObject $wc -EventName DownloadFileCompleted -MessageData $state -Action {
        $Event.MessageData.Done = $true
        if ($EventArgs.Error) { $Event.MessageData.Error = $EventArgs.Error }
    }

    try {
        $wc.DownloadFileAsync([Uri]$Url, $OutFile)

        while (-not $state.Done) {
            Write-Progress -Activity $Activity -Status "$($state.Percent)%" -PercentComplete $state.Percent
            Start-Sleep -Milliseconds 100
        }
        Write-Progress -Activity $Activity -Completed

        if ($state.Error) {
            throw $state.Error
        }
    } finally {
        Unregister-Event -SourceIdentifier $progressSub.Name -ErrorAction SilentlyContinue
        Unregister-Event -SourceIdentifier $completedSub.Name -ErrorAction SilentlyContinue
        Remove-Job -Id $progressSub.Id -Force -ErrorAction SilentlyContinue
        Remove-Job -Id $completedSub.Id -Force -ErrorAction SilentlyContinue
        $wc.Dispose()
    }
}

# ---------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------

$Repo = if ($env:AKK_REPO) { $env:AKK_REPO } else { "XhaStudio/Akkhara-Programming-Language" }

$Version = if ($env:AKK_VERSION) { $env:AKK_VERSION } else { "latest" }

$InstallDir = if ($env:AKK_INSTALL_DIR) { $env:AKK_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "Akkhara\bin" }

$BinName = "akk.exe"

Write-Host "Akkhara installer (Windows)" -ForegroundColor Magenta

# ---------------------------------------------------------------------
# 1. Detect architecture
# ---------------------------------------------------------------------

Write-Step "Detecting platform"

$arch = $env:PROCESSOR_ARCHITECTURE

switch ($arch) {
    "AMD64" { $target = "x86_64-pc-windows-msvc" }
    "ARM64" { Write-Fail "arm64 Windows builds aren't published yet." }
    default { Write-Fail "unsupported architecture: $arch" }
}

Write-Ok "Detected $target"

# ---------------------------------------------------------------------
# 2. Check for the Visual C++ Redistributable (x64)
# ---------------------------------------------------------------------

Write-Step "Checking for Visual C++ Redistributable (x64)"

function Test-VCRedistX64 {
    # The MSVC redist installer records itself under one of these keys
    # depending on version/toolset; check the common modern (v14, VS
    # 2015-2022) location first, then fall back to the older Wow6432Node
    # mirror some installers also populate.
    $candidateKeys = @(
        "HKLM:\SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\X64",
        "HKLM:\SOFTWARE\WOW6432Node\Microsoft\VisualStudio\14.0\VC\Runtimes\X64",
        "HKLM:\SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64"
    )

    foreach ($key in $candidateKeys) {
        if (Test-Path $key) {
            $prop = Get-ItemProperty -Path $key -ErrorAction SilentlyContinue
            if ($prop -and $prop.Installed -eq 1) {
                return $true
            }
        }
    }
    return $false
}

if ($env:AKK_SKIP_VCREDIST -eq "1") {
    Write-Warn2 "Skipping VC++ Redistributable check (AKK_SKIP_VCREDIST=1)"
} elseif (Test-VCRedistX64) {
    Write-Ok "VC++ Redistributable (x64) is already installed"
} else {
    Write-Warn2 "VC++ Redistributable (x64) not found -- downloading it first"

    $vcUrl = "https://aka.ms/vs/17/release/vc_redist.x64.exe"
    $vcTmpDir = Join-Path $env:TEMP ("akkhara_vcredist_" + [System.Guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Path $vcTmpDir -Force | Out-Null
    $vcPath = Join-Path $vcTmpDir "vc_redist.x64.exe"

    try {
        Invoke-DownloadWithProgress -Url $vcUrl -OutFile $vcPath -Activity "Downloading VC++ Redistributable"
        Write-Ok "Downloaded VC++ Redistributable"
    } catch {
        Write-Warn2 "could not download VC++ Redistributable: $_"
        Write-Warn2 "continuing without it -- akk.exe may fail to run until it's installed manually"
        $vcPath = $null
    }

    if ($vcPath -and (Test-Path $vcPath)) {
        Write-Step "Installing VC++ Redistributable (this may prompt for admin rights)"
        try {
            $vcProc = Start-Process -FilePath $vcPath -ArgumentList "/install", "/quiet", "/norestart" -Wait -PassThru
            if ($vcProc.ExitCode -eq 0) {
                Write-Ok "VC++ Redistributable installed"
            } elseif ($vcProc.ExitCode -eq 3010) {
                Write-Ok "VC++ Redistributable installed (a restart is recommended)"
            } else {
                Write-Warn2 "VC++ Redistributable installer exited with code $($vcProc.ExitCode) -- continuing anyway"
            }
        } catch {
            Write-Warn2 "could not run the VC++ Redistributable installer: $_"
            Write-Warn2 "you may need to install it manually from $vcUrl"
        }
        Remove-Item -Path $vcTmpDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

# ---------------------------------------------------------------------
# 3. Resolve download URL from GitHub Releases
# ---------------------------------------------------------------------

Write-Step "Looking up release"

$apiUrl = if ($Version -eq "latest") {
    "https://api.github.com/repos/$Repo/releases/latest"
} else {
    "https://api.github.com/repos/$Repo/releases/tags/$Version"
}

try {
    $release = Invoke-RestMethod -Uri $apiUrl -Headers @{ "User-Agent" = "akkhara-installer" }
} catch {
    Write-Fail "could not reach $apiUrl : $_"
}

$assetName = "akk-$target.zip"

$asset = $release.assets | Where-Object { $_.name -eq $assetName } | Select-Object -First 1

if (-not $asset) {
    Write-Fail "no asset named '$assetName' found in release '$($release.tag_name)'. Check https://github.com/$Repo/releases"
}

$downloadUrl = $asset.browser_download_url

Write-Ok "Found $assetName ($($release.tag_name))"

# ---------------------------------------------------------------------
# 4. Download, extract, install
# ---------------------------------------------------------------------

Write-Step "Downloading akk"

$tmpDir = Join-Path $env:TEMP ("akkhara_install_" + [System.Guid]::NewGuid().ToString("N"))

New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null

$zipPath = Join-Path $tmpDir $assetName

try {
    Invoke-DownloadWithProgress -Url $downloadUrl -OutFile $zipPath -Activity "Downloading $assetName"
} catch {
    Write-Fail "download failed: $_"
}

Write-Ok "Downloaded $assetName"

Write-Step "Extracting and installing to $InstallDir"

Expand-Archive -Path $zipPath -DestinationPath $tmpDir -Force

$exeSource = Join-Path $tmpDir $BinName

if (-not (Test-Path $exeSource)) {
    Write-Fail "extracted archive did not contain $BinName"
}

New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null

$destPath = Join-Path $InstallDir $BinName

if (Test-Path $destPath) {
    # If akk.exe already exists, it may be the very process that invoked
    # this installer (e.g. via "akk update"), so it's still running and
    # its file content is locked -- Copy-Item -Force would fail with
    # "used by another process". Windows *does* allow renaming or deleting
    # a running executable's directory entry (just not overwriting its
    # content in place), so rename it out of the way first.
    Remove-Item -Path "$destPath.old" -Force -ErrorAction SilentlyContinue
    try {
        Rename-Item -Path $destPath -NewName "$BinName.old" -Force -ErrorAction Stop
    } catch {
        Write-Fail "could not replace $destPath -- it may be locked. Close any running 'akk' processes and try again."
    }
}

Copy-Item -Path $exeSource -Destination $destPath -Force

# Best-effort cleanup of the renamed-aside old binary. This can still fail
# if something else has it locked; that's fine, it'll just linger harmlessly
# and get cleaned up on the next update.
Remove-Item -Path "$destPath.old" -Force -ErrorAction SilentlyContinue

Remove-Item -Path $tmpDir -Recurse -Force -ErrorAction SilentlyContinue

Write-Ok "Installed to $destPath"

# akk itself also creates this on every run, but making it here too means
# it's visible right away instead of only after the first `akk` invocation.
New-Item -ItemType Directory -Path (Join-Path $InstallDir "libraries") -Force | Out-Null

# ---------------------------------------------------------------------
# 5. Add to User PATH if needed
# ---------------------------------------------------------------------

Write-Step "Checking PATH"

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")

if ($userPath -split ";" | Where-Object { $_ -eq $InstallDir }) {
    Write-Ok "$InstallDir is already on your PATH"
} else {
    $newPath = if ($userPath) { "$userPath;$InstallDir" } else { $InstallDir }

    [Environment]::SetEnvironmentVariable("Path", $newPath, "User")

    $env:Path += ";$InstallDir"

    Write-Ok "Added $InstallDir to your User PATH"
    Write-Warn2 "Restart your terminal for this to apply everywhere"
}

# ---------------------------------------------------------------------
# 6. Smoke test
# ---------------------------------------------------------------------

Write-Step "Verifying install"

try {
    $akkPath = Join-Path $InstallDir $BinName

    $process = Start-Process -FilePath $akkPath -ArgumentList "--version" -Wait -PassThru -NoNewWindow

    if ($process.ExitCode -eq 0) {
        Write-Ok "akk runs correctly"
    } else {
        Write-Warn2 "installed but 'akk --version' returned exit code $($process.ExitCode)"
    }
} catch {
    Write-Warn2 "installed but 'akk --version' didn't run cleanly -- check manually"
}

# ---------------------------------------------------------------------
# Complete
# ---------------------------------------------------------------------

Write-Step "Install complete"

Write-Host "    Run:  akk myprogram.akk" -ForegroundColor Green
Write-Host "    (Open a new terminal first if PATH was just updated.)"
