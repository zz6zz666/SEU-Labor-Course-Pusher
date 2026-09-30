# Builds the app and compiles a single-file setup.exe with Inno Setup 6.
# Output: release\seu-labor-setup-<version>.exe
param(
    [string]$Version = "1.2.0",
    [string]$Output = "release"
)

$ErrorActionPreference = "Stop"
$root = $PSScriptRoot

Write-Host "==> building app ($Version)"
& (Join-Path $root "build.ps1") -Version $Version -Output $Output

$candidates = @(
    (Join-Path $env:LOCALAPPDATA "Programs\Inno Setup 6\ISCC.exe"),
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe"
)
$iscc = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) {
    throw "未找到 Inno Setup 的 ISCC.exe，请先安装 Inno Setup 6（winget install JRSoftware.InnoSetup）。"
}

Write-Host "==> compiling installer with $iscc"
& $iscc "/DMyAppVersion=$Version" (Join-Path $root "installer\seu-labor.iss")
if ($LASTEXITCODE -ne 0) {
    throw "Inno Setup 编译失败（退出码 $LASTEXITCODE）"
}

$setup = Join-Path $root "$Output\seu-labor-setup-$Version.exe"
if (-not (Test-Path $setup)) {
    throw "未生成安装包: $setup"
}
$size = "{0:N2} MB" -f ((Get-Item $setup).Length / 1MB)
Write-Host "built $setup ($size)"
