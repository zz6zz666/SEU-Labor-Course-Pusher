# Builds a single self-contained Windows executable.
# The tray, toast and autostart implementations are pure Go, so CGO is off.
param(
    [string]$Version = "1.1.1",
    [string]$Output = "release"
)

$ErrorActionPreference = "Stop"

$go = (Get-Command go -ErrorAction SilentlyContinue).Source
if (-not $go) {
    $go = @(
        (Join-Path $env:ProgramFiles "Go\bin\go.exe"),
        (Join-Path $env:LOCALAPPDATA "Programs\Go\bin\go.exe"),
        "C:\Go\bin\go.exe"
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
}
if (-not $go) {
    throw "未找到 go，请安装 Go 1.24+ 或将其加入 PATH。"
}

$env:CGO_ENABLED = "0"
New-Item -ItemType Directory -Path $Output -Force | Out-Null

# -H=windowsgui: GUI subsystem, so the resident app shows no console window.
& $go build -trimpath -ldflags "-s -w -H=windowsgui -X main.version=$Version" -o "$Output\seu-labor.exe" ./cmd/seu-labor

$size = "{0:N2} MB" -f ((Get-Item "$Output\seu-labor.exe").Length / 1MB)
Write-Host "built $Output\seu-labor.exe ($size)"
