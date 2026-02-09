#Requires -Version 5.1
<#
.SYNOPSIS
    Tool CLI installer for Windows with animated progress.

.DESCRIPTION
    Downloads and installs Tool CLI from GitHub releases.

.PARAMETER Version
    Install specific version (default: latest)

.PARAMETER Prefix
    Installation prefix (default: $env:LOCALAPPDATA\Programs\tool)

.PARAMETER Uninstall
    Remove tool binary

.PARAMETER Check
    Verify existing installation

.PARAMETER NoModifyPath
    Skip PATH configuration

.PARAMETER Quiet
    Minimal output

.PARAMETER Force
    Overwrite without prompts

.EXAMPLE
    irm https://raw.githubusercontent.com/zerocore-ai/tool-cli/main/install.ps1 | iex

.EXAMPLE
    .\install.ps1 -Version 0.1.0

.EXAMPLE
    .\install.ps1 -Uninstall
#>

[CmdletBinding()]
param(
    [string]$Version = "",
    [string]$Prefix = "",
    [switch]$Uninstall,
    [switch]$Check,
    [switch]$NoModifyPath,
    [switch]$Quiet,
    [switch]$Force
)

#--------------------------------------------------------------------------------------------------
# Constants
#--------------------------------------------------------------------------------------------------

$Script:GITHUB_REPO = "zerocore-ai/tool-cli"
$Script:SCRIPT_VERSION = "0.1.0"
$Script:BINARY_NAME = "tool.exe"

if ([string]::IsNullOrEmpty($Prefix)) {
    $Prefix = Join-Path $env:LOCALAPPDATA "Programs\tool"
}

#--------------------------------------------------------------------------------------------------
# Terminal Detection
#--------------------------------------------------------------------------------------------------

$Script:HasColor = $true
$Script:HasUnicode = $true

# Check if running in a real terminal
if ($env:CI -or $env:GITHUB_ACTIONS -or !$Host.UI.SupportsVirtualTerminal) {
    $Script:HasColor = $false
}

# Enable virtual terminal processing for colors
if ($Script:HasColor) {
    try {
        $null = [Console]::OutputEncoding
        [Console]::OutputEncoding = [System.Text.Encoding]::UTF8
    } catch {
        $Script:HasUnicode = $false
    }
}

#--------------------------------------------------------------------------------------------------
# Colors & Symbols
#--------------------------------------------------------------------------------------------------

if ($Script:HasColor) {
    $Script:RED = "`e[31m"
    $Script:GREEN = "`e[32m"
    $Script:YELLOW = "`e[33m"
    $Script:BLUE = "`e[34m"
    $Script:MAGENTA = "`e[35m"
    $Script:CYAN = "`e[36m"
    $Script:BOLD = "`e[1m"
    $Script:DIM = "`e[2m"
    $Script:RESET = "`e[0m"
} else {
    $Script:RED = ""
    $Script:GREEN = ""
    $Script:YELLOW = ""
    $Script:BLUE = ""
    $Script:MAGENTA = ""
    $Script:CYAN = ""
    $Script:BOLD = ""
    $Script:DIM = ""
    $Script:RESET = ""
}

