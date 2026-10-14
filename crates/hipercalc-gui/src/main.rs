//! 可执行入口：起 iced 事件循环。
//!
//! 界面实现全在库里（见 `lib.rs` 与 `app.rs`）—— 分成 lib + bin 两半
//! 是为了让 `tests/` 下的集成测试能驱动界面（快照、点击模拟）。

use hipercalc_gui::{app, theme};
use iced::window;

fn main() -> iced::Result {
    iced::application(app::State::default, app::State::update, app::State::view)
        .title("HiPerCalc — 超高精度计算器")
        .subscription(app::subscription)
        .theme(theme::theme)
        .window(window::Settings {
            size: iced::Size::new(880.0, 780.0),
            // 透明窗口：让 DWM 的亚克力透上来。探测失败时
            // `theme::root` 会给出不透明背景，观感依然完整。
            transparent: true,
            ..window::Settings::default()
        })
        .centered()
        .run()
}
