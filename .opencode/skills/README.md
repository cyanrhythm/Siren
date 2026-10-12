# Agent Skills

本目录存放项目使用的 AI 编程助手技能（Agent Skills），供 opencode / Claude Code 等工具按需加载。

## 目录结构

```
.opencode/skills/
├── gpui-kit/                    # 组件用法、GPUI 机制、编码规范
│   ├── SKILL.md
│   └── references/
│       ├── coding-guides.md     # 编码规范（架构/状态/命名/测试）
│       ├── conventions.md       # 组件族约定
│       ├── recipes.md           # 完整应用范例
│       ├── usage.md
│       └── gpui/                # GPUI 机制详解
│           ├── element*.md      # Element API 与模式
│           ├── entity*.md       # Entity 与状态所有权
│           ├── action.md        # Action 与快捷键
│           ├── async.md · context.md · event.md · focus-handle.md
│           ├── global.md · layout-style.md
│           └── test*.md         # UI 集成测试
└── gpui-kit-design-guides/      # 设计规范
    ├── SKILL.md
    └── references/design-guides.md
```

## 来源与许可

本目录内容来自 **[longbridge/gpui-kit](https://github.com/longbridge/gpui-kit)** 的 `skills/` 目录，
原样复制，**未作修改**。

| 项 | 说明 |
|---|---|
| 来源 | https://github.com/longbridge/gpui-kit/tree/main/skills |
| 文档许可 | [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) |
| 上游代码许可 | Apache-2.0 |
| 获取时间 | 见本目录首次提交的 git 记录 |

依据 CC BY 4.0，此处**署名 GPUI Kit** 并**链接来源**。Siren 自身以 MIT 分发，
与本目录内容的 CC BY 4.0 许可兼容。

> 如需更新，请从上游重新拉取，而非就地修改——保持与上游一致便于比对。

## 为什么放在项目里

1. **团队一致**：所有贡献者的 AI 助手看到同一套规范
2. **版本可控**：skill 更新随提交走
3. **离线可用**：不必依赖网络获取

## 如何使用

支持 Agent Skills 的工具会自动发现 `.opencode/skills/<name>/SKILL.md`
（opencode 亦兼容 `.claude/skills/` 路径）。

技能内容较长，按需加载。开始编写 GPUI 代码前，建议先读：

- `gpui-kit/references/coding-guides.md` —— **架构、状态所有权、命名、测试的强制规范**
- `gpui-kit-design-guides/references/design-guides.md` —— 布局、间距、层级、动效

> 上游 SKILL.md 明确要求：**在做架构、状态所有权、公共 API、命名或测试决策前，
> 先读编码指南。** 这些是要求，不是建议。
