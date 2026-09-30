//! Manages the "run at login" entry in the current user's registry Run key.

use anyhow::Result;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
    RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE,
    REG_OPTION_NON_VOLATILE, REG_SZ, REG_VALUE_TYPE,
};

use crate::osutil::wide;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "SEULaborPusher";

pub fn is_enabled() -> bool {
    let subkey = wide(RUN_KEY);
    let mut hkey = HKEY::default();
    let r = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            None,
            KEY_QUERY_VALUE,
            &mut hkey,
        )
    };
    if r != ERROR_SUCCESS {
        return false;
    }
    let value = query_value(hkey);
    unsafe {
        let _ = RegCloseKey(hkey);
    }
    matches!(value, Some(v) if !v.is_empty())
}

pub fn enable() -> Result<()> {
    let exe = std::env::current_exe()?;
    let data = format!("\"{}\"", exe.display());
    let subkey = wide(RUN_KEY);
    let name = wide(VALUE_NAME);
    let mut hkey = HKEY::default();
    let r = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut hkey,
            None,
        )
    };
    if r != ERROR_SUCCESS {
        return Err(anyhow::anyhow!("无法写入注册表 Run 项(错误 {})", r.0));
    }
    let wide: Vec<u16> = data.encode_utf16().collect();
    let bytes = as_bytes(&wide);
    let r = unsafe { RegSetValueExW(hkey, PCWSTR(name.as_ptr()), None, REG_SZ, Some(bytes)) };
    unsafe {
        let _ = RegCloseKey(hkey);
    }
    if r != ERROR_SUCCESS {
        return Err(anyhow::anyhow!("无法写入注册表 Run 值(错误 {})", r.0));
    }
    Ok(())
}

pub fn disable() -> Result<()> {
    let subkey = wide(RUN_KEY);
    let name = wide(VALUE_NAME);
    let mut hkey = HKEY::default();
    let r = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            None,
            KEY_SET_VALUE,
            &mut hkey,
        )
    };
    if r != ERROR_SUCCESS {
        return Ok(());
    }
    let r = unsafe { RegDeleteValueW(hkey, PCWSTR(name.as_ptr())) };
    unsafe {
        let _ = RegCloseKey(hkey);
    }
    if r != ERROR_SUCCESS && r.0 != 2 {
        return Err(anyhow::anyhow!("无法删除注册表 Run 值(错误 {})", r.0));
    }
    Ok(())
}

/// Enables or disables autostart and returns the state actually in effect.
pub fn apply(enabled: bool) -> bool {
    let _ = if enabled { enable() } else { disable() };
    is_enabled()
}

fn as_bytes(wide: &[u16]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(wide.as_ptr() as *const u8, std::mem::size_of_val(wide)) }
}

fn query_value(hkey: HKEY) -> Option<String> {
    let name = wide(VALUE_NAME);
    let mut ty = REG_VALUE_TYPE::default();
    let mut size: u32 = 0;
    let r = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut ty),
            None,
            Some(&mut size),
        )
    };
    if r != ERROR_SUCCESS && r != ERROR_MORE_DATA {
        return None;
    }
    if size == 0 {
        return Some(String::new());
    }
    let mut buf = vec![0u8; size as usize];
    let r = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut ty),
            Some(buf.as_mut_ptr()),
            Some(&mut size),
        )
    };
    if r != ERROR_SUCCESS {
        return None;
    }
    let u16s: Vec<u16> = buf
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    Some(
        String::from_utf16_lossy(&u16s)
            .trim_end_matches('\0')
            .to_string(),
    )
}
