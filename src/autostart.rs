//! Manages the "run at login" entry in the current user's registry Run key.

use std::ffi::c_void;

use anyhow::Result;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "SEULaborPusher";

const HKEY_CURRENT_USER: isize = 0x8000_0001u32 as i32 as isize;
const KEY_QUERY_VALUE: u32 = 0x0001;
const KEY_SET_VALUE: u32 = 0x0002;
const REG_OPTION_NON_VOLATILE: u32 = 0;
const REG_SZ: u32 = 1;
const ERROR_SUCCESS: i32 = 0;
const ERROR_MORE_DATA: i32 = 234;

#[link(name = "advapi32")]
extern "system" {
    fn RegCreateKeyExW(
        hkey: isize,
        subkey: *const u16,
        reserved: u32,
        class: *const u16,
        options: u32,
        sam: u32,
        sa: *mut c_void,
        result: *mut isize,
        disposition: *mut u32,
    ) -> i32;
    fn RegOpenKeyExW(
        hkey: isize,
        subkey: *const u16,
        options: u32,
        sam: u32,
        result: *mut isize,
    ) -> i32;
    fn RegSetValueExW(
        hkey: isize,
        name: *const u16,
        reserved: u32,
        ty: u32,
        data: *const u8,
        cb: u32,
    ) -> i32;
    fn RegQueryValueExW(
        hkey: isize,
        name: *const u16,
        reserved: *mut u32,
        ty: *mut u32,
        data: *mut u8,
        cb: *mut u32,
    ) -> i32;
    fn RegDeleteValueW(hkey: isize, name: *const u16) -> i32;
    fn RegCloseKey(hkey: isize) -> i32;
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn is_enabled() -> bool {
    let subkey = to_wide(RUN_KEY);
    let mut hkey: isize = 0;
    let r = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            KEY_QUERY_VALUE,
            &mut hkey,
        )
    };
    if r != ERROR_SUCCESS {
        return false;
    }
    let value = query_value(hkey);
    unsafe { RegCloseKey(hkey) };
    matches!(value, Some(v) if !v.is_empty())
}

pub fn enable() -> Result<()> {
    let exe = std::env::current_exe()?;
    let data = format!("\"{}\"", exe.display());
    let subkey = to_wide(RUN_KEY);
    let name = to_wide(VALUE_NAME);
    let mut hkey: isize = 0;
    let r = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            std::ptr::null_mut(),
            &mut hkey,
            std::ptr::null_mut(),
        )
    };
    if r != ERROR_SUCCESS {
        return Err(anyhow::anyhow!("无法写入注册表 Run 项(错误 {})", r));
    }
    let wide: Vec<u16> = data.encode_utf16().collect();
    let bytes: &[u8] =
        unsafe { std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2) };
    let r = unsafe {
        RegSetValueExW(
            hkey,
            name.as_ptr(),
            0,
            REG_SZ,
            bytes.as_ptr(),
            bytes.len() as u32,
        )
    };
    unsafe { RegCloseKey(hkey) };
    if r != ERROR_SUCCESS {
        return Err(anyhow::anyhow!("无法写入注册表 Run 值(错误 {})", r));
    }
    Ok(())
}

pub fn disable() -> Result<()> {
    let subkey = to_wide(RUN_KEY);
    let name = to_wide(VALUE_NAME);
    let mut hkey: isize = 0;
    let r = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            KEY_SET_VALUE,
            &mut hkey,
        )
    };
    if r != ERROR_SUCCESS {
        return Ok(());
    }
    let r = unsafe { RegDeleteValueW(hkey, name.as_ptr()) };
    unsafe { RegCloseKey(hkey) };
    if r != ERROR_SUCCESS && r != 2 {
        return Err(anyhow::anyhow!("无法删除注册表 Run 值(错误 {})", r));
    }
    Ok(())
}

/// Enables or disables autostart and returns the state actually in effect.
pub fn apply(enabled: bool) -> bool {
    let _ = if enabled { enable() } else { disable() };
    is_enabled()
}

fn query_value(hkey: isize) -> Option<String> {
    let name = to_wide(VALUE_NAME);
    let mut ty: u32 = 0;
    let mut size: u32 = 0;
    let r = unsafe {
        RegQueryValueExW(
            hkey,
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut ty,
            std::ptr::null_mut(),
            &mut size,
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
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut ty,
            buf.as_mut_ptr(),
            &mut size,
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
