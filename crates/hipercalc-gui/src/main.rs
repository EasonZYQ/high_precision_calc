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
    // **带任何参数 ⇒ 直接当命令行版跑**（用户要求："当 gui 程序带参数时直接启动 cli"）。
    //
    // 这样 `hipercalc-gui -e "1+1"` / `--stdin` / `-f 脚本` 全部可用 ——
    // GUI 二进制里本来就链了整套 CLI（`hipercalc::run()`），不用另带一个 exe。
    // Windows 上这一个 exe 是"窗口子系统"，所以 `run_cli_here` 里会先接管/新开控制台。
    if hipercalc_gui::cli::wants_cli() {
        std::process::exit(hipercalc_gui::cli::run_cli_here());
    }

    // ⚠️ **字体必须在建窗口之前装好**（`Settings::fonts`）。
    // iced 把这份字节交给 fontdb 解析，之后渲染器才认识这些族名。
    // 装晚了的话，文字会**静默**退回默认字体 —— 不报错，只是观感不对。
    let settings = iced::Settings {
        fonts: fonts::bundled_fonts(),
        ..iced::Settings::default()
    };

    iced::application(app::boot, app::State::update, app::State::view)
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
            // ⚠️ 尺寸与位置都要在**建窗口之前**定下来（之后再调会闪一下），
            // 所以这里先读一次设置。
            //
            // 尺寸来源有两处：上次退出时记下的**实际大小**优先（用户拖过窗口就按他拖的），
            // 没有记录才回落到"基准尺寸 × 渲染缩放"。
            size: {
                let p = hipercalc::session::GuiPrefs::load();
                let k = p.ui_scale.clamp(0.5, 1.0);
                if p.win_w > 0.0 && p.win_h > 0.0 {
                    iced::Size::new(p.win_w, p.win_h)
                } else {
                    iced::Size::new(app::WINDOW_W * k, app::WINDOW_H * k)
                }
            },
            // 位置：有记录就精确还原，没有就居中（等同于原来的 `.centered()`）
            position: {
                let p = hipercalc::session::GuiPrefs::load();
                if p.win_w > 0.0 && p.win_h > 0.0 {
                    window::Position::Specific(iced::Point::new(p.win_x, p.win_y))
                } else {
                    window::Position::Centered
                }
            },
            // 透明窗口：让 DWM 的亚克力透上来。探测失败时
            // `theme::root` 会给出不透明背景，观感依然完整。
            transparent: true,
            // **不要系统标题栏**：Win32 灰条 + 直角边框与这套玻璃界面是两种语言
            //（用户 2026-10-26："程序窗口与程序的现代化界面严重割裂"）。
            // 拖动 / 缩放 / 最小化 / 最大化 / 关闭全部由 `chrome.rs` 自绘补上。
            decorations: false,
            // 低于这个尺寸布局就挤不下了（键盘 4 行 + 输入区 + 历史区）
            min_size: Some(iced::Size::new(720.0, 640.0)),
            ..window::Settings::default()
        })
        // ⚠️ **不要**再调 `.centered()`：它会覆盖上面按设置还原的位置。
        // 没有记录时的居中已经由 `window::Position::Centered` 表达。
        .run()
}
