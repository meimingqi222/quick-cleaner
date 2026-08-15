//! QuickCleaner 核心库
//!
//! 三层结构，依赖方向严格自上而下：
//!
//! - [`ui`] —— 基于 GPUI 的视图与控件。只依赖 `core` 的领域类型和
//!   `platform` 的门面函数，不直接触碰任何操作系统 API。
//! - [`core`] —— 领域模型与跨平台业务逻辑：扫描、清理、安全规则、
//!   软件与磁盘模型。不知道 UI 的存在（配色之类属于 `ui::theme`）。
//! - [`platform`] —— 操作系统适配层。对上暴露一组固定签名的函数，
//!   由 `platform::mod` 里的契约宏在编译期校验各平台分支不漏项。

pub mod core;
pub mod platform;
pub mod ui;
