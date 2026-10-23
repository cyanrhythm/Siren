# Windows 原生库

打包 Windows 安装包前，把 **`libmpv-2.dll`** 放到本目录下的 `bin/`：

```
assets/native/windows/bin/libmpv-2.dll
```

（`bin/` 不存在时自行创建。`cargo packager` 会把 `bin/*.dll` 复制到安装目录，与
`siren.exe` 同目录；运行时由 `siren-platform` 的 `NativeLibraryLocator` 按
`libmpv-2.dll` 加载。）

## 为什么不在仓库里

- 体积：单个 `libmpv-2.dll` 约 30–40 MB；
- 许可：libmpv 为 LGPL/GPL 双许可，分发条款与 MIT 的仓库主体不同；
- 平台：只有 Windows 需要预置（Linux 走 `libmpv.so.2` / 系统 `mpv`，macOS 走 Homebrew 路径）。

## 缺失时会怎样

**打包不会失败**（`resources` 用 `bin/*.dll` 通配），但产物里没有音频引擎：

- 启动时记一条 `warn`：`未找到 libmpv，播放不可用：...`；
- 首次播放时界面提示一次「静音模式」，队列/界面/歌词照常工作，只是不出声。
  （见 `crates/siren-app/src/ui/playback.rs` 的 `mpv_missing`。）

所以发布前请显式确认素材齐备：

```powershell
# Windows PowerShell
Get-ChildItem assets/native/windows/bin/*.dll
```

```bash
# Linux / macOS（交叉打包前确认）
ls -l assets/native/windows/bin/*.dll
```

## 从哪里获取

任选一个与目标架构匹配的 mpv Windows 构建（x86_64），取其中的 `libmpv-2.dll`：

- mpv 官方提供的 Windows 构建（例如 [mpv-winbuild](https://github.com/shinchiro/mpv-winbuild-cmake) 产出的压缩包）；
- 或自行编译 `libmpv` 并确认导出 `mpv_*` 符号（`dumpbin /exports libmpv-2.dll`）。

放入后建议跑一次冒烟验证：启动 `siren.exe` 播放任意歌曲，确认有声音且日志里没有
`未找到 libmpv`。
