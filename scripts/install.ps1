<#
.SYNOPSIS
  Install, update or remove Rusty Inbox on Windows 10/11 (PowerShell 5+).
.EXAMPLE
  irm https://raw.githubusercontent.com/karem505/rusty-inbox/main/scripts/install.ps1 | iex
.EXAMPLE
  & ([scriptblock]::Create((irm https://raw.githubusercontent.com/karem505/rusty-inbox/main/scripts/install.ps1))) -Portable
#>
[CmdletBinding()]
param(
    [switch]$Portable,   # unzip into %LOCALAPPDATA%\Programs\Rusty Inbox instead of running the installer
    [switch]$Update,     # install only when the latest release is newer than the installed one
    [switch]$Uninstall,  # remove Rusty Inbox
    [string]$Version,    # a specific release (default: latest)
    [switch]$DryRun      # print what would happen, change nothing
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$Repo = 'karem505/rusty-inbox'
$InstallDir = Join-Path $env:LOCALAPPDATA 'Programs\Rusty Inbox'
$Exe = Join-Path $InstallDir 'rusty-inbox.exe'
$StartMenu = Join-Path ([Environment]::GetFolderPath('Programs')) 'Rusty Inbox.lnk'

function Write-Step($msg) { Write-Host "==> $msg" }

function Get-InstalledVersion {
    if (-not (Test-Path $Exe)) { return $null }
    try { return ((& $Exe --version) -replace '^rusty-inbox\s+', '').Trim() } catch { return $null }
}

function Get-LatestVersion {
    $rel = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest" -Headers @{ 'User-Agent' = 'rusty-inbox-installer' }
    return ($rel.tag_name -replace '^v', '')
}

function Stop-Running {
    Get-Process -Name 'rusty-inbox' -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -eq $Exe } | Stop-Process -Force -ErrorAction SilentlyContinue
}

function Remove-RustyInbox {
    Write-Step 'Removing Rusty Inbox'
    if ($DryRun) { Write-Host "[dry run] would uninstall from $InstallDir"; return }
    Stop-Running
    $unins = Get-ChildItem $InstallDir -Filter 'unins*.exe' -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($unins) {
        Start-Process $unins.FullName -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES' -Wait
    } elseif (Test-Path $InstallDir) {
        Remove-Item $InstallDir -Recurse -Force
    }
    if (Test-Path $StartMenu) { Remove-Item $StartMenu -Force }
    Write-Host 'Removed. Your mail data is kept in %LOCALAPPDATA%\rusty-inbox.'
}

if ($Uninstall) { Remove-RustyInbox; return }

$ver = if ($Version) { $Version -replace '^v', '' } elseif ($DryRun) { '<latest>' } else { Get-LatestVersion }

if ($Update) {
    $cur = Get-InstalledVersion
    if ($cur -and $cur -eq $ver) { Write-Host "Rusty Inbox $cur is up to date."; return }
    Write-Step "Updating $(if ($cur) { $cur } else { '(not installed)' }) -> $ver"
}

$base = "https://github.com/$Repo/releases/download/v$ver"
$assetName = if ($Portable) { "rusty-inbox-$ver-windows-x64.zip" } else { "rusty-inbox-$ver-windows-x64-setup.exe" }
$url = "$base/$assetName"

if ($DryRun) {
    Write-Host "[dry run] would download $url"
    if ($Portable) { Write-Host "[dry run] would unzip into $InstallDir and create $StartMenu" }
    else { Write-Host '[dry run] would run the installer silently (/VERYSILENT /SUPPRESSMSGBOXES /CURRENTUSER)' }
    return
}

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("rusty-inbox-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    $file = Join-Path $tmp $assetName
    Write-Step "Downloading $assetName"
    Invoke-WebRequest -Uri $url -OutFile $file -UseBasicParsing
    Stop-Running
    if ($Portable) {
        Write-Step "Unpacking into $InstallDir"
        if (Test-Path $InstallDir) { Remove-Item $InstallDir -Recurse -Force }
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
        Expand-Archive -Path $file -DestinationPath $InstallDir -Force
        $shell = New-Object -ComObject WScript.Shell
        $lnk = $shell.CreateShortcut($StartMenu)
        $lnk.TargetPath = $Exe
        $lnk.WorkingDirectory = $InstallDir
        $lnk.Save()
    } else {
        Write-Step 'Running the installer'
        $p = Start-Process $file -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/CURRENTUSER', '/NORESTART' -Wait -PassThru
        if ($p.ExitCode -ne 0) { throw "installer exited with code $($p.ExitCode)" }
    }
    Write-Host "Installed Rusty Inbox $ver to $InstallDir"
} finally {
    Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}
