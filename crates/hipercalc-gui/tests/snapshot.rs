//! 界面快照测试 —— 本项目"截图验证"的**主力手段**。
//!
//! ## 为什么不用系统抓屏
//!
//! 试过 PowerShell + `System.Drawing` 的 `CopyFromScreen`，在本机返回空 bounds、
//! `Bitmap.Save` 静默失败（受限会话下 GDI 不可用）。于是改用 iced 官方的
//! `iced_test`：它在**进程内**做真正的布局与渲染（software renderer），
//! 完全不依赖显示器，结果还能当视觉回归基线。
//!
//! ⚠️ **离屏渲染看不到桌面透出**，所以它无法证明"DWM 亚克力真的生效"。
//! 那一项要另想办法验证（见 crate README 的"验证手段"一节）。
//! 这里能验证的是：布局、配色、圆角、深浅主题、数学排版是否画对。
//!
//! ## 快照怎么更新
//!
//! `matches_image` 在**文件不存在时会自己创建**并返回 `true`。
//! 所以改完界面：删掉 `tests/snapshots/*.png` 再跑一次，就得到新基线 ——
//! 然后**必须用 Read 打开 PNG 亲眼看一下**，不能只看测试变绿。

use hipercalc_gui::app::{Message, State};

/// 快照落地目录（相对 crate 根）
const SNAP_DIR: &str = "tests/snapshots";

/// 固定视口尺寸 —— 必须与 `main.rs` 的 `window::Settings.size` 一致，
/// 否则快照里看到的布局不等于用户真实看到的布局（这类偏差最难发现）
const VIEWPORT: (f32, f32) = (880.0, 780.0);

/// 全局串行锁 —— **不是可选项**。
///
/// 明暗模式存在 `theme` 模块的进程级 `AtomicBool` 里（样式函数签名是
/// `Fn(&Theme) -> Style`，拿不到 `State`，只能这样）。而 `cargo test` 默认多线程，
/// 于是"深色测试刚设成 true、浅色测试刚要渲染"就会互相覆盖 ——
/// 实测症状是浅色快照渲染出深色界面（第一次跑就踩到了）。
///
/// 所以凡是读写进程级开关的测试都先抢这把锁，把它变成串行。
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 抢到串行锁。`Mutex` 中毒说明**前一个持锁测试 panic 了**，
/// 此时后续测试已经不可能有干净的全局状态，直接把中毒传下去更安全
/// （用 `unwrap_or_else` 吞掉会让失败被掩盖）。
fn lock() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

/// 造一个"已经算过几题"的状态，让快照能体现真实内容
/// （空界面看不出分数竖排、根号、颜色语义对不对）。
fn populated_state() -> State {
    let mut s = State::default();
    // 覆盖几类典型结果：
    // - 分数（竖排 + 横线）
    // - 根号（√ + 顶线）+ 分数相乘
    // - 带 π 的分数
    // - 错误（红色语义）
    for input in [
        "1/2",
        "(1 / 3)*sqrt(2)",
        "(5 / 6)*pi",
        "sqrt(2)+sqrt(3)",
        "1+",
    ] {
        s.set_input_for_test(input);
        let _ = s.update(Message::Evaluate);
    }
    s
}

#[test]
fn dark_theme_snapshot() {
    let _guard = lock();
    let mut state = populated_state();
    // 深色是默认，但显式写出来，免得以后改默认值时这条测试悄悄换了主题
    state.set_dark_for_test(true);

    let snapshot = snap(&state);
    let path = format!("{SNAP_DIR}/dark.png");
    assert!(
        snapshot.matches_image(&path).expect("快照比对/写入失败"),
        "深色主题界面与基线不一致：{path}"
    );
}

#[test]
fn light_theme_snapshot() {
    let _guard = lock();
    let mut state = populated_state();
    state.set_dark_for_test(false);

    let snapshot = snap(&state);
    let path = format!("{SNAP_DIR}/light.png");
    assert!(
        snapshot.matches_image(&path).expect("快照比对/写入失败"),
        "浅色主题界面与基线不一致：{path}"
    );
}

