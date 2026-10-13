//! HiPerCalc 图形界面（Iced 0.14）—— **骨架与环境自检**，不是正式界面。
//!
//! 这个 crate 的最终目标是把 `hipercalc` 做成图形界面。当前这个文件只是
//! **环境验证**用的最小程序，用来证明两件事：
//!
//! 1. iced 0.14 能在本机（Windows / MSVC / wgpu）编译并真的开出窗口；
//! 2. 本 crate 能链接数值底座 `hipercalc-core`，并把它算出来的高精度结果显示出来。
//!
//! 正式的界面（表达式输入框、结果区、历史、模式切换…）**尚未开始**，计划见 crate 的
//! `README.md`。写正式界面前请先确认 `hipercalc` 那边已把求值入口暴露成公开 API。
//!
//! 运行：`cargo run -p hipercalc-gui`

use iced::Center;
use iced::widget::{Column, button, column, text};

/// 应用状态（Elm 架构里的 State）。
struct State {
    /// 按钮点击次数：只为验证 `Message → update → view` 这条回路真的通了。
    clicks: u64,
    /// 由 `hipercalc-core` 现场算出的高精度结果（验证跨 crate 链接）。
    sqrt2: String,
}

impl Default for State {
    fn default() -> Self {
        // 开窗之前先让数值底座算一次 √2，取 40 位有效数字。
        // 这里刻意**不写死字符串**——值必须真的由 hipercalc-core 算出来，
        // 否则这个自检就证明不了"GUI 能调到底座"。
        let precision = hipercalc_core::bigfloat::precision();
        let sqrt2 = hipercalc_core::bigfloat::BigFloat::from_u64(2)
            .sqrt(precision)
            .to_significant_string(40);
        Self { clicks: 0, sqrt2 }
    }
}

/// 消息（Elm 架构里的 Message）。
#[derive(Debug, Clone, Copy)]
enum Message {
    Increment,
}

impl State {
    /// 更新逻辑：只改状态，不画界面。
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => self.clicks += 1,
        }
    }

    /// 视图逻辑：只读状态，产出 widget。
    fn view(&self) -> Column<'_, Message> {
        column![
            text("HiPerCalc GUI — 环境自检").size(28),
            text("下面这个值是 hipercalc-core 现场算出来的：").size(16),
            text(format!("√2 = {}", self.sqrt2)).size(16),
            button(text(format!("点一下（已点 {} 次）", self.clicks))).on_press(Message::Increment),
        ]
        .spacing(12)
        .padding(24)
        .align_x(Center)
    }
}

fn main() -> iced::Result {
    // 用 application() 而不是 run()：后者不能设窗口标题/尺寸/主题，
    // 而正式 GUI 一定要设，这里先把这条链路走通。
    iced::application(State::default, State::update, State::view)
        .title("HiPerCalc GUI — 环境自检")
        .window_size((760.0, 420.0))
        .centered()
        .run()
}
