# Arch Linux 打包

`siren-git`：从 GitHub 当前 HEAD 构建。项目尚未发版，暂不提供固定版本包；等有 tag 后
再加一个 `siren` 的稳定版 PKGBUILD。

## 构建安装

```bash
cd resources/package/archlinux
makepkg -si          # 构建并安装
makepkg             # 只构建，产物为 siren-git-*.pkg.tar.zst
makepkg -c          # 清理构建目录
```

## 依赖

运行时：

- `libxcb` / `libxkbcommon` / `libxkbcommon-x11`：X11 窗口与键盘。
- `wayland`：Wayland 会话（X11 / XWayland 也可运行）。
- `libglvnd` / `vulkan-icd-loader`：wgpu 在运行时加载 EGL / Vulkan，具体驱动由系统提供
  （`mesa` / `nvidia-utils` / `vulkan-radeon` / `vulkan-intel` 等）。
- `gcc-libs`。

可选：

- `mpv`：音频。应用运行时动态加载 `libmpv.so.2`，缺失时静音启动（首次播放会提示一次）。
- `noto-fonts-cjk`：中文界面字体。
- `mesa`：OpenGL 渲染回退。
- `xdg-desktop-portal`：文件选择 / 打开链接。

构建：`rust`（含 cargo）、`git`、`pkgconf`、`freetype2`、`libxcb`、`libxkbcommon`、
`libxkbcommon-x11`、`wayland`。

## 说明

- 包名 `siren-git`，`provides=('siren')`；以后加了固定版本包会与之冲突。
- `Cargo.lock` 已提交，构建用 `--locked`；rustup 的 stable 工具链即可。
- release 配置开了 LTO + `codegen-units=1`，首次构建较慢、内存占用较高（建议 ≥8 GB）。
- 桌面文件安装为 `com.cyanrhythm.Siren.desktop`，与应用的 `app_id` 一致（Wayland 靠它
  匹配图标与任务栏分组），`StartupWMClass` 供 X11 使用。
- 默认跟随上游默认分支（`master`）；想装开发分支，把 `source` 改成
  `"$pkgname::git+$url.git#branch=dev"`。
- 发布到 AUR 前补 `.SRCINFO`：`makepkg --printsrcinfo > .SRCINFO`。
