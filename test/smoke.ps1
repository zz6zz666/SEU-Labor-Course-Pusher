# Smoke test for seu-labor: launches the real app, its tray and both
# settings-window engines, and checks they come up. Needs an interactive Windows
# desktop; the borrowed-engine and push checks additionally need an installed
# Chromium browser.
#
#   .\test\smoke.ps1
#   .\test\smoke.ps1 -SkipBrowsers      # only the tray + webview2 checks

[CmdletBinding()]
param(
    [string]$Exe = (Join-Path (Split-Path $PSScriptRoot -Parent) "release\seu-labor.exe"),
    [string]$Version = "1.2.0",
    [switch]$SkipBrowsers
)

$ErrorActionPreference = "Stop"
if (-not (Test-Path -LiteralPath $Exe)) {
    throw "executable not found: $Exe (run .\build.ps1 -Version $Version first)"
}
$Exe = (Resolve-Path -LiteralPath $Exe).Path

# The reusable crates (and the push self-test example) live in the sibling kit.
$kitRoot = Join-Path (Split-Path (Split-Path $PSScriptRoot -Parent) -Parent) "rust-webui-kit"

$fail = 0
function Check([string]$Name, [bool]$Ok) {
    if ($Ok) { Write-Host ("PASS  " + $Name) -ForegroundColor Green }
    else { Write-Host ("FAIL  " + $Name) -ForegroundColor Red; $script:fail++ }
}
function Stop-Apps {
    Get-Process seu-labor, msedge, chrome -ErrorAction SilentlyContinue |
        Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 500
}

Add-Type -Namespace Smoke -Name Native -MemberDefinition @'
public delegate bool EnumProc(System.IntPtr hWnd, System.IntPtr lParam);
[System.Runtime.InteropServices.DllImport("user32.dll")]
public static extern bool EnumWindows(EnumProc cb, System.IntPtr lParam);
[System.Runtime.InteropServices.DllImport("user32.dll", CharSet = System.Runtime.InteropServices.CharSet.Unicode)]
public static extern int GetClassName(System.IntPtr hWnd, System.Text.StringBuilder s, int n);
[System.Runtime.InteropServices.DllImport("user32.dll")]
public static extern bool IsWindowVisible(System.IntPtr hWnd);
[System.Runtime.InteropServices.DllImport("user32.dll")]
public static extern bool PostMessage(System.IntPtr hWnd, uint m, System.IntPtr w, System.IntPtr l);
'@

function Find-Class([string]$Class) {
    $script:smokeFound = [IntPtr]::Zero
    $cb = [Smoke.Native+EnumProc] {
        param($h, $l)
        $sb = New-Object System.Text.StringBuilder 256
        [void][Smoke.Native]::GetClassName($h, $sb, 256)
        if ($sb.ToString() -eq $Class) { $script:smokeFound = $h; return $false }
        return $true
    }
    [void][Smoke.Native]::EnumWindows($cb, [IntPtr]::Zero)
    return $script:smokeFound
}

