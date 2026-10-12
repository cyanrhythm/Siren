//! Windows：libmpv 定位。

use crate::native_library::NativeLibraryLocator;

/// Windows 实现：随包分发 `libmpv-2.dll`（可执行文件同目录）。
#[derive(Default)]
pub struct WindowsNativeLibraryLocator;

impl NativeLibraryLocator for WindowsNativeLibraryLocator {
    fn mpv_candidates(&self) -> Vec<String> {
        vec!["libmpv-2.dll".to_string()]
    }

    // MSVC CRT 默认 `LC_NUMERIC` 即为 "C"，无需处理。
}
