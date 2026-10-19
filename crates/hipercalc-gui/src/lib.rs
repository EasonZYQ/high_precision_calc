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
//! | [`evaluator`] | **后台求值线程**：`Session` 独占持有者，UI 永不被求值阻塞 |
//! | [`theme`] | 品牌主题与**全部**样式函数（配色、圆角、玻璃卡片） |
//! | [`fonts`] | 打包字体（等线 + 数学衬线）、字体选择、按内容挑字体 |
//! | [`keypad`] | 虚拟键盘与函数目录的**纯数据 + 纯函数**（可脱离 GUI 单测） |
//! | [`mathrender`] | 把 MathIO 文本排成真正的数学式（分数、竖排、根号、幂、π） |
//! | [`liquid`] | **液态玻璃按键外观**（shader + 圆角矩形 SDF，仿 iOS liquid glass） |
//! | [`refraction`] | 背景的"折射扭曲"shader（凸面玻璃） |
//! | [`backdrop`] | 桌面模糊（DWM 亚克力）；Windows 走手写 Win32 FFI |
//!
//! ## 求值为什么在别的线程
//!
//! 早期版本在 `update` 里直接 `session.eval(..)`，于是 `100^100^10` 会把 iced 的
//! 事件循环冻住、Windows 给标题栏挂上「(未响应)」。现在 `Session` 由
//! [`evaluator`] 里的常驻 worker 独占，UI 只发消息、拿结果 ——
//! 详见该模块头。
//!
//! ## 亚克力的启用时机
//!
//! 必须等窗口**真的创建出来**才能反查 HWND 调 DWM（窗口不存在时 `EnumWindows`
//! 找不到目标）。所以走 `window::events()` 订阅，在收到 `Event::Opened` 的那一帧
//! 才探测；探测结果无论成败都如实记录（失败则退回不透明背景，
//! 见 [`theme::set_acrylic`]）。**远程桌面下 DWM 模糊通常不生效**，那时走回退路径。
//!
//! ## 字体的启用时机（与亚克力相反）
//!
//! 字体必须**在窗口创建之前**就交给 iced（`iced::Settings::fonts`），否则
//! 渲染器不认识这些族名、文字会静默退回默认字体。所以走 `main.rs` 里的
//! `Settings`，不在运行时加载。见 [`fonts`] 模块头。

pub mod app;
pub mod backdrop;
pub mod evaluator;
pub mod fonts;
pub mod keypad;
pub mod liquid;
pub mod mathrender;
pub mod refraction;
pub mod theme;
