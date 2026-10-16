//! 可执行入口：起 iced 事件循环。
//!
//! 界面实现全在库里（见 `lib.rs` 与 `app.rs`）—— 分成 lib + bin 两半
//! 是为了让 `tests/` 下的集成测试能驱动界面（快照、点击模拟）。

// ⚠️ **Windows 上不弹控制台窗口**。
//
// 不带这个属性时，bin 走的是「控制台子系统」，双击 exe 会先弹一个黑框
//（GUI 程序的观感直接崩掉）。`windows_subsystem = "windows"` 把它切成
//「窗口子系统」，控制台不再出现。
//
// **只在非 debug 构建生效**：debug 下保留控制台，`eprintln!`（比如 DWM 亚克力
// 探测失败的报告）与 panic 回溯源才看得见 —— 开发时看不到输出会很痛苦。
// 这是 Rust 官方推荐的写法（`cfg_attr` + `not(debug_assertions)`）。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use hipercalc_gui::{app, fonts, theme};
use iced::window;

fn main() -> iced::Result {
    // ⚠️ **字体必须在建窗口之前装好**（`Settings::fonts`）。
    // iced 把这份字节交给 fontdb 解析，之后渲染器才认识这些族名。
    // 装晚了的话，文字会**静默**退回默认字体 —— 不报错，只是观感不对。
    let settings = iced::Settings {
        fonts: fonts::bundled_fonts(),
        ..iced::Settings::default()
    };

    iced::application(app::State::load, app::State::update, app::State::view)
        // `.title()` 收的是**闭包** `Fn(&State) -> String`（不是字符串），
        // 所以这里传函数名即可。标题按**启动时的语言**算 —— 语言存在设置文件里，
        // `State::load` 在建窗口之前就恢复了它（`iced::application` 的构造函数
        // 先跑，`.title` 的闭包在真正建窗口时才调用）。
        //
        // ⚠️ 之后在设置里当场换语言，标题**不会**跟着变：iced 0.14 只在建窗口时
        // 读一次标题，运行时没有 `set_title` 之类 API（详见 `Message::SetLang` 分支的注释）。
        .title(app::window_title)
        .subscription(app::subscription)
        .theme(theme::theme)
        .settings(settings)
        .window(window::Settings {
            size: iced::Size::new(940.0, 860.0),
            // 透明窗口：让 DWM 的亚克力透上来。探测失败时
            // `theme::root` 会给出不透明背景，观感依然完整。
            transparent: true,
            ..window::Settings::default()
        })
        .centered()
        .run()
}
