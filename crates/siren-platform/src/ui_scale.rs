//! UI 缩放（HiDPI）。
//!
//! ## 背景
//!
//! Melodia 在 JVM 上遇到过此问题：AWT 在 Linux 只支持**整数缩放**
//! （`sun.java2d.uiScale` 的分数值被静默忽略），而 KDE 常见 125% / 150%，
//! 导致界面偏小，需自行探测并补偿。
//!
//! ## Siren 的情况
//!
//! GPUI 原生处理 HiDPI，**预计无需自建探测逻辑**。
//! 但仍需在 PoC 阶段验证 KDE 分数缩放下的实际表现
//! （见 `docs/05-PoC验证清单.md` P1-3）。
//!
//! TODO: 若实测发现 GPUI 未能正确处理，再定义 trait
