# Per-user installer for SEU 劳动教育课程推送助手.
# No admin required: installs under %LOCALAPPDATA%\Programs and registers a
# Start Menu shortcut and an uninstall entry in HKCU.
param([switch]$NoLaunch)

$ErrorActionPreference = "Stop"
$AppName = "SEU劳动教育助手"
$Root = Join-Path $env:LOCALAPPDATA "Programs\$AppName"
$Exe = Join-Path $Root "seu-labor.exe"

$src = Join-Path $PSScriptRoot "seu-labor.exe"
if (-not (Test-Path $src)) {
    $src = Join-Path $PSScriptRoot "release\seu-labor.exe"
}
if (-not (Test-Path $src)) {
    Write-Host "未找到可执行文件,正在构建…"
    & (Join-Path $PSScriptRoot "build.ps1")
    $src = Join-Path $PSScriptRoot "release\seu-labor.exe"
}

New-Item -ItemType Directory -Path $Root -Force | Out-Null
Copy-Item $src $Exe -Force
Copy-Item (Join-Path $PSScriptRoot "assets\icon.ico") (Join-Path $Root "icon.ico") -Force
Copy-Item (Join-Path $PSScriptRoot "uninstall.ps1") $Root -Force

# Start Menu shortcut
$startMenu = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"
$lnk = Join-Path $startMenu "$AppName.lnk"
$ws = New-Object -ComObject WScript.Shell
$sc = $ws.CreateShortcut($lnk)
$sc.TargetPath = $Exe
$sc.WorkingDirectory = $Root
$sc.IconLocation = "$Root\icon.ico"
$sc.Description = "SEU 劳动教育课程推送助手"
$sc.Save()

# Uninstall entry (HKCU, per-user)
$version = (Get-Item $Exe).VersionInfo.ProductVersion
if (-not $version) { $version = "1.1.0" }
$uninstall = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\SEULaborPusher"
New-Item -Path $uninstall -Force | Out-Null
Set-ItemProperty $uninstall -Name DisplayName -Value $AppName
Set-ItemProperty $uninstall -Name DisplayVersion -Value $version
Set-ItemProperty $uninstall -Name Publisher -Value "zz6zz666"
Set-ItemProperty $uninstall -Name InstallLocation -Value $Root
Set-ItemProperty $uninstall -Name DisplayIcon -Value "$Root\icon.ico"
Set-ItemProperty $uninstall -Name UninstallString -Value "powershell -ExecutionPolicy Bypass -File `"$Root\uninstall.ps1`""
Set-ItemProperty $uninstall -Name NoModify -Value 1 -Type DWord
Set-ItemProperty $uninstall -Name NoRepair -Value 1 -Type DWord

Write-Host "已安装到 $Exe"
if (-not $NoLaunch) {
    Start-Process $Exe
    Write-Host "已启动,托盘见右下角图标。"
}
