//! 原生动态库定位：**唯一涉及平台库名/搜索路径的能力**。
//!
//! `siren-core` 的 libmpv 引擎只接收一个候选列表，不感知平台；
//! 各平台实现负责给出正确的库名与路径，以及在加载前做必要的一次性准备。
//!
//! ## 平台差异
//!
//! | 平台 | 库名 | 备注 |
//! |---|---|---|
//! | Windows | `libmpv-2.dll` | 随包分发，放可执行文件同目录 |
//! | Linux | `libmpv.so.2` → `libmpv.so` → `mpv` | 优先随包，回退系统库 |
//! | macOS | `libmpv.2.dylib` | 常见于 Homebrew 路径 |
//!
//! 另：libmpv 要求进程 `LC_NUMERIC` 为 `"C"`，否则 `mpv_create` 会失败——
//! 该修正属平台差异，由实现方在 [`NativeLibraryLocator::prepare`] 中处理。

/// 原生库定位契约。
pub trait NativeLibraryLocator: Send + Sync {
    /// 该平台可尝试的 libmpv 库名/路径候选（按优先级排列）。
    fn mpv_candidates(&self) -> Vec<String>;

    /// 加载 libmpv 前的一次性准备（如纠正数值区域）。默认无操作。
    fn prepare(&self) {}
}

/// 当前平台的原生库定位实现。
pub fn native_library() -> &'static dyn NativeLibraryLocator {
    crate::imp::native_library()
}

/// 当前平台的 libmpv 库名/路径候选（便捷入口）。
pub fn mpv_library_candidates() -> Vec<String> {
    native_library().mpv_candidates()
}

/// 加载 libmpv 前的一次性准备（如纠正数值区域）。
pub fn prepare_native_environment() {
    native_library().prepare();
}
