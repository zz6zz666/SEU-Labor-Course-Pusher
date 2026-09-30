# Builds a single self-contained Windows executable with Cargo.
# Toolchain: x86_64-pc-windows-gnu (rustup), no MSVC / CGO equivalent needed.
param(
    [string]$Version = "1.2.0",
    [string]$Output = "release"
)

$ErrorActionPreference = "Stop"

$cargo = (Get-Command cargo -ErrorAction SilentlyContinue).Source
if (-not $cargo) {
    $candidate = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
    if (Test-Path $candidate) { $cargo = $candidate }
}
if (-not $cargo) {
    throw "未找到 cargo，请安装 Rust(https://rustup.rs) 或将其加入 PATH。"
}

$env:SEU_LABOR_VERSION = $Version
New-Item -ItemType Directory -Path $Output -Force | Out-Null

Write-Host "==> cargo build --release"
# cargo reports progress on stderr; don't let PowerShell turn that into a
# terminating error under Stop. The exit code is the real signal.
$prevEap = $ErrorActionPreference
$ErrorActionPreference = "Continue"
& $cargo build --release
$code = $LASTEXITCODE
$ErrorActionPreference = $prevEap
if ($code -ne 0) { throw "cargo build 失败(退出码 $code)" }

Copy-Item "target\release\seu-labor.exe" (Join-Path $Output "seu-labor.exe") -Force
Copy-Item "vendor\WebView2Loader.dll" (Join-Path $Output "WebView2Loader.dll") -Force
$size = "{0:N2} MB" -f ((Get-Item (Join-Path $Output "seu-labor.exe")).Length / 1MB)
Write-Host "built $Output\seu-labor.exe ($size)"
