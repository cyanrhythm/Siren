# 贡献指南

感谢参与 Siren。本文说明本项目的开发约定与提交流程。

---

## 开发环境

### 依赖

- **Rust**：stable 通道（通过 [rustup](https://rustup.rs) 安装）
- **Linux 额外需要**：

  ```bash
  # Arch
  sudo pacman -S libmpv base-devel

  # Debian / Ubuntu
  sudo apt install libmpv-dev build-essential
  ```

- **Windows**：MSVC 工具链
- **macOS**：Xcode Command Line Tools

### 常用命令

```bash
cargo build --workspace        # 构建
cargo test --workspace         # 测试
cargo run -p siren-app         # 运行

cargo fmt --all                # 格式化
cargo clippy --workspace --all-targets -- -D warnings   # 静态检查
```

---

## 提交规范

本项目使用 **Conventional Commits**。提交信息格式：

```
<type>(<scope>): <简短描述>

<正文：做了什么、为什么>

<页脚：关联 Issue>
```

### type

| type | 用途 |
|---|---|
| `feat` | 新功能 |
| `fix` | 修复 |
| `refactor` | 重构 |
| `perf` | 性能 |
| `docs` | 文档 |
| `test` | 测试 |
| `build` | 构建 / 依赖 |
| `ci` | CI 配置 |
| `chore` | 杂项 |

### scope

`app`（应用层）· `core`（跨平台核心）· `platform`（平台能力）·
`linux` · `windows` · `macos` · `net` · `player` · `lyrics` · `deps`

### 启用提交模版（可选）

仓库内提供了 `.gitmessage` 模版，可在本地启用：

```bash
git config commit.template .gitmessage
```

### 示例

```
feat(player): 新增流式播放边缓存

fix(platform): 修正 MPRIS 注册顺序，避免 KDE 忽略播放器

根因：KDE 的 libkmpris 监听 NameOwnerChanged，名称出现即发起 GetAll，
而此前先抢总线名后导出对象，导致首次抓取失败、播放器被永久忽略。
改为先导出对象再注册总线名。

Closes #12
```

---

## 提交 PR

创建 PR 时按改动类型选择模板（在创建地址后追加参数）：

| 类型 | 参数 |
|---|---|
| 功能 | `?template=feature.md` |
| 修复 | `?template=fix.md` |
| 重构 | `?template=refactor.md` |
| 其他（文档 / 构建 / 依赖） | 使用默认模板 |

---

## 架构约束

**这是本项目最重要的约定：平台差异必须收敛在 `siren-platform`。**

| crate | 是否允许 `#[cfg(target_os)]` |
|---|---|
| `siren-app` | ❌ 禁止 |
| `siren-core` | ❌ 禁止 |
| `siren-platform` | ✅ **仅此处允许** |

上层通过 `siren-platform` 暴露的 trait 调用平台能力，由编译期决定具体实现。

详见 [`docs/01-架构设计.md`](docs/01-架构设计.md)。

### 新增平台能力时

1. 在 `crates/siren-platform/src/` 下定义 trait（与平台无关）
2. 在 `windows/`、`linux/`、`macos/` 分别实现
3. 在 `lib.rs` 中通过 `#[cfg]` 补充统一导出

> 若漏掉某个平台，**编译会失败**——这是设计意图，而非疏漏。

### Linux 的额外注意

X11 与 Wayland 的差异**不是** GUI 工具包能抹平的：

| 能力 | X11 | Wayland |
|---|---|---|
| 全局热键 | ✅ | ⚠️ 仅 XDG Portal |
| 窗口点击穿透 | ✅ | ❌ 无协议 |
| 系统媒体控制 | ✅ | ✅（D-Bus，与显示协议无关） |
| 托盘 | ✅ | ✅（D-Bus，与显示协议无关） |

不可实现的能力应**明确降级并在界面提示**，而非静默失败。

详见 [`docs/02-平台能力规格.md`](docs/02-平台能力规格.md)。

---

## 已知陷阱

实现平台能力前请先阅读：

| 主题 | 位置 |
|---|---|
| Linux MPRIS 的 8 个坑 | [`docs/02-平台能力规格.md § 3.4`](docs/02-平台能力规格.md) |
| 网易云加密算法要点 | [`docs/04-迁移规格.md § 5`](docs/04-迁移规格.md) |
| 平台选择的经验教训 | [`docs/04-迁移规格.md`](docs/04-迁移规格.md) |

---

## AI 辅助开发

项目内置了 **GPUI Kit 官方 Agent Skills**，位于 [`.opencode/skills/`](.opencode/skills/)：

| Skill | 内容 |
|---|---|
| `gpui-kit` | 组件用法、GPUI 机制（Element / Entity / Action / 测试）、**编码规范** |
| `gpui-kit-design-guides` | 设计规范（布局、间距、层级、色彩、动效） |

支持 Agent Skills 的工具（opencode / Claude Code 等）会自动发现并加载。

> ⚠️ 上游明确要求：**在做架构、状态所有权、公共 API、命名或测试决策前，
> 先读 `gpui-kit/references/coding-guides.md`。** 这些是强制规范，不是参考建议。

这些文件来自 [longbridge/gpui-kit](https://github.com/longbridge/gpui-kit)（CC BY 4.0），
**请勿就地修改**——如需更新请从上游重新拉取。详见 `.opencode/skills/README.md`。

---

## 自检清单

提交前请确认：

- [ ] `cargo fmt --all -- --check` 通过
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` 无警告
- [ ] `cargo test --workspace` 通过
- [ ] 平台差异未泄漏到 `siren-core` / `siren-app`
- [ ] 新增平台能力已补齐三平台实现
- [ ] 没有提交临时文件、调试代码、密钥