# Isolated data dir so the test never touches the real profile.
$dataDir = Join-Path $env:TEMP "seu-labor-smoke"
Remove-Item -Recurse -Force $dataDir -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $dataDir | Out-Null
Set-Content -LiteralPath (Join-Path $dataDir "state.json") `
    -Value '{"pushedUniqueIds":[],"autoHandledIds":[],"authState":"unknown","firstRunCompleted":true}'
Set-Content -LiteralPath (Join-Path $dataDir "config.json") -Value '{}'
$env:SEU_DAEMON_DATA_DIR = $dataDir
$env:SEU_DAEMON_CONFIG = Join-Path $dataDir "config.json"
Remove-Item Env:\SEU_WIZARD_ENGINE -ErrorAction SilentlyContinue

Stop-Apps

Write-Host "`n== CLI ==" -ForegroundColor Cyan
$v = (cmd /c "`"$Exe`" -version") 2>&1 | Out-String
Check "-version prints $Version" ($v.Trim() -eq $Version)

$once = Start-Process $Exe -ArgumentList "-once" -PassThru -Wait
Check "-once exits cleanly" ($once.ExitCode -eq 0)

Write-Host "`n== tray ==" -ForegroundColor Cyan
$null = Start-Process $Exe -PassThru
Start-Sleep -Seconds 3
$tray = Find-Class "traykit_tray"
Check "tray icon window exists" ($tray -ne [IntPtr]::Zero)
if ($tray -ne [IntPtr]::Zero) {
    [void][Smoke.Native]::PostMessage($tray, 0x8001, [IntPtr]::Zero, [IntPtr]0x0205) # WM_RBUTTONUP
    Start-Sleep -Milliseconds 800
    Check "tray menu opens on right click" ([Smoke.Native]::IsWindowVisible((Find-Class "traykit_menu")))

    # Left click opens the course page in a borrowed browser window.
    [void][Smoke.Native]::PostMessage($tray, 0x8001, [IntPtr]::Zero, [IntPtr]0x0202) # WM_LBUTTONUP
    $coursePortFile = Join-Path $dataDir "browser-profile-view\DevToolsActivePort"
    $coursePort = $null
    for ($i = 0; $i -lt 40; $i++) {
        Start-Sleep -Milliseconds 500
        if (Test-Path -LiteralPath $coursePortFile) {
            $coursePort = ((Get-Content -LiteralPath $coursePortFile -TotalCount 1) -replace '\D', '')
            if ($coursePort) { break }
        }
    }
    $courseUrl = ""
    if ($coursePort) {
        for ($j = 0; $j -lt 20; $j++) {
            try {
                $pp = Invoke-RestMethod "http://127.0.0.1:$coursePort/json" -TimeoutSec 5
                $pg = $pp | Where-Object { $_.type -eq "page" -and $_.url -match "seu\.edu\.cn" } |
                    Select-Object -First 1
                if ($pg) { $courseUrl = $pg.url; break }
            } catch { }
            Start-Sleep -Milliseconds 500
        }
    }
    Check "left click opens the course page" ($courseUrl -match "seu\.edu\.cn")
}
Stop-Apps

if (-not $SkipBrowsers) {
    Write-Host "`n== webview2 engine ==" -ForegroundColor Cyan
    $env:SEU_WIZARD_ENGINE = "webview"
    $null = Start-Process $Exe -ArgumentList "-wizard" -PassThru
    Start-Sleep -Seconds 6
    Check "webview2 settings window opens" ((Find-Class "websurface_wv2") -ne [IntPtr]::Zero)
    Stop-Apps

    Write-Host "`n== borrowed engine ==" -ForegroundColor Cyan
    $env:SEU_WIZARD_ENGINE = "browser"
    $null = Start-Process $Exe -ArgumentList "-wizard" -PassThru
    $portFile = Join-Path $dataDir "wizard-webview\browser\DevToolsActivePort"
    $port = $null
    for ($i = 0; $i -lt 40; $i++) {
        Start-Sleep -Milliseconds 500
        if (Test-Path -LiteralPath $portFile) {
            $port = ((Get-Content -LiteralPath $portFile -TotalCount 1) -replace '\D', '')
            if ($port) { break }
        }
    }
    $title = ""
    if ($port) {
        # The port file appears before the page loads, so poll until it renders.
        for ($j = 0; $j -lt 30; $j++) {
            try {
                $pages = Invoke-RestMethod "http://127.0.0.1:$port/json" -TimeoutSec 5
                $page = $pages | Where-Object { $_.type -eq "page" -and $_.url -match "127\.0\.0\.1" } |
                    Select-Object -First 1
                if ($page -and $page.title) { $title = $page.title; break }
            } catch { }
            Start-Sleep -Milliseconds 500
        }
    }
    Check "borrowed engine serves the settings page" ($title -match "SEU")
    Stop-Apps
    Remove-Item Env:\SEU_WIZARD_ENGINE -ErrorAction SilentlyContinue

    $pushManifest = Join-Path $kitRoot "crates\websurface\Cargo.toml"
    if ((Get-Command cargo -ErrorAction SilentlyContinue) -and (Test-Path -LiteralPath $pushManifest)) {
        Write-Host "`n== host push ==" -ForegroundColor Cyan
        # Edge writes its own banner to stderr; don't let that abort the run.
        $prevEap = $ErrorActionPreference
        $ErrorActionPreference = "Continue"
        foreach ($engine in @("webview2", "borrowed")) {
            & cargo run -q --manifest-path $pushManifest --example push_selftest -- $engine 2>$null |
                ForEach-Object { $_.ToString() }
            Check "push reaches the page ($engine)" ($LASTEXITCODE -eq 0)
        }
        $ErrorActionPreference = $prevEap
    }
}

Write-Host ""
if ($fail -eq 0) {
    Write-Host "all smoke checks passed" -ForegroundColor Green
    exit 0
} else {
    Write-Host "$fail smoke check(s) failed" -ForegroundColor Red
    exit 1
}
