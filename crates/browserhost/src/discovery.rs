//! Locates a Chromium-family browser to drive over CDP.
//!
//! Edge ships with Windows, but the user may prefer Chrome or a third-party
//! Chromium build, or may have no Chromium browser at all. We gather candidates
//! from several sources (explicit override, the default-browser association,
//! well-known install locations and the registry's App Paths) and let the
//! launcher try them in order.

use std::collections::HashSet;
use std::error::Error as StdError;
use std::fmt;
use std::path::PathBuf;

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
    KEY_READ, REG_EXPAND_SZ, REG_SZ, REG_VALUE_TYPE,
};
use windows::Win32::UI::Shell::{AssocQueryStringW, ASSOCF_NONE, ASSOCSTR_EXECUTABLE};

/// Executable names of well-known Chromium browsers, used to find their usual
/// install locations and App Paths. This is a heuristic for *locating* browsers,
/// not a gate: a fork the user made their default (e.g. Tabbit, ZERO) is
/// accepted, and whether a candidate really speaks CDP is settled at launch
/// time by whether it opens a debug port.
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

/// Browsers that do not speak CDP at all, so we skip them without a doomed
/// launch. Everything else is tried and rejected only if it opens no debug port.
const NON_CDP_EXES: &[&str] = &["firefox.exe", "iexplore.exe", "safari.exe"];

/// Returned by [`no_browser_error`] when no Chromium-family browser is present.
#[derive(Debug)]
pub struct NoBrowser;

impl fmt::Display for NoBrowser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("no Chromium-based browser (Edge, Chrome, ...) was found")
    }
}

impl StdError for NoBrowser {}

/// The error returned when nothing usable was found. The host is expected to
/// map this to its own, actionable guidance.
pub fn no_browser_error() -> anyhow::Error {
    anyhow::Error::new(NoBrowser)
}

/// Whether `err` is the [`NoBrowser`] error, so a host can offer its own
/// guidance instead of this crate's generic message.
pub fn is_no_browser(err: &anyhow::Error) -> bool {
    err.downcast_ref::<NoBrowser>().is_some()
}

/// Returns usable browser executables in preference order: explicit override,
/// Edge, Chrome, the user's default browser, other well-known install locations,
/// then registry App Paths.
pub fn candidates(override_path: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    if let Some(p) = override_path {
        let p = p.trim().trim_matches('"');
        if !p.is_empty() {
            push_existing(&mut out, &mut seen, PathBuf::from(p));
        }
    }
    // Edge/Chrome ahead of the user's default, so a managed image keeps its
    // known-good browser even when a third-party app grabbed the default.
    for p in preferred_paths() {
        push_existing(&mut out, &mut seen, p);
    }
    if let Some(p) = default_browser() {
        push_existing(&mut out, &mut seen, PathBuf::from(p));
    }
    for p in known_paths() {
        push_existing(&mut out, &mut seen, p);
    }
    for p in app_paths() {
        push_existing(&mut out, &mut seen, PathBuf::from(p));
    }
    out
}

/// Appends `p` when it is an existing file not already present (case-insensitive
/// on the path, matching Windows' case-insensitive filesystem).
fn push_existing(out: &mut Vec<String>, seen: &mut HashSet<String>, p: PathBuf) {
    if !p.is_file() {
        return;
    }
    let key = p.to_string_lossy().to_lowercase();
    if seen.insert(key) {
        out.push(p.to_string_lossy().into_owned());
    }
}

/// The executable of the current default browser. Returned regardless of its
/// vendor name so the user's own choice (e.g. a Chromium fork like Tabbit or
/// ZERO) is tried first; only known non-CDP browsers are skipped outright, and
/// anything else that cannot actually be driven is dropped by the launch probe.
fn default_browser() -> Option<String> {
    let path = assoc_executable(".html")?;
    let name = PathBuf::from(&path)
        .file_name()
        .map(|s| s.to_string_lossy().to_lowercase())?;
    (!NON_CDP_EXES.contains(&name.as_str())).then_some(path)
}

/// Install-root prefixes we probe for well-known browsers.
fn base_dirs() -> Vec<String> {
    [
        std::env::var("LocalAppData").ok(),
        std::env::var("ProgramFiles").ok(),
        std::env::var("ProgramFiles(x86)").ok(),
    ]
    .into_iter()
    .flatten()
    .filter(|s| !s.is_empty())
    .collect()
}

fn paths_under(rels: &[&str]) -> Vec<PathBuf> {
    let bases = base_dirs();
    let mut out = Vec::new();
    for rel in rels {
        for base in &bases {
            out.push(PathBuf::from(base).join(rel));
        }
    }
    out
}

/// Edge and Chrome, always tried before a third-party default browser.
fn preferred_paths() -> Vec<PathBuf> {
    paths_under(&[
        r"Microsoft\Edge\Application\msedge.exe",
        r"Google\Chrome\Application\chrome.exe",
    ])
}

fn known_paths() -> Vec<PathBuf> {
    // Remaining Chromium builds, after Edge/Chrome above.
    paths_under(&[
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
    ])
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates a real file so `push_existing`'s `is_file` check passes.
    fn touch(dir: &std::path::Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, b"x").unwrap();
        p
    }

    #[test]
    fn push_existing_keeps_only_files_and_dedups_case_insensitively() {
        let dir = std::env::temp_dir().join("browserhost-discovery-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let a = touch(&dir, "Edge.exe");
        let missing = dir.join("Nope.exe");

        let mut out = Vec::new();
        let mut seen = HashSet::new();
        push_existing(&mut out, &mut seen, a.clone());
        push_existing(&mut out, &mut seen, a.clone()); // duplicate path
        push_existing(&mut out, &mut seen, a.with_file_name("edge.EXE")); // same file, different case
        push_existing(&mut out, &mut seen, missing); // does not exist

        assert_eq!(out.len(), 1, "duplicates and missing files are dropped");
        assert!(out[0].to_lowercase().ends_with("edge.exe"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_browser_error_is_detectable_by_the_host() {
        let e = no_browser_error();
        assert!(is_no_browser(&e));
        assert!(!is_no_browser(&anyhow::anyhow!("something else")));
    }
}
