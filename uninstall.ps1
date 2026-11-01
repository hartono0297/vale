# Vale uninstaller.
#
#   irm https://raw.githubusercontent.com/hartono0297/vale/main/uninstall.ps1 | iex
#
# Your data (%APPDATA%\Vale: settings, notes, Google login) is kept.
# To delete it as well, set DI_PURGE=1 first:
#   $env:DI_PURGE = '1'; irm https://raw.githubusercontent.com/hartono0297/vale/main/uninstall.ps1 | iex
#
# Leftovers of Dynamic Island (Vale's name before the rename) are removed too.

$ErrorActionPreference = 'Stop'

$InstallDir = Join-Path $env:LOCALAPPDATA 'Programs\Vale'
$Exe = Join-Path $InstallDir 'vale.exe'
$Programs = [Environment]::GetFolderPath('Programs')
$Shortcut = Join-Path $Programs 'Vale.lnk'
$DataDir = Join-Path $env:APPDATA 'Vale'
$RunKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
# Before the rename.
$OldInstallDir = Join-Path $env:LOCALAPPDATA 'Programs\DynamicIsland'
$OldExe = Join-Path $OldInstallDir 'dynamic-island.exe'
$OldShortcut = Join-Path $Programs 'Dynamic Island.lnk'
$OldDataDir = Join-Path $env:APPDATA 'DynamicIsland'

function Say([string]$Message) { Write-Host "  $Message" }

Write-Host ''
Write-Host 'Vale - uninstall'

# 1. Remove the island's Claude Code hooks while an exe still exists (either
#    removes the hooks of both names).
$hookExe = @($Exe, $OldExe) | Where-Object { Test-Path $_ } | Select-Object -First 1
if ($hookExe) {
    Start-Process -FilePath $hookExe -ArgumentList '--uninstall-hooks' -Wait -WindowStyle Hidden
    Say 'Hook Claude Code milik island dihapus (kalau ada).'
}

# 2. Stop the island.
$running = Get-Process -Name 'vale', 'dynamic-island' -ErrorAction SilentlyContinue
if ($running) {
    $running | Stop-Process -Force
    Start-Sleep -Milliseconds 800
    Say 'Vale ditutup.'
}

# 3. Autostart entries (only when they point at the island).
foreach ($name in @('Vale', 'DynamicIsland')) {
    $run = Get-ItemProperty -Path $RunKey -Name $name -ErrorAction SilentlyContinue
    if ($run -and ($run.$name -match '(vale|dynamic-island)\.exe')) {
        Remove-ItemProperty -Path $RunKey -Name $name
        Say 'Autostart dihapus.'
    }
}

# 4. Shortcuts and program files.
foreach ($link in @($Shortcut, $OldShortcut)) {
    if (Test-Path $link) {
        Remove-Item -Path $link -Force
        Say 'Shortcut Start Menu dihapus.'
    }
}
foreach ($dir in @($InstallDir, $OldInstallDir)) {
    if (Test-Path $dir) {
        Remove-Item -Path $dir -Recurse -Force
        Say "Folder $dir dihapus."
    }
}

# 5. Data.
foreach ($dir in @($DataDir, $OldDataDir)) {
    if (-not (Test-Path $dir)) { continue }
    if ($env:DI_PURGE -eq '1') {
        Remove-Item -Path $dir -Recurse -Force
        Say "Data di $dir dihapus."
    } else {
        Say "Data (setting, notes, login Google) tetap disimpan di $dir"
        Say 'Hapus folder itu kalau tidak dipakai lagi.'
    }
}

Write-Host ''
Say 'Selesai.'
Write-Host ''
