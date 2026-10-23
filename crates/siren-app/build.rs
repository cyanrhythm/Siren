//! 构建脚本：把图标与版本信息编进 Windows exe（其它平台什么都不做）。
//!
//! 交给 [`winresource`] 而不是自己调 `rc.exe`：
//! - 工具链覆盖 MSVC（`rc.exe`，自带 SDK 路径探测）、GNU（`windres`，含交叉前缀）、
//!   LLVM（`llvm-rc`），不必在本仓库里维护一套查找逻辑；
//! - `FileVersion` / `ProductVersion` 默认取 `CARGO_PKG_VERSION`，不会与
//!   workspace 版本脱节（自建版本曾长期停在 0.1.0 而 workspace 已是 0.2.0）；
//! - 图标文件变化会自动 `rerun-if-changed`。
//!
//! 找不到资源编译器时只警告、不失败：`cargo check --target x86_64-pc-windows-gnu`
//! 这类纯检查（不需要链接资源）在无 MinGW 的机器上也应当能过。

fn main() {
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
