[CmdletBinding()]
param(
    [string]$Version = "",
    [string]$InstallDir = "",
    [switch]$NoSkill
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$Repo = "hyperb1iss/unifly"
$Binary = "unifly.exe"
$Asset = "unifly-windows-amd64.exe"

function Write-Info([string]$Message) {
    Write-Host ":: $Message" -ForegroundColor Cyan
}

function Write-Ok([string]$Message) {
    Write-Host ":: $Message" -ForegroundColor Green
}

function Write-Warn([string]$Message) {
    Write-Host ":: $Message" -ForegroundColor Yellow
}

function Get-LatestVersion {
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
    if (-not $release.tag_name) {
        throw "Failed to fetch the latest release tag from GitHub."
    }

    return [string]$release.tag_name
}

function Get-InstallDir {
    if ($InstallDir) {
        return $InstallDir
    }

    return Join-Path $env:LOCALAPPDATA "unifly\bin"
}

function Ensure-PathContains([string]$PathEntry) {
    $currentPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $entries = @()

    if ($currentPath) {
        $entries = $currentPath.Split(';', [System.StringSplitOptions]::RemoveEmptyEntries)
    }

    if ($entries -contains $PathEntry) {
        return $false
    }

    $updatedEntries = @($entries + $PathEntry | Select-Object -Unique)
    [Environment]::SetEnvironmentVariable("Path", ($updatedEntries -join ';'), "User")
    return $true
}

function Install-Unifly([string]$ResolvedVersion, [string]$TargetDir) {
    $downloadUrl = "https://github.com/$Repo/releases/download/$ResolvedVersion/$Asset"
    $tempFile = Join-Path ([System.IO.Path]::GetTempPath()) ("unifly-" + [System.Guid]::NewGuid() + ".exe")
    $destination = Join-Path $TargetDir $Binary

    New-Item -ItemType Directory -Force -Path $TargetDir | Out-Null

    Write-Info "Downloading $Asset $ResolvedVersion..."
    Invoke-WebRequest -Uri $downloadUrl -OutFile $tempFile

    Write-Info "Installing to $destination..."
    Move-Item -Force $tempFile $destination

    return $destination
}

# Install the agent skill for the current user. Package managers cannot
# reach a profile directory, but this script runs as the user. Pass
# -NoSkill or set UNIFLY_NO_SKILL=1 to skip it. Native stderr must not
# trip $ErrorActionPreference = "Stop" on Windows PowerShell 5.1.
function Install-AgentSkill([string]$BinaryPath) {
    if ($NoSkill -or $env:UNIFLY_NO_SKILL) {
        return
    }

    $previousPreference = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    $succeeded = $false
    $output = @()
    try {
        # Releases before `unifly skill --install` cannot do this.
        $help = (& $BinaryPath skill --help 2>$null) -join "`n"
        if ($help -notmatch "--install") {
            return
        }
        $output = @(& $BinaryPath skill --install 2>&1)
        $succeeded = $LASTEXITCODE -eq 0
    } catch {
        $succeeded = $false
    } finally {
        $ErrorActionPreference = $previousPreference
    }

    if ($succeeded) {
        foreach ($line in $output) {
            if ($line -is [System.Management.Automation.ErrorRecord]) {
                Write-Ok "$line"
            }
        }
    } else {
        Write-Warn "Could not install the agent skill; run 'unifly skill --install' to see why."
    }
}

Write-Host ""
Write-Host "  unifly installer" -ForegroundColor Magenta
Write-Host ""

$resolvedVersion = if ($Version) { $Version } else { Get-LatestVersion }
$resolvedInstallDir = Get-InstallDir
$binaryPath = Install-Unifly -ResolvedVersion $resolvedVersion -TargetDir $resolvedInstallDir
$pathUpdated = Ensure-PathContains -PathEntry $resolvedInstallDir
$installedVersion = & $binaryPath --version

Write-Ok "Installed $installedVersion"
Install-AgentSkill -BinaryPath $binaryPath

if ($pathUpdated) {
    Write-Warn "Added $resolvedInstallDir to your user PATH."
    Write-Warn "Open a new terminal before running unifly from anywhere."
}

Write-Host ""
Write-Host "  Get started:" -ForegroundColor Cyan
Write-Host "    unifly config init    # Set up your controller"
Write-Host "    unifly devices list   # List network devices"
Write-Host "    unifly tui            # Launch the dashboard"
Write-Host ""
