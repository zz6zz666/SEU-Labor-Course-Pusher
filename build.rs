//! Compiles a Windows resource (icon + version info) into the executable so the
//! exe looks identical to the previous build in Explorer and the file-properties
//! dialog. Uses the mingw `windres` shipped with the GNU toolchain.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=SEU_LABOR_VERSION");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let version =
        std::env::var("SEU_LABOR_VERSION").unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string());
    let parts: Vec<u32> = version.split('.').map(|x| x.parse().unwrap_or(0)).collect();
    let get = |i: usize| parts.get(i).copied().unwrap_or(0);
    let file_ver = format!("{},{},{},{}", get(0), get(1), get(2), get(3));
    let str_ver = format!("{}.{}.{}.{}", get(0), get(1), get(2), get(3));

    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let icon = root.join("assets").join("icon.ico");
    let icon_path = icon.display().to_string().replace('\\', "\\\\");

    let rc = format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {fv}
PRODUCTVERSION {fv}
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "080404b0"
    BEGIN
      VALUE "CompanyName", "SEU"
      VALUE "FileDescription", "SEU 劳动教育课程推送助手"
      VALUE "FileVersion", "{sv}"
      VALUE "InternalName", "seu-labor"
      VALUE "LegalCopyright", "MIT License"
      VALUE "OriginalFilename", "seu-labor.exe"
      VALUE "ProductName", "SEU 劳动教育课程推送助手"
      VALUE "ProductVersion", "{sv}"
      VALUE "Comments", "https://github.com/zz6zz666/SEU-Labor-Course-Pusher"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x804, 1200
  END
END
"#,
        icon = icon_path,
        fv = file_ver,
        sv = str_ver,
    );

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let rc_path = out.join("app.rc");
    if std::fs::write(&rc_path, rc).is_err() {
        return;
    }

    let Some(windres) = find_windres() else {
        println!("cargo:warning=未找到 windres,已跳过图标/版本信息");
        return;
    };
    // windres shells out to gcc for preprocessing; make sure its sibling gcc is
    // reachable by prepending the windres directory to the child's PATH.
    let path = match windres.parent() {
        Some(dir) => {
            let old = std::env::var("PATH").unwrap_or_default();
            format!("{};{}", dir.display(), old)
        }
        None => std::env::var("PATH").unwrap_or_default(),
    };
    let obj = out.join("app.res.o");
    let status = Command::new(&windres)
        .env("PATH", path)
        .args(["--codepage=65001", "-O", "coff", "-i"])
        .arg(&rc_path)
        .arg("-o")
        .arg(&obj)
        .status();
    match status {
        Ok(s) if s.success() => {
            println!("cargo:rustc-link-arg={}", obj.display());
        }
        _ => println!("cargo:warning=windres 资源编译失败,已跳过图标/版本信息"),
    }
}

fn find_windres() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("WINDRES") {
        let p = PathBuf::from(p);
        if p.exists() {
            return Some(p);
        }
    }
    for cand in [
        r"C:\msys64\ucrt64\bin\windres.exe",
        r"C:\msys64\clang64\bin\windres.exe",
        r"C:\msys64\mingw64\bin\windres.exe",
    ] {
        let p = PathBuf::from(cand);
        if p.exists() {
            return Some(p);
        }
    }
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(';') {
            let p = PathBuf::from(dir).join("windres.exe");
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}
