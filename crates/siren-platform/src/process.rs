//! 进程级度量：内存占用。
//!
//! 供设置页「图片缓存」与周期日志展示真实占用——只看图片缓存自己的计数会漏掉
//! GPUI 资产表里的解码位图，必须配合进程 RSS 才能判断优化是否生效。
//!
//! 各平台实现经 `crate::imp`（见 `lib.rs`「三」）选择；做不到的平台返回 `None`，
//! 上层展示为「—」，绝不 panic。

/// 当前进程常驻内存（RSS / Working Set），单位字节。
pub fn rss_bytes() -> Option<u64> {
    crate::imp::rss_bytes()
}
