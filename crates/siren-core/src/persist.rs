//! 原子文件写入：先写同目录临时文件再 `rename` 替换。
//!
//! 直接 `fs::write` 在崩溃/断电瞬间可能留下被截断的 JSON，导致整个配置文件静默丢失。
//! 临时文件与目标同目录（同卷），`rename` 为原子替换。

use std::path::Path;

/// 原子写入 `contents` 到 `path`（必要时创建父目录）。
///
/// 失败静默忽略（与既有持久化策略一致，不阻断业务流程）。
pub fn write_atomic(path: &Path, contents: &str) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, contents).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return;
    }
    if std::fs::rename(&tmp, path).is_err() {
        // 少数平台/情形下 rename 不覆盖已存在目标：删除后再试一次。
        let _ = std::fs::remove_file(path);
        if std::fs::rename(&tmp, path).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_replaces_existing() {
        let dir = std::env::temp_dir().join(format!("siren-persist-test-{}", std::process::id()));
        let path = dir.join("x.json");
        write_atomic(&path, "{\"a\":1}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"a\":1}");
        // 覆盖写入，且不残留临时文件。
        write_atomic(&path, "{\"a\":2}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"a\":2}");
        assert!(!path.with_extension("tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
