//! Windows：`GetProcessMemoryInfo` 取 Working Set。
//!
//! 注意：本文件只在 Windows 目标下编译；本仓库的 Linux CI 覆盖不到它，
//! 改动后请在本机跑一次 `cargo check`（或依赖 windows-latest 的 CI 矩阵）。

use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::Threading::GetCurrentProcess;

/// 当前进程工作集（字节）。
pub(crate) fn rss_bytes() -> Option<u64> {
    unsafe {
        let mut counters = PROCESS_MEMORY_COUNTERS::default();
        counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb)
            .ok()
            .map(|_| counters.WorkingSetSize as u64)
    }
}
