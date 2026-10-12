//! macOS：libmpv 定位。

use crate::native_library::NativeLibraryLocator;

/// macOS 实现：优先随包 `.dylib`，再回退 Homebrew 常见路径。
#[derive(Default)]
pub struct MacosNativeLibraryLocator;

impl NativeLibraryLocator for MacosNativeLibraryLocator {
    fn mpv_candidates(&self) -> Vec<String> {
        vec![
            "libmpv.2.dylib".to_string(),
            "libmpv.dylib".to_string(),
            "/opt/homebrew/lib/libmpv.2.dylib".to_string(),
            "/usr/local/lib/libmpv.2.dylib".to_string(),
        ]
    }

    fn prepare(&self) {
        unsafe {
            libc::setlocale(libc::LC_NUMERIC, c"C".as_ptr());
        }
    }
}
