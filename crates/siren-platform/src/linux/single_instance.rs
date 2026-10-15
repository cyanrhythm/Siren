//! Linux：单实例（flock）+ 已有实例唤起（D-Bus 激活）。
//!
//! ## 机制
//!
//! - **互斥**：`$XDG_RUNTIME_DIR/siren.lock` 上的 `flock(LOCK_EX | LOCK_NB)`。
//!   锁随文件描述符存亡：进程崩溃即自动释放，不存在陈旧锁；`$XDG_RUNTIME_DIR`
//!   缺失时回退系统临时目录（文件名带 uid，避免多用户互扰）。
//! - **唤起**：首实例在会话总线上以应用标识（如 `com.cyanrhythm.Siren`）注册
//!   `org.freedesktop.Application` 接口（XDG 桌面项规范的单实例激活通道，
//!   与 MPRIS / SNI 同走 D-Bus）；后续实例调用其 `Activate()` 方法后退出。
//! - **交接**：首实例把激活请求入队，由 UI 主循环经
//!   [`crate::poll_activation_events`] 取出后执行（唤起窗口需图形连接，
//!   Wayland 下必须由 GPUI 侧走 `xdg_activation_v1`——见
//!   `docs/02-平台能力规格.md` §4 与平台层 `window_system.rs` 的降级说明）。
//!
//! 无会话总线（罕见）时：互斥仍生效，仅无法唤起窗口，记日志不阻断。
//!
//! ## Wayland 下的已知限制
//!
//! xdg-shell 无「置前」协议：激活经 `xdg_activation_v1`，合成器可拒绝无有效
//! 用户输入的请求（KWin / Mutter 通常只高亮任务栏、不高亮并置前主窗口）。
//! 此时用户仍可从任务栏恢复窗口；X11（`_NET_ACTIVE_WINDOW`）无此限制。
//! 详见 `docs/06-路线图.md` 的降级约定。

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::ActivationEvent;

/// 应用在会话总线上的接口（XDG 桌面项规范）。
const ACTIVATION_INTERFACE: &str = "org.freedesktop.Application";
/// 锁文件名（置于 `$XDG_RUNTIME_DIR`）。
const LOCK_NAME: &str = "siren.lock";

/// 首实例的锁文件句柄（保活：进程存活期间一直持有 flock）。
static LOCK: OnceLock<File> = OnceLock::new();

/// 待消费的激活请求（去重，避免连点堆积）。
static PENDING: Mutex<Vec<ActivationEvent>> = Mutex::new(Vec::new());

/// 若为唯一实例返回 `true`；已有实例则请求其激活窗口并返回 `false`。
pub(crate) fn ensure_single_instance(app_id: &str) -> bool {
    match try_acquire() {
        Ok(true) => {
            spawn_activation_service(app_id);
            true
        }
        Ok(false) => {
            if let Err(e) = request_activation(app_id) {
                log::warn!("检测到已有实例，但唤起其窗口失败：{e}");
            }
            false
        }
        Err(e) => {
            // 锁不可用（权限/磁盘异常）时不阻断启动——与占位期「放行」约定一致。
            log::warn!("单实例锁不可用（{e}），按唯一实例继续");
            true
        }
    }
}

/// 取走待处理的激活请求（供主循环轮询）。
pub(crate) fn poll_activation_events(out: &mut Vec<ActivationEvent>) {
    match PENDING.lock() {
        Ok(mut pending) => out.append(&mut pending),
        Err(e) => log::warn!("激活请求队列已毒化，丢弃本轮事件：{e}"),
    }
}

// ── 互斥（flock）────────────────────────────────────────────────────

/// 尝试获取单实例锁；返回是否为本进程首次获取。
fn try_acquire() -> io::Result<bool> {
    if LOCK.get().is_some() {
        return Ok(true);
    }
    let (file, acquired) = lock_file(&lock_path())?;
    if acquired {
        // set 失败仅在并发重复调用时发生，属正常；锁仍被本次持有的 fd 保住。
        let _ = LOCK.set(file);
    }
    Ok(acquired)
}

/// 锁文件路径：`$XDG_RUNTIME_DIR/siren.lock`，缺失时回退系统临时目录。
fn lock_path() -> PathBuf {
    lock_path_in(std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).as_deref())
}

/// 可测核心：`None` / 空路径时回退临时目录（文件名带 uid）。
fn lock_path_in(runtime_dir: Option<&Path>) -> PathBuf {
    match runtime_dir.filter(|dir| !dir.as_os_str().is_empty()) {
        Some(dir) => dir.join(LOCK_NAME),
        None => std::env::temp_dir().join(format!("siren-{}.lock", unsafe { libc::getuid() })),
    }
}

/// 打开锁文件并尝试非阻塞排他锁；返回（文件句柄, 是否取得锁）。
///
/// flock 与 open 的文件描述符绑定：**取得锁后必须保活句柄**（见 [`LOCK`]），
/// drop 即释放。同一进程再次打开同一文件同样会被已持有的锁拒绝。
fn lock_file(path: &Path) -> io::Result<(File, bool)> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // 锁文件只作 flock 载体，内容不读写：显式不截断（truncate(false)）。
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    loop {
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc == 0 {
            return Ok((file, true));
        }
        let err = io::Error::last_os_error();
        match err.raw_os_error() {
            Some(libc::EWOULDBLOCK) => return Ok((file, false)),
            Some(libc::EINTR) => continue,
            _ => return Err(err),
        }
    }
}

