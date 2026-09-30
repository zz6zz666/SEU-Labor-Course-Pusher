//! Locates a Chromium-family browser to drive over CDP.
//!
//! Edge ships with Windows, but the user may prefer Chrome or a third-party
//! Chromium build, or may have no Chromium browser at all. We gather candidates
//! from several sources (explicit override, the default-browser association,
//! well-known install locations and the registry's App Paths) and let the
//! launcher try them in order.

use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::anyhow;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
    KEY_READ, REG_EXPAND_SZ, REG_SZ, REG_VALUE_TYPE,
};
use windows::Win32::UI::Shell::{AssocQueryStringW, ASSOCF_NONE, ASSOCSTR_EXECUTABLE};

/// Executable names accepted as CDP-capable Chromium browsers. Electron shells
/// (e.g. Tabbit, ZERO) are deliberately excluded: they ignore `--app` and
/// `--remote-debugging-port`.
const CHROMIUM_EXES: &[&str] = &[
    "msedge.exe",
    "chrome.exe",
    "brave.exe",
    "brave-browser.exe",
    "vivaldi.exe",
    "opera.exe",
    "chromium.exe",
    "360chrome.exe",
    "360chromex.exe",
    "360se.exe",
    "sogouexplorer.exe",
    "qqbrowser.exe",
];

/// The error shown when nothing usable was found.
pub fn no_browser_error() -> anyhow::Error {
    anyhow!(
        "未找到可用的 Chromium 内核浏览器(Edge/Chrome 等)。\
         请在 config.json 设置 browser.path,或安装 Edge/Chrome 后重试"
    )
}

/// Whether an executable name is a Chromium browser we can drive over CDP.
pub fn is_browser_exe(name: &str) -> bool {
    CHROMIUM_EXES.contains(&name.to_lowercase().as_str())
}

/// Returns usable browser executables in preference order: explicit override,
/// the default browser, well-known install locations, then registry App Paths.
pub fn candidates(override_path: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut add = |p: PathBuf| {
        if !p.is_file() {
            return;
        }
        let key = p.to_string_lossy().to_lowercase();
        if seen.insert(key) {
            out.push(p.to_string_lossy().into_owned());
        }
    };

    if let Some(p) = override_path {
        let p = p.trim().trim_matches('"');
        if !p.is_empty() {
            add(PathBuf::from(p));
        }
    }
    if let Some(p) = default_browser() {
        add(PathBuf::from(p));
    }
    for p in known_paths() {
        add(p);
    }
    for p in app_paths() {
        add(PathBuf::from(p));
    }
    out
}

/// The executable of the current default browser, but only when it is a known
/// Chromium build (so we never hand `--app` to Firefox or an Electron shell).
fn default_browser() -> Option<String> {
    let path = assoc_executable(".html")?;
    let name = PathBuf::from(&path)
        .file_name()
        .map(|s| s.to_string_lossy().to_lowercase())?;
    CHROMIUM_EXES.contains(&name.as_str()).then_some(path)
}

fn known_paths() -> Vec<PathBuf> {
    let bases: Vec<String> = [
        std::env::var("LocalAppData").ok(),
        std::env::var("ProgramFiles").ok(),
        std::env::var("ProgramFiles(x86)").ok(),
    ]
    .into_iter()
    .flatten()
    .filter(|s| !s.is_empty())
    .collect();

    // Vendor order: Edge first (present on every supported Windows), then
    // Chrome and the other Chromium builds.
    let rels = [
        r"Microsoft\Edge\Application\msedge.exe",
        r"Google\Chrome\Application\chrome.exe",
        r"BraveSoftware\Brave-Browser\Application\brave.exe",
        r"Vivaldi\Application\vivaldi.exe",
        r"Chromium\Application\chrome.exe",
        r"Programs\Opera\opera.exe",
        r"Opera\opera.exe",
        r"360Chrome\Chrome\Application\360chrome.exe",
        r"360ChromeX\Chrome\Application\360ChromeX.exe",
        r"360se6\Application\360se.exe",
        r"SogouExplorer\SogouExplorer.exe",
        r"Tencent\QQBrowser\QQBrowser.exe",
    ];
    let mut out = Vec::new();
    for rel in rels {
        for base in &bases {
            out.push(PathBuf::from(base).join(rel));
        }
    }
    out
}

fn app_paths() -> Vec<String> {
    let roots = [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE];
    let subs = [
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths",
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\App Paths",
    ];
    let mut out = Vec::new();
    for exe in CHROMIUM_EXES {
        for root in roots {
            for sub in subs {
                let key = format!(r"{}\{}", sub, exe);
                if let Some(v) = read_reg_string(root, &key) {
                    out.push(v);
                }
            }
        }
    }
    out
}

/// Reads the default (unnamed) value of a registry key as a string.
fn read_reg_string(root: HKEY, subkey: &str) -> Option<String> {
    let sub = to_wide(subkey);
    let name = to_wide("");
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(root, PCWSTR(sub.as_ptr()), None, KEY_READ, &mut hkey) != ERROR_SUCCESS {
            return None;
        }
        let mut kind = REG_VALUE_TYPE::default();
        let mut size: u32 = 0;
        let probe = RegQueryValueExW(
            hkey,
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut kind),
            None,
            Some(&mut size),
        );
        if probe != ERROR_SUCCESS || size == 0 {
            let _ = RegCloseKey(hkey);
            return None;
        }
        let mut buf = vec![0u8; size as usize + 2];
        let got = RegQueryValueExW(
            hkey,
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut kind),
            Some(buf.as_mut_ptr()),
            Some(&mut size),
        );
        let _ = RegCloseKey(hkey);
        if got != ERROR_SUCCESS {
            return None;
        }
        if kind != REG_SZ && kind != REG_EXPAND_SZ {
            return None;
        }
        let usable = (size as usize).min(buf.len());
        let wide: Vec<u16> = buf[..usable]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let s = String::from_utf16_lossy(&wide);
        let s = s.trim_end_matches('\u{0}').trim().to_string();
        (!s.is_empty()).then_some(s)
    }
}

fn assoc_executable(ext: &str) -> Option<String> {
    let ext_w = to_wide(ext);
    let verb_w = to_wide("open");
    let hook = |out: Option<PWSTR>, len: *mut u32| unsafe {
        AssocQueryStringW(
            ASSOCF_NONE,
            ASSOCSTR_EXECUTABLE,
            PCWSTR(ext_w.as_ptr()),
            PCWSTR(verb_w.as_ptr()),
            out,
            len,
        )
    };

    let mut len: u32 = 0;
    let _ = hook(None, &mut len);
    if len == 0 {
        return None;
    }
    let mut buf = vec![0u16; len as usize + 1];
    if hook(Some(PWSTR(buf.as_mut_ptr())), &mut len).is_err() {
        return None;
    }
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let s = String::from_utf16_lossy(&buf[..end]);
    let s = s.trim().to_string();
    (!s.is_empty()).then_some(s)
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
