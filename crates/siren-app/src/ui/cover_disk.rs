//! 封面磁盘缓存的**命名与容量**：通用 LRU 在 [`crate::disk_lru`]。
//!
//! 键是档位化 URL（含 `?param=WyH`，见 `covers::sized_url`），文件名取其
//! **FNV-1a 128 位哈希**——缓存目录里看不到原始 URL，只留下不可逆的哈希。
//! 因此这一层只缓存「公开封面/头像」这类可公开的内容，不含任何用户数据。

/// 磁盘上限（对齐 Melodia `DISK_CACHE_MAX_BYTES`）。
pub(crate) const COVER_DISK_LIMIT: u64 = 500 * 1024 * 1024;
/// 淘汰水位：一次清到该值以下，避免刚过线就反复删。
pub(crate) const COVER_DISK_LOW_WATER: u64 = 450 * 1024 * 1024;

/// 水位必须低于上限，否则一次 trim 之后仍会反复触发。
const _: () = assert!(COVER_DISK_LOW_WATER < COVER_DISK_LIMIT);

/// 键 → 条目文件名（128 位哈希 + `.img`）。
pub(crate) fn cover_file_name(key: &str) -> String {
    format!("{:016x}{:016x}.img", hash64(key, 1), hash64(key, 2))
}

/// 稳定哈希：同一把 salt 下同一字符串总是同一个值（跨进程一致，`DefaultHasher` 不保证，
/// 故这里用 FNV-1a 手写，避免换 Rust 版本后整盘缓存失效）。
fn hash64(key: &str, salt: u8) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325 ^ u64::from(salt);
    for byte in key.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_key_same_name_and_distinct_keys_differ() {
        let a = cover_file_name("https://p1.music.126.net/x.jpg?param=64y64");
        let b = cover_file_name("https://p1.music.126.net/x.jpg?param=64y64");
        let c = cover_file_name("https://p1.music.126.net/x.jpg?param=128y128");
        assert_eq!(a, b, "同一键必须落到同一文件");
        assert_ne!(a, c, "不同档位必须是不同条目");
        assert!(a.ends_with(".img"), "后缀便于人工识别与清理：{a}");
        assert_eq!(a.len(), 32 + 4, "128 位哈希 + 后缀：{a}");
        assert!(!a.contains("music.126.net"), "文件名不得泄露原始 URL");
    }

    #[test]
    fn hash_is_stable_across_calls_and_salts_are_independent() {
        // 换 Rust 版本不能让整盘缓存失效，故断言 FNV-1a 的具体值。
        assert_eq!(hash64("a", 0), 0xaf63_dc4c_8601_ec8c);
        assert_ne!(hash64("abc", 1), hash64("abc", 2));
        assert_ne!(hash64("abc", 1), hash64("abd", 1));
    }
}
