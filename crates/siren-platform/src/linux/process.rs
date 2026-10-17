//! Linux：从 `/proc/self/status` 读 `VmRSS`。

/// 当前进程常驻内存（字节）。
pub(crate) fn rss_bytes() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    // 形如 `VmRSS:\t   123456 kB`
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_a_plausible_rss() {
        let rss = super::rss_bytes().expect("Linux 上应能读到 /proc/self/status");
        // 任何活着的测试进程都不止 1 MB，也不会超过 1 TB（防止单位解析错）。
        assert!(rss > 1024 * 1024, "RSS 过小，单位可能算错: {rss}");
        assert!(rss < 1024u64.pow(4), "RSS 过大，单位可能算错: {rss}");
    }
}