/// 纯 LineIO（小数）模式：数学排版应退化为等宽文本，且不误判结构
#[test]
fn lineio_snapshot() {
    let _guard = lock();
    let mut state = populated_state();
    state.set_display_mode_for_test(hipercalc::parser_api::DisplayMode::LineIO);
    state.set_dark_for_test(true);

    let snapshot = snap(&state);
    let path = format!("{SNAP_DIR}/lineio.png");
    assert!(
        snapshot.matches_image(&path).expect("快照比对/写入失败"),
        "LineIO 界面与基线不一致：{path}"
    );
}

/// 函数弹窗展开态：验证 `stack` 浮层与函数目录的排布
#[test]
fn function_panel_snapshot() {
    let _guard = lock();
    let mut state = populated_state();
    state.set_dark_for_test(true);
    let _ = state.update(Message::ToggleFuncs);

    let snapshot = snap(&state);
    let path = format!("{SNAP_DIR}/funcs.png");
    assert!(
        snapshot.matches_image(&path).expect("快照比对/写入失败"),
        "函数弹窗界面与基线不一致：{path}"
    );
}

/// 空界面（首启动）：应显示引导文案而不是一片空白
#[test]
fn empty_state_snapshot() {
    let _guard = lock();
    let mut state = State::default();
    state.set_dark_for_test(true);

    let snapshot = snap(&state);
    let path = format!("{SNAP_DIR}/empty.png");
    assert!(
        snapshot.matches_image(&path).expect("快照比对/写入失败"),
        "空界面与基线不一致：{path}"
    );
}

/// 把状态渲染成一张离屏快照。
///
/// 视口尺寸**必须固定**：`Simulator::new` 用的是 `window::Settings::default().size`，
/// 一旦上游改了默认值，所有基线会同时失配且原因极难定位。
/// 这里用 `with_size` 显式覆盖成窗口的实际初始化尺寸（与 `main.rs` 的
/// `window::Settings` 保持一致）。
///
/// ⚠️ `iced_test::Settings` 在 crate 根**没有** re-export（它其实是
/// `iced_core::Settings`，管默认字体/字体列表），要从 `iced_test::core` 拿；
/// `Snapshot` 则只存在于 `iced_test::simulator` 模块里。
fn snap(state: &State) -> iced_test::simulator::Snapshot {
    use iced_test::Simulator;

    let mut ui = Simulator::with_size(
        iced_test::core::Settings::default(),
        iced::Size::new(VIEWPORT.0, VIEWPORT.1),
        state.view_for_test(),
    );
    ui.snapshot(&state.theme_for_test()).expect("离屏渲染失败")
}

/// 键盘区必须真的可点 —— 用真实的按键消息走完整回路
/// （比"看一眼快照"强：它会真的走 Message → update → session.eval）。
#[test]
fn keypad_is_clickable_through_simulator() {
    let _guard = lock();
    use hipercalc_gui::keypad::LAYOUT;

    /// 按键面上文字定位按键下标
    fn pos_of(label: &str) -> (usize, usize) {
        LAYOUT
            .iter()
            .enumerate()
            .find_map(|(ri, row)| row.iter().position(|k| k.label == label).map(|ci| (ri, ci)))
            .unwrap_or_else(|| panic!("键盘里应有按键 {label}"))
    }

    let mut state = State::default();
    // 依次点 `1` `÷` `2` `=`，等价于输入 `1/2`
    for label in ["1", "÷", "2"] {
        let (r, c) = pos_of(label);
        let _ = state.update(Message::Key(r, c));
    }
    let (r, c) = pos_of("=");
    let _ = state.update(Message::Key(r, c));

    assert_eq!(state.history_len_for_test(), 1, "点 `=` 后应完成一次求值");
    let out = state.last_output_for_test();
    assert!(
        out.contains("1 / 2") || out.contains("0.5"),
        "键盘输入 `1÷2` 的结果应含 `1 / 2` 或 `0.5`，实际：{out}"
    );
}
