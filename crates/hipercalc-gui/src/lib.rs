//! HiPerCalc 图形界面（Iced 0.14）—— WinUI3 风格。
//!
//! 这个文件是**库入口**（`lib.rs`）：把各模块对外可见，
//! 好让 `tests/` 下的集成测试能驱动界面（快照、点击模拟）。
//! 真正的可执行入口是 `main.rs`，它只负责调用本库 + 起 iced 事件循环。
//!
//! ## 模块职责
//!
//! | 模块 | 职责 |
//! | --- | --- |
//! | [`app`] | 界面主体：`State` / `Message` / `update` / `view` / `subscription` |
//! | [`theme`] | 品牌主题与**全部**样式函数（配色、圆角、玻璃卡片） |
//! | [`keypad`] | 虚拟键盘与函数目录的**纯数据 + 纯函数**（可脱离 GUI 单测） |
//! | [`mathrender`] | 把 MathIO 文本排成真正的数学式（分数、竖排、根号、幂、π） |
//! | [`backdrop`] | 桌面模糊（DWM 亚克力）；Windows 走手写 Win32 FFI |
//!
//! ## 亚克力的启用时机
//!
//! 必须等窗口**真的创建出来**才能反查 HWND 调 DWM（窗口不存在时 `EnumWindows`
//! 找不到目标）。所以走 `window::events()` 订阅，在收到 `Event::Opened` 的那一帧
//! 才探测；探测结果无论成败都如实记录（失败则退回不透明背景，
//! 见 [`theme::set_acrylic`]）。**远程桌面下 DWM 模糊通常不生效**，那时走回退路径。

pub mod app;
pub mod backdrop;
pub mod keypad;
pub mod mathrender;
pub mod theme;