// ── 唤起（D-Bus）───────────────────────────────────────────────────

/// 应用标识 → D-Bus 对象路径（`com.cyanrhythm.Siren` → `/com/cyanrhythm/Siren`）。
fn object_path(app_id: &str) -> String {
    format!("/{}", app_id.replace('.', "/"))
}

/// 首实例：后台线程注册激活接口，等待后续实例的 `Activate` 请求。
fn spawn_activation_service(app_id: &str) {
    let app_id = app_id.to_string();
    let spawned = std::thread::Builder::new()
        .name("siren-dbus-activation".into())
        .spawn(move || {
            if let Err(e) = serve_activation(&app_id) {
                log::warn!("单实例激活通道不可用（{e}）；再次启动应用将只退出、不唤起窗口");
            }
        });
    if let Err(e) = spawned {
        log::warn!("无法创建单实例激活线程：{e}");
    }
}

/// 服务端：先导出接口再抢总线名，等待后续实例的 `Activate` 请求。
///
/// 顺序对齐 `docs/02-平台能力规格.md` §3.4 的 MPRIS 教训：宿主（及后续实例）
/// 可能在名字出现的瞬间就发起调用，接口必须先于名字就绪。
fn serve_activation(app_id: &str) -> zbus::Result<()> {
    use zbus::fdo::RequestNameFlags;

    let connection = zbus::blocking::connection::Builder::session()?
        .serve_at(object_path(app_id).as_str(), ApplicationService)?
        .build()?;
    // DoNotQueue：名字被他人占用（异常场景）时立即失败，不挂起启动。
    connection.request_name_with_flags(app_id, RequestNameFlags::DoNotQueue.into())?;
    log::info!("单实例激活通道就绪（D-Bus {app_id}）");
    // 阻塞式 Connection 的执行器在后台线程运行；本线程守住连接所有权即可。
    loop {
        std::thread::park();
    }
}

/// 后续实例：请已有实例激活主窗口。
fn request_activation(app_id: &str) -> zbus::Result<()> {
    let connection = zbus::blocking::connection::Builder::session()?
        .method_timeout(std::time::Duration::from_secs(2))
        .build()?;
    let platform_data: std::collections::HashMap<String, zbus::zvariant::OwnedValue> = Default::default();
    connection.call_method(
        Some(app_id),
        object_path(app_id).as_str(),
        Some(ACTIVATION_INTERFACE),
        "Activate",
        &(platform_data,),
    )?;
    log::info!("已请求唤起床有实例的窗口");
    Ok(())
}

/// `org.freedesktop.Application` 服务实现（仅需 `Activate`）。
struct ApplicationService;

#[zbus::interface(name = "org.freedesktop.Application")]
impl ApplicationService {
    /// 桌面环境 / 后续实例请求把主窗口带到前台（单实例已由锁保证，直接入队）。
    fn activate(&self, _platform_data: std::collections::HashMap<String, zbus::zvariant::OwnedValue>) {
        enqueue_activation();
    }
}

/// 入队一条激活请求（去重：连点时不必堆多条）。
fn enqueue_activation() {
    match PENDING.lock() {
        Ok(mut pending) => {
            if !pending.contains(&ActivationEvent::ActivateMainWindow) {
                pending.push(ActivationEvent::ActivateMainWindow);
            }
        }
        Err(e) => log::warn!("激活请求队列已毒化，忽略该请求：{e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_path_follows_desktop_id() {
        assert_eq!(object_path("com.cyanrhythm.Siren"), "/com/cyanrhythm/Siren");
    }

    #[test]
    fn lock_path_prefers_runtime_dir_and_falls_back() {
        assert_eq!(
            lock_path_in(Some(Path::new("/run/user/1000"))),
            PathBuf::from("/run/user/1000/siren.lock")
        );
        let fallback = lock_path_in(None);
        assert!(fallback
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("siren-")));
        // 环境变量为空串时同样回退。
        assert_eq!(lock_path_in(Some(Path::new(""))), fallback);
    }

    #[test]
    fn flock_excludes_second_open_and_releases_on_drop() {
        let dir = std::env::temp_dir().join(format!("siren-lock-test-{}", std::process::id()));
        let path = dir.join("lock");
        let (first, acquired) = lock_file(&path).unwrap();
        assert!(acquired, "首次加锁应成功");
        let (_second, acquired) = lock_file(&path).unwrap();
        assert!(!acquired, "已被持有的锁应拒绝第二次加锁");
        drop(first);
        let (_third, acquired) = lock_file(&path).unwrap();
        assert!(acquired, "释放后应可再次加锁");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn activation_requests_drain_and_dedupe() {
        enqueue_activation();
        enqueue_activation();
        let mut out = Vec::new();
        poll_activation_events(&mut out);
        assert_eq!(out, vec![ActivationEvent::ActivateMainWindow], "重复请求应去重");
        let mut again = Vec::new();
        poll_activation_events(&mut again);
        assert!(again.is_empty(), "取走后队列应为空");
    }
}