if ($Script:HasUnicode) {
    $Script:SYM_OK = [char]::ConvertFromUtf32(0x2713)      # ✓
    $Script:SYM_ERR = [char]::ConvertFromUtf32(0x2717)    # ✗
    $Script:SYM_WARN = [char]::ConvertFromUtf32(0x26A0)   # ⚠
    $Script:SYM_ARROW = [char]::ConvertFromUtf32(0x2192)  # →
    $Script:SYM_BULLET = [char]::ConvertFromUtf32(0x2022) # •
    $Script:SPINNER_FRAMES = @(
        [char]::ConvertFromUtf32(0x280B), # ⠋
        [char]::ConvertFromUtf32(0x2819), # ⠙
        [char]::ConvertFromUtf32(0x2839), # ⠹
        [char]::ConvertFromUtf32(0x2838), # ⠸
        [char]::ConvertFromUtf32(0x283C), # ⠼
        [char]::ConvertFromUtf32(0x2834), # ⠴
        [char]::ConvertFromUtf32(0x2826), # ⠦
        [char]::ConvertFromUtf32(0x2827), # ⠧
        [char]::ConvertFromUtf32(0x2807), # ⠇
        [char]::ConvertFromUtf32(0x280F)  # ⠏
    )
    $Script:BOX_TL = [char]::ConvertFromUtf32(0x256D) # ╭
    $Script:BOX_TR = [char]::ConvertFromUtf32(0x256E) # ╮
    $Script:BOX_BL = [char]::ConvertFromUtf32(0x2570) # ╰
    $Script:BOX_BR = [char]::ConvertFromUtf32(0x256F) # ╯
    $Script:BOX_H = [char]::ConvertFromUtf32(0x2500)  # ─
    $Script:BOX_V = [char]::ConvertFromUtf32(0x2502)  # │
    $Script:FILL = [char]::ConvertFromUtf32(0x2588)   # █
    $Script:EMPTY = [char]::ConvertFromUtf32(0x2591)  # ░
} else {
    $Script:SYM_OK = "+"
    $Script:SYM_ERR = "x"
    $Script:SYM_WARN = "!"
    $Script:SYM_ARROW = "->"
    $Script:SYM_BULLET = "*"
    $Script:SPINNER_FRAMES = @('|', '/', '-', '\')
    $Script:BOX_TL = "+"
    $Script:BOX_TR = "+"
    $Script:BOX_BL = "+"
    $Script:BOX_BR = "+"
    $Script:BOX_H = "-"
    $Script:BOX_V = "|"
    $Script:FILL = "#"
    $Script:EMPTY = "-"
}

#--------------------------------------------------------------------------------------------------
# Utility Functions
#--------------------------------------------------------------------------------------------------

function Write-Log {
    param([string]$Message)
    if (-not $Quiet) {
        Write-Host $Message
    }
}

function Write-LogNoNewline {
    param([string]$Message)
    if (-not $Quiet) {
        Write-Host $Message -NoNewline
    }
}

function Write-Err {
    param([string]$Message)
    Write-Host $Message -ForegroundColor Red
}

function Get-HorizontalLine {
    param([int]$Width = 64)
    return ($Script:BOX_H * $Width)
}

function Write-BoxTop {
    param([string]$Title, [int]$Width = 66)

    $inner = $Width - 2
    $padTotal = $inner - $Title.Length
    $padLeft = [math]::Floor($padTotal / 2)
    $padRight = $padTotal - $padLeft

    Write-Log "$($Script:CYAN)$($Script:BOX_TL)$(Get-HorizontalLine -Width $inner)$($Script:BOX_TR)$($Script:RESET)"
    Write-Log "$($Script:CYAN)$($Script:BOX_V)$($Script:RESET)$(' ' * $padLeft)$($Script:CYAN)$($Script:BOLD)$Title$($Script:RESET)$(' ' * $padRight)$($Script:CYAN)$($Script:BOX_V)$($Script:RESET)"
    Write-Log "$($Script:CYAN)$($Script:BOX_BL)$(Get-HorizontalLine -Width $inner)$($Script:BOX_BR)$($Script:RESET)"
}

function Write-StepOk {
    param([string]$Message)
    Write-Log "  $($Script:GREEN)$($Script:SYM_OK)$($Script:RESET) $Message"
}

function Write-StepErr {
    param([string]$Message)
    Write-Err "  $($Script:RED)$($Script:SYM_ERR)$($Script:RESET) $Message"
}

function Write-StepWarn {
    param([string]$Message)
    Write-Log "  $($Script:YELLOW)$($Script:SYM_WARN)$($Script:RESET) $Message"
}

function Write-StepInfo {
    param([string]$Message)
    Write-Log "  $($Script:BLUE)$($Script:SYM_BULLET)$($Script:RESET) $Message"
}

#--------------------------------------------------------------------------------------------------
# Spinner Animation
#--------------------------------------------------------------------------------------------------

function Invoke-WithSpinner {
    param(
        [string]$Message,
        [scriptblock]$ScriptBlock
    )

    if ($Quiet) {
        & $ScriptBlock | Out-Null
        return $LASTEXITCODE -eq 0 -or $?
    }

    $job = Start-Job -ScriptBlock $ScriptBlock

    $i = 0
    $frameCount = $Script:SPINNER_FRAMES.Count

    while ($job.State -eq 'Running') {
        $frame = $Script:SPINNER_FRAMES[$i % $frameCount]
        Write-Host "`r  $($Script:CYAN)$frame$($Script:RESET) $Message" -NoNewline
        $i++
        Start-Sleep -Milliseconds 80
    }

    $result = Receive-Job -Job $job
    $success = $job.State -eq 'Completed' -and $job.ChildJobs[0].Error.Count -eq 0
    Remove-Job -Job $job

    # Clear line
    Write-Host "`r$(' ' * ($Message.Length + 10))" -NoNewline
    Write-Host "`r" -NoNewline

    if ($success) {
        Write-StepOk $Message
    } else {
        Write-StepErr $Message
    }

    return $success
}

#--------------------------------------------------------------------------------------------------
# Platform Detection
#--------------------------------------------------------------------------------------------------

function Get-Platform {
    $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture

    switch ($arch) {
        "X64" { return "windows-x86_64" }
        "Arm64" { return "windows-aarch64" }
        default {
            Write-Err "Unsupported architecture: $arch"
            exit 1
        }
    }
}

#--------------------------------------------------------------------------------------------------
# Version Detection
#--------------------------------------------------------------------------------------------------

function Get-LatestVersion {
    $url = "https://api.github.com/repos/$($Script:GITHUB_REPO)/releases/latest"

    try {
        $response = Invoke-RestMethod -Uri $url -UseBasicParsing
        $version = $response.tag_name -replace '^v', ''
        return $version
    } catch {
        Write-Err "Failed to fetch latest version: $_"
        exit 1
    }
}

#--------------------------------------------------------------------------------------------------
# Download Functions
#--------------------------------------------------------------------------------------------------

function Get-Release {
    param(
        [string]$ReleaseVersion,
        [string]$Platform,
        [string]$DestDir
    )

    $archive = "tool-$ReleaseVersion-$Platform.tar.gz"
    $url = "https://github.com/$($Script:GITHUB_REPO)/releases/download/v$ReleaseVersion/$archive"
    $destFile = Join-Path $DestDir $archive

    Write-Log "  $($Script:CYAN)$([char]0x2193)$($Script:RESET) Downloading $archive"

    try {
        # Get file size
        $response = Invoke-WebRequest -Uri $url -Method Head -UseBasicParsing
        $totalBytes = [long]$response.Headers['Content-Length']

        # Download with progress
        $webClient = New-Object System.Net.WebClient
        $completed = $false

        $progressHandler = {
            param($sender, $e)
            $percent = $e.ProgressPercentage
            $received = $e.BytesReceived
            $total = $e.TotalBytesToReceive

            $filled = [math]::Floor($percent * 40 / 100)
            $empty = 40 - $filled

            $bar = ($Script:FILL * $filled) + ($Script:EMPTY * $empty)
            $recMB = "{0:N1}" -f ($received / 1MB)
            $totMB = "{0:N1}" -f ($total / 1MB)

            Write-Host "`r    [$($Script:GREEN)$bar$($Script:RESET)] $($percent.ToString().PadLeft(3))% ${recMB}M/${totMB}M" -NoNewline
        }

        $completedHandler = {
            param($sender, $e)
            $script:completed = $true
        }

        $webClient.add_DownloadProgressChanged($progressHandler)
        $webClient.add_DownloadFileCompleted($completedHandler)

        $task = $webClient.DownloadFileTaskAsync($url, $destFile)
        while (-not $task.IsCompleted) {
            Start-Sleep -Milliseconds 100
        }

        if ($task.IsFaulted) {
            throw $task.Exception
        }

        Write-Host ""
        Write-StepOk "Downloaded $archive"

        return $archive
    } catch {
        Write-Host ""
        Write-StepErr "Download failed: $_"
        exit 1
    }
}

function Get-Checksum {
    param(
        [string]$ReleaseVersion,
        [string]$Archive,
        [string]$DestDir
    )

    $checksumFile = "$Archive.sha256"
    $url = "https://github.com/$($Script:GITHUB_REPO)/releases/download/v$ReleaseVersion/$checksumFile"
    $destFile = Join-Path $DestDir $checksumFile

    try {
        Invoke-WebRequest -Uri $url -OutFile $destFile -UseBasicParsing
        $content = Get-Content $destFile -Raw
        $hash = ($content -split '\s+')[0]
        return $hash.ToLower()
    } catch {
        return ""
    }
}

function Test-Checksum {
    param(
        [string]$FilePath,
        [string]$ExpectedHash
    )

    $actualHash = (Get-FileHash -Path $FilePath -Algorithm SHA256).Hash.ToLower()

    if ($actualHash -eq $ExpectedHash) {
        return $true
    } else {
        Write-Err "Checksum mismatch:"
        Write-Err "  Expected: $ExpectedHash"
        Write-Err "  Actual:   $actualHash"
        return $false
    }
}

#--------------------------------------------------------------------------------------------------
# Installation Functions
#--------------------------------------------------------------------------------------------------

function Expand-TarGz {
    param(
        [string]$Archive,
        [string]$DestDir
    )

    # Windows 10 1803+ has tar built-in
    $tarPath = Join-Path $env:SystemRoot "System32\tar.exe"

    if (Test-Path $tarPath) {
        Push-Location $DestDir
        & $tarPath -xzf $Archive 2>$null
        $result = $LASTEXITCODE -eq 0
        Pop-Location
        return $result
    } else {
        Write-StepErr "tar.exe not found. Windows 10 1803 or later required."
        return $false
    }
}

function Install-Binary {
    param(
        [string]$SourceDir,
        [string]$DestDir
    )

    if (-not (Test-Path $DestDir)) {
        New-Item -ItemType Directory -Path $DestDir -Force | Out-Null
    }

    $binDir = Join-Path $DestDir "bin"
    if (-not (Test-Path $binDir)) {
        New-Item -ItemType Directory -Path $binDir -Force | Out-Null
    }

    $source = Join-Path $SourceDir $Script:BINARY_NAME
    $dest = Join-Path $binDir $Script:BINARY_NAME

    if (Test-Path $source) {
        Copy-Item -Path $source -Destination $dest -Force
        return $true
    } else {
        Write-StepErr "Binary not found in archive"
        return $false
    }
}

function Test-Installation {
    param([string]$BinDir)

    $binary = Join-Path $BinDir $Script:BINARY_NAME

    if (Test-Path $binary) {
        try {
            $null = & $binary --version 2>&1
            return $true
        } catch {
            return $true  # Binary exists, may not have --version
        }
    }

    return $false
}

#--------------------------------------------------------------------------------------------------
# PATH Configuration
#--------------------------------------------------------------------------------------------------

function Add-ToPath {
    param([string]$BinDir)

    $currentPath = [Environment]::GetEnvironmentVariable("Path", "User")

    if ($currentPath -split ';' -contains $BinDir) {
        Write-StepOk "$BinDir already in PATH"
        return
    }

    if ($NoModifyPath) {
        Write-StepWarn "$BinDir not in PATH"
        Write-StepInfo "Add it manually to your PATH environment variable"
        return
    }

    Write-Log ""
    Write-LogNoNewline "  Add $($Script:CYAN)$BinDir$($Script:RESET) to PATH? [Y/n] "

    if ($Force) {
        Write-Log "y (--Force)"
        $reply = "y"
    } else {
        $reply = Read-Host
        if ([string]::IsNullOrWhiteSpace($reply)) {
            $reply = "y"
        }
    }

    if ($reply -match '^[Nn]') {
        Write-StepInfo "Skipped PATH configuration"
        Write-StepInfo "Add manually: `$env:Path += `";$BinDir`""
    } else {
        $newPath = "$currentPath;$BinDir"
        [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
        $env:Path = "$env:Path;$BinDir"
        Write-StepOk "Added to PATH"
        Write-StepInfo "Restart your terminal for changes to take effect"
    }
}

#--------------------------------------------------------------------------------------------------
# Uninstall
#--------------------------------------------------------------------------------------------------

function Invoke-Uninstall {
    Write-Log ""
    Write-BoxTop "Tool CLI Uninstaller"
    Write-Log ""

    $binDir = Join-Path $Prefix "bin"
    $binary = Join-Path $binDir $Script:BINARY_NAME
    $removed = $false

    if (Test-Path $binary) {
        Remove-Item -Path $binary -Force
        Write-StepOk "Removed $($Script:BINARY_NAME)"
        $removed = $true
    }

    # Optionally remove from PATH
    $currentPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($currentPath -split ';' -contains $binDir) {
        $newPath = ($currentPath -split ';' | Where-Object { $_ -ne $binDir }) -join ';'
        [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
        Write-StepOk "Removed $binDir from PATH"
    }

    # Remove empty directories
    if ((Test-Path $binDir) -and (Get-ChildItem $binDir | Measure-Object).Count -eq 0) {
        Remove-Item -Path $binDir -Force
    }
    if ((Test-Path $Prefix) -and (Get-ChildItem $Prefix | Measure-Object).Count -eq 0) {
        Remove-Item -Path $Prefix -Force
    }

    if (-not $removed) {
        Write-StepInfo "No tool-cli installation found in $binDir"
    } else {
        Write-Log ""
        Write-StepOk "Uninstall complete"
    }

    Write-Log ""
}

#--------------------------------------------------------------------------------------------------
# Check Installation
#--------------------------------------------------------------------------------------------------

function Invoke-Check {
    Write-Log ""
    Write-BoxTop "Tool CLI Installation Check"
    Write-Log ""

    $binDir = Join-Path $Prefix "bin"
    $binary = Join-Path $binDir $Script:BINARY_NAME
    $allOk = $true

    if (Test-Path $binary) {
        try {
            $ver = & $binary --version 2>&1 | Select-Object -First 1
            Write-StepOk "$($Script:BINARY_NAME) $($Script:DIM)($ver)$($Script:RESET)"
        } catch {
            Write-StepOk "$($Script:BINARY_NAME) $($Script:DIM)(version unknown)$($Script:RESET)"
        }
    } else {
        Write-StepErr "$($Script:BINARY_NAME) not found"
        $allOk = $false
    }

    Write-Log ""

    # Check PATH
    $currentPath = $env:Path -split ';'
    if ($currentPath -contains $binDir) {
        Write-StepOk "$binDir is in PATH"
    } else {
        Write-StepWarn "$binDir is not in PATH"
    }

    Write-Log ""

    return $allOk
}

#--------------------------------------------------------------------------------------------------
# Main Installation
#--------------------------------------------------------------------------------------------------

function Invoke-Install {
    Write-Log ""
    Write-BoxTop "Tool CLI Installer"
    Write-Log ""

    $platform = Get-Platform
    Write-StepInfo "Platform: $($Script:BOLD)$platform$($Script:RESET)"

    # Get version
    if ([string]::IsNullOrEmpty($Version)) {
        Write-LogNoNewline "  $($Script:BLUE)$($Script:SYM_BULLET)$($Script:RESET) Fetching latest version..."
        $Version = Get-LatestVersion
        Write-Host "`r$(' ' * 50)" -NoNewline
        Write-Host "`r" -NoNewline
        Write-StepInfo "Version:  $($Script:BOLD)$Version$($Script:RESET)"
    } else {
        Write-StepInfo "Version:  $($Script:BOLD)$Version$($Script:RESET)"
    }

    Write-Log ""

    # Create temp directory
    $tempDir = Join-Path ([System.IO.Path]::GetTempPath()) "tool-cli-install-$([System.Guid]::NewGuid().ToString('N').Substring(0, 8))"
    New-Item -ItemType Directory -Path $tempDir -Force | Out-Null

    try {
        # Download
        $archive = Get-Release -ReleaseVersion $Version -Platform $platform -DestDir $tempDir
        $archivePath = Join-Path $tempDir $archive

        # Checksum
        $expectedHash = Get-Checksum -ReleaseVersion $Version -Archive $archive -DestDir $tempDir
        if (-not [string]::IsNullOrEmpty($expectedHash)) {
            $checksumOk = Invoke-WithSpinner -Message "Verifying checksum" -ScriptBlock {
                Test-Checksum -FilePath $using:archivePath -ExpectedHash $using:expectedHash
            }
            if (-not $checksumOk) {
                exit 1
            }
        } else {
            Write-StepWarn "Checksum not available, skipping verification"
        }

        # Extract
        $extractOk = Invoke-WithSpinner -Message "Extracting files" -ScriptBlock {
            Expand-TarGz -Archive $using:archivePath -DestDir $using:tempDir
        }
        if (-not $extractOk) {
            exit 1
        }

        # Install
        $installOk = Invoke-WithSpinner -Message "Installing binary" -ScriptBlock {
            Install-Binary -SourceDir $using:tempDir -DestDir $using:Prefix
        }
        if (-not $installOk) {
            exit 1
        }

        # Verify
        $binDir = Join-Path $Prefix "bin"
        $verifyOk = Invoke-WithSpinner -Message "Verifying installation" -ScriptBlock {
            Test-Installation -BinDir $using:binDir
        }
        if (-not $verifyOk) {
            exit 1
        }

        # PATH configuration
        Add-ToPath -BinDir $binDir

        # Summary
        Write-Log ""
        Write-Log "  $($Script:GREEN)$($Script:SYM_OK)$($Script:RESET) $($Script:BOLD)Installation complete$($Script:RESET)"
        Write-Log ""

        $binary = Join-Path $binDir $Script:BINARY_NAME
        try {
            $ver = & $binary --version 2>&1 | Select-Object -First 1
        } catch {
            $ver = ""
        }
        Write-Log "    $($Script:SYM_BULLET) $($Script:BOLD)tool$($Script:RESET) $($Script:DIM)$binary$($Script:RESET) $($Script:DIM)$ver$($Script:RESET)"
        Write-Log ""
        Write-Log "  Run $($Script:CYAN)tool --help$($Script:RESET) or $($Script:CYAN)tool --tree$($Script:RESET) to get started."
        Write-Log ""

    } finally {
        # Cleanup temp directory
        if (Test-Path $tempDir) {
            Remove-Item -Path $tempDir -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}

#--------------------------------------------------------------------------------------------------
# Entry Point
#--------------------------------------------------------------------------------------------------

if ($Uninstall) {
    Invoke-Uninstall
} elseif ($Check) {
    Invoke-Check
} else {
    Invoke-Install
}
