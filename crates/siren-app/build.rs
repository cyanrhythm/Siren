//! 构建脚本：
//! 1. 把当前提交号写进 `SIREN_GIT_HASH`（「关于」页展示）；
//! 2. Windows 下把图标与版本信息编进 exe（其它平台跳过）。
//!
//! 资源交给 [`winresource`] 而不是自己调 `rc.exe`：
//! - 工具链覆盖 MSVC（`rc.exe`，自带 SDK 路径探测）、GNU（`windres`，含交叉前缀）、
//!   LLVM（`llvm-rc`），不必在本仓库里维护一套查找逻辑；
//! - `FileVersion` / `ProductVersion` 默认取 `CARGO_PKG_VERSION`，不会与
//!   workspace 版本脱节（自建版本曾长期停在 0.1.0 而 workspace 已是 0.2.0）；
//! - 图标文件变化会自动 `rerun-if-changed`。
//!
//! 找不到资源编译器时只警告、不失败：`cargo check --target x86_64-pc-windows-gnu`
//! 这类纯检查（不需要链接资源）在无 MinGW 的机器上也应当能过。

use std::process::Command;

fn main() {
    emit_git_hash();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon("../../assets/icons/siren.ico")
        .set("CompanyName", "cyanrhythm")
        .set("FileDescription", "Siren")
        .set("InternalName", "siren")
        .set("OriginalFilename", "siren.exe")
        .set("ProductName", "Siren")
        // 简体中文（0x0804）优先，探测不到就沿用默认语言。
        .set_language(0x0804);
    if let Err(err) = resource.compile() {
        println!("cargo:warning=Windows 资源未编译（缺少 rc.exe / windres）：{err}");
    }
}

/// 当前提交号 → `SIREN_GIT_HASH`（源目录不是 git 仓库时为 `unknown`）。
///
/// 只在 HEAD 变化时重跑：`HEAD` 指向分支时内容不变，故把分支 ref 与 reflog 也列入
/// 监听（存在才算），这样切分支/提交都会重新取号。
fn emit_git_hash() {
    let hash = git(&["rev-parse", "--short=8", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=SIREN_GIT_HASH={hash}");
    for changed in ["../../.git/HEAD", "../../.git/logs/HEAD"] {
        if std::path::Path::new(changed).exists() {
            println!("cargo:rerun-if-changed={changed}");
        }
    }
    // HEAD 指向的分支 ref 才是提交真正写入的位置。
    if let Some(ref_name) = git(&["symbolic-ref", "--quiet", "--short", "HEAD"]) {
        let path = format!("../../.git/{ref_name}");
        if std::path::Path::new(&path).exists() {
            println!("cargo:rerun-if-changed={path}");
        }
    }
}

/// 执行 `git <args>` 并返回去空白后的输出；失败返回 `None`。
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!text.is_empty()).then_some(text)
}
