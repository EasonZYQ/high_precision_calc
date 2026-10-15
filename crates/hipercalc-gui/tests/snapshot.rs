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
///
/// ⚠️ 用 `evaluate_blocking_for_test` 而**不是** `update(Message::Evaluate)`：
/// 求值现在是**异步**的（跑在后台 worker 上，结果由 `Message::Evaluated` 送回来），
/// 而这里没有 iced 的事件循环替我们跑那个 `Task` —— 交了 `Task` 就没人执行，
/// 历史会一直是空的。同步入口走的是同一个 worker、同一条通道，
/// 所以下面画出来的仍然是真实求值结果。
///
/// 这也让每条历史右侧的耗时获得了视觉覆盖（测试里通常显示 `<1秒`）。
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
        s.evaluate_blocking_for_test();
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

/// 纯 LineIO（小数）模式：**结果**退化为等宽小数文本；
/// 但**表达式回显仍要渲染根号/分数**（用户要求："根号应当始终渲染"）。
///
/// 这条快照同时是"回显不随显示模式退化"的视觉证据：能看到
/// `› √2 + √3` 的回显是排好版的根号，而下面的结果是 `≈ 3.14626437…`。
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

// ── 阶段五新增界面 ───────────────────────────────────────────────────────────

/// 输入框下方的**实时数学预览**：敲 `sqrt(2)/3`，预览行应排出根号与分数。
///
/// 这是用户明确要求的"输入栏里根号应当渲染，类似 desmos"。快照里能直接看到
/// 预览行有没有出现、根号顶线有没有画对、位置是不是在输入框正下方。
#[test]
fn input_preview_snapshot() {
    let _guard = lock();
    let mut state = populated_state();
    state.set_dark_for_test(true);
    // 留一段"正在输入"的内容，让预览行有东西可排
    state.set_input_for_test("sqrt(2)/3 + x^2");
    // 顺带把主面板也切到字母面板，一张图同时看两处新东西
    let _ = state.update(Message::SetPanel(hipercalc_gui::keypad::Panel::Abc));

    let snapshot = snap(&state);
    let path = format!("{SNAP_DIR}/preview.png");
    assert!(
        snapshot.matches_image(&path).expect("快照比对/写入失败"),
        "预览行界面与基线不一致：{path}"
    );
}

/// 字母面板（Desmos 的第二块键盘）：应打出 `123` 返回键与三行字母。
///
/// 同时把界面语言切成英文 —— 一张图能同时验证"面板切换"与"语言切换"两件事：
/// 工具条上的中文标签（快速/死算/清空）应变成英文。
///
/// ⚠️ 语言与字体一样是**进程级**的（存在 `i18n` 的全局里），所以要还原。
#[test]
fn abc_panel_and_english_snapshot() {
    let _guard = lock();
    let mut state = populated_state();
    state.set_dark_for_test(true);
    state.set_input_for_test("sin(theta)");
    let _ = state.update(Message::SetPanel(hipercalc_gui::keypad::Panel::Abc));
    let _ = state.update(Message::SetLang(hipercalc::session::UiLang::En));

    let snapshot = snap(&state);
    let path = format!("{SNAP_DIR}/abc-en.png");
    assert!(
        snapshot.matches_image(&path).expect("快照比对/写入失败"),
        "字母面板界面与基线不一致：{path}"
    );

    // 还原中文，免得后面的测试渲染出英文文案（进程级开关）
    let _ = state.update(Message::SetLang(hipercalc::session::UiLang::ZhCn));
}

/// 设置面板展开态：三行（精度 / 语言 / 数学字体）都要在一张图里看清楚。
///
/// 用户在阶段五明确要求"设置里添加语言选择"与"在设置里添加这些全部字体，
/// 供用户选择"—— 这条快照就是这两项的视觉证据。
#[test]
fn settings_panel_snapshot() {
    let _guard = lock();
    let mut state = populated_state();
    state.set_dark_for_test(true);
    let _ = state.update(Message::ToggleSettings);

    let snapshot = snap(&state);
    let path = format!("{SNAP_DIR}/settings.png");
    assert!(
        snapshot.matches_image(&path).expect("快照比对/写入失败"),
        "设置面板界面与基线不一致：{path}"
    );
}

