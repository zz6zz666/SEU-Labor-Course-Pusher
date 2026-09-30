# Per-user uninstall for SEU 劳动教育课程推送助手.
# User data in %APPDATA%\SEU劳动教育课程推送助手 is preserved.
$ErrorActionPreference = "SilentlyContinue"
$AppName = "SEU劳动教育助手"
$Root = Join-Path $env:LOCALAPPDATA "Programs\$AppName"

Get-Process seu-labor | Stop-Process -Force
Get-CimInstance Win32_Process -Filter "Name='msedge.exe'" |
    Where-Object { $_.CommandLine -match 'browser-profile|wizard-webview' } |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force }

Remove-Item (Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\$AppName.lnk") -Force
Remove-Item "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\SEULaborPusher" -Recurse -Force
Remove-Item $Root -Recurse -Force

Write-Host "已卸载(用户数据保留在 %APPDATA%\SEU劳动教育课程推送助手)"
