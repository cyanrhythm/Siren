//! Linux：libmpv 定位。

use crate::native_library::NativeLibraryLocator;

/// Linux 实现：优先随包 `.so`，再回退系统库。
#[derive(Default)]
pub struct LinuxNativeLibraryLocator;

impl NativeLibraryLocator for LinuxNativeLibraryLocator {
    fn mpv_candidates(&self) -> Vec<String> {
        vec!["libmpv.so.2".to_string(), "libmpv.so".to_string(), "mpv".to_string()]
    }

    fn prepare(&self) {
        // glibc / musl：LC_NUMERIC = 1
        unsafe {
            libc::setlocale(libc::LC_NUMERIC, c"C".as_ptr());
        }
    }
}