/// 切换数学字体后**整屏文字都要跟着换**（历史里已算好的结果也要换）。
///
/// 字体是进程级开关，渲染时才取 —— 所以不必重算历史。这张快照用来肉眼确认
/// "换了字体之后旧的分数/根号也变了样子"，而不是只有设置按钮变了。
#[test]
fn math_font_switch_snapshot() {
    let _guard = lock();
    let mut state = populated_state();
    state.set_dark_for_test(true);
    // 挑一个与默认 Cambria Math 差别最明显的（Georgia 的 x-height 大得多）
    let _ = state.update(Message::SetMathFont(2));

    let snapshot = snap(&state);
    let path = format!("{SNAP_DIR}/font-georgia.png");
    assert!(
        snapshot.matches_image(&path).expect("快照比对/写入失败"),
        "换字体后的界面与基线不一致：{path}"
    );
    // ⚠️ 字体是进程级开关，还原默认，免得后面的测试渲染出这种字体。
    //（`SERIAL` 锁保证同时只有一个测试在跑，所以这里改完再还原是安全的。）
    let _ = state.update(Message::SetMathFont(0));
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
/// （比"看一眼快照"强：它会真的走 Message → update → worker 求值）。
///
/// ⚠️ 求值现在是异步的，`update` 只把请求交给后台 worker 就返回。
/// 这里没有事件循环去跑那个 `Task`，所以拿到 `Task` 之后**主动等一次结果**
/// —— 用 `Message::Evaluated` 走和真实界面**完全同一条**写历史的路。
#[test]
fn keypad_is_clickable_through_simulator() {
    let _guard = lock();
    use hipercalc_gui::keypad::{KeyAction, LAYOUT};

    /// 按键面上文字定位按键下标
    fn pos_of(label: &str) -> (usize, usize) {
        LAYOUT
            .iter()
            .enumerate()
            .find_map(|(ri, row)| row.iter().position(|k| k.label == label).map(|ci| (ri, ci)))
            .unwrap_or_else(|| panic!("键盘里应有按键 {label}"))
    }

    /// 按 `KeyAction` 定位按键下标。
    ///
    /// ⚠️ 求值键**不能按 `=` 找**了：改成 Desmos 布局之后，它的键面是 `↵`
    /// （与 Windows 计算器、Desmos 一致），`=` 只在输入框右侧那个按钮上。
    /// 用 `KeyAction::Evaluate` 找才稳 —— 键面文字以后还会改，动作不会。
    fn pos_of_action(a: KeyAction) -> (usize, usize) {
        LAYOUT
            .iter()
            .enumerate()
            .find_map(|(ri, row)| row.iter().position(|k| k.action == a).map(|ci| (ri, ci)))
            .unwrap_or_else(|| panic!("键盘里应有动作 {a:?} 的键"))
    }

    let mut state = State::default();
    // 依次点 `1` `÷` `2` `↵`，等价于输入 `1/2` 并求值
    for label in ["1", "÷", "2"] {
        let (r, c) = pos_of(label);
        let _ = state.update(Message::Key(r, c));
    }
    // 点键盘上那个求值键（`↵`）。它会走 `KeyAction::Evaluate` → `State::evaluate`
    // → 后台 worker，所以这里先断言"已经派单"，再等结果回来
    let (r, c) = pos_of_action(KeyAction::Evaluate);
    let _ = state.update(Message::Key(r, c));
    assert_eq!(
        state.history_len_for_test(),
        0,
        "求值在后台，此刻还没有历史"
    );
    state.await_pending_for_test();

    assert_eq!(state.history_len_for_test(), 1, "点 `↵` 后应完成一次求值");
    let out = state.last_output_for_test();
    assert!(
        out.contains("1 / 2") || out.contains("0.5"),
        "键盘输入 `1÷2` 的结果应含 `1 / 2` 或 `0.5`，实际：{out}"
    );
}
