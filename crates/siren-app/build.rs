//! 构建脚本：Windows 下用 Windows SDK 的 `rc.exe` 编译资源（图标 + 版本信息），
//! 并把生成的 `.res` 交给链接器（MSVC `link.exe` 可直接链接 `.res`）。

use std::path::PathBuf;
use std::process::Command;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=../../assets/icons/siren.ico");

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR 未设置");
    let res_path = format!("{out_dir}/siren.res");
    let rc = find_rc().expect("未找到 rc.exe（Windows SDK 资源编译器）");

    let status = Command::new(&rc)
        .arg("/nologo")
        .arg("/fo")
        .arg(&res_path)
        .arg("app.rc")
        .status()
        .unwrap_or_else(|e| panic!("执行 {} 失败: {e}", rc.display()));
    assert!(status.success(), "rc.exe 编译资源失败");

    println!("cargo:rustc-link-arg={res_path}");
}

/// 定位 `rc.exe`：优先环境变量，其次 Windows Kits 目录，再次 PATH。
fn find_rc() -> Option<PathBuf> {
    for key in ["RC_x86_64_pc_windows_msvc", "RC"] {
        if let Ok(value) = std::env::var(key) {
            if !value.trim().is_empty() {
                return Some(PathBuf::from(value));
            }
        }
    }
    let kits = std::env::var("ProgramFiles(x86)")
        .or_else(|_| std::env::var("ProgramFiles"))
        .map(|pf| PathBuf::from(pf).join("Windows Kits").join("10").join("bin"));
    if let Ok(kits) = kits {
        let mut candidates: Vec<PathBuf> = std::fs::read_dir(&kits)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path().join("x64").join("rc.exe"))
            .filter(|path| path.exists())
            .collect();
        // 目录名形如 10.0.22621.0，取版本最大的
        candidates.sort();
        if let Some(path) = candidates.pop() {
            return Some(path);
        }
    }
    Some(PathBuf::from("rc.exe"))
}
