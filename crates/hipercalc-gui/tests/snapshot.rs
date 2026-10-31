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
//! ⚠️ **基线文件名带 `-wgpu` 后缀**（`matches_image` 内部会把
//! `funcs.png` 换成 `<stem>-<renderer>.png`，即 `funcs-wgpu.png`）。
//! 上次改函数面板时对着不存在的 `funcs.png` 删了半天、以为"改了但画面不变"，
//! 实际基线一直是旁边的 `funcs-wgpu.png` —— 换基线要删它，看效果也要 Read 它。
//!
//! `matches_image` 在**文件不存在时会自己创建**并返回 `true`。
//! 所以改完界面：删掉 `tests/snapshots/*-wgpu.png` 再跑一次，就得到新基线 ——
//! 然后**必须用 Read 打开 PNG 亲眼看一下**，不能只看测试变绿。

use hipercalc_gui::app::{Message, State};

/// 快照落地目录（相对 crate 根）
const SNAP_DIR: &str = "tests/snapshots";

/// 固定视口尺寸 —— 必须与 `main.rs` 的 `window::Settings.size` 一致，
/// 否则快照里看到的布局不等于用户真实看到的布局（这类偏差最难发现）
const VIEWPORT: (f32, f32) = (940.0, 900.0);

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

/// **三角形求解页**（用户 2026-10-27 要求的独立界面）
/// **「自定义函数与变量」页**（用户 2026-10-31 要求）。
///
/// 覆盖三种状态：函数列表有内容、变量列表有内容、输入框里正打着一条定义。
#[test]
fn userdefs_page_snapshot() {
    let _guard = lock();
    let mut state = populated_state();
    state.set_dark_for_test(true);
    let _ = state.update(Message::ToggleUserDefs);
    // 定义一条函数 + 一个变量，再在输入框留一条待提交的内容
    let _ = state.update(Message::UdfChanged("f(x)=x^2+1".into()));
    let _ = state.update(Message::UdfSubmit);
    state.await_pending_for_test();
    let _ = state.update(Message::UdfChanged("A=3".into()));
    let _ = state.update(Message::UdfSubmit);
    state.await_pending_for_test();
    let _ = state.update(Message::UdfChanged("g(x,y)=x*y".into()));

    let snapshot = snap(&state);
    let path = format!("{SNAP_DIR}/userdefs.png");
    assert!(
        snapshot.matches_image(&path).expect("快照比对/写入失败"),
        "自定义函数与变量页与基线不一致：{path}"
    );
}

#[test]
fn triangle_page_snapshot() {
    let _guard = lock();
    let mut state = populated_state();
    state.set_dark_for_test(true);
    let _ = state.update(Message::ToggleTriangle);
    // 填两个已知量，顺便看输入框/按钮排布
    // 填 a=3 b=4 mA=4 并**真的求解一次**：快照要覆盖"结果就地显示"这一块
    let _ = state.update(Message::TriChanged(0, "3".into()));
    let _ = state.update(Message::TriChanged(1, "4".into()));
    let _ = state.update(Message::TriChanged(6, "4".into()));
    let _ = state.update(Message::TriSolve);
    state.await_pending_for_test();

    let snapshot = snap(&state);
    let path = format!("{SNAP_DIR}/triangle.png");
    assert!(
        snapshot.matches_image(&path).expect("快照比对/写入失败"),
        "三角形求解页与基线不一致：{path}"
    );
}

/// **窗口拉宽后设置面板要重排**（用户 2026-10-29："加上拖动窗口重建 UI"）。
///
/// 940 宽时"数学字体"那一行装不下、折成两行；拉到 1400 宽应当**合并回一行**，
/// 面板的最大高度也跟着视口变高。
#[test]
fn settings_wide_snapshot() {
    let _guard = lock();
    let (w, h) = (1400.0f32, 900.0f32);
    let mut state = populated_state();
    state.set_dark_for_test(true);
    state.set_viewport_for_test(w, h);
    let _ = state.update(Message::ToggleSettings);

    let snapshot = snap_sized(&state, w, h);
    let path = format!("{SNAP_DIR}/settings-wide.png");
    assert!(
        snapshot.matches_image(&path).expect("快照比对/写入失败"),
        "宽窗口下的设置面板与基线不一致：{path}"
    );
}

/// **自绘标题栏（最大化态）**：第三个按钮要画成"还原"（两个错开的小方块）。
///
/// 标题栏是窗口装饰之外的唯一"窗口级 UI"，它的三种图标（最小化 / 最大化 /
/// 还原 / 关闭）都要有视觉回归 —— 平时快照只能看到"最大化"那一种
///（默认未最大化），所以单独拍一张最大化态的。
#[test]
fn titlebar_maximized_snapshot() {
    let _guard = lock();
    let mut state = State::default();
    state.set_dark_for_test(true);
    state.set_maximized_for_test(true);

    let snapshot = snap(&state);
    let path = format!("{SNAP_DIR}/titlebar-max.png");
    assert!(
        snapshot.matches_image(&path).expect("快照比对/写入失败"),
        "标题栏（最大化态）与基线不一致：{path}"
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

/// Σ / Π 大符号 + **空参数占位框**（用户本轮要求的两项渲染）。
///
/// 输入区故意留一个空参数（`sum(k^2, k, 1, )`）：应当看到
/// Σ 的上方是上限、下方是 `k = 1`、主体是 `k²`，而第 4 个位置是一个**灰色小方块**
///（点了它光标会跳进去 —— 那是"预览即输入栏"的交互核心）。
///
/// ⚠️ 这条快照是**必须肉眼看的**：Σ 的上下限竖排、占位框的大小与颜色，
/// 都是"测试变绿但看着不对"最容易漏掉的地方。
#[test]
fn bigop_and_placeholder_snapshot() {
    let _guard = lock();
    let mut state = populated_state();
    state.set_dark_for_test(true);
    // 还差最后一个参数没打 ⇒ 那个位置应是占位框
    state.set_input_for_test("∑()");
    let snapshot = snap(&state);
    let path = format!("{SNAP_DIR}/bigop.png");
    assert!(
        snapshot.matches_image(&path).expect("快照比对/写入失败"),
        "Σ 大符号与占位框的渲染与基线不一致：{path}"
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

/// **玻璃质感下的液态玻璃按键**（用户要求："虚拟键盘的按键和顶部的选项都要液态玻璃"）。
///
/// ⚠️ 这两张是那套外观的**唯一视觉证据**：默认质感是亚克力（`GuiPrefs::default`），
/// 而液态玻璃只在 `Texture::Glass` 下启用 —— 其他 10 张快照里根本看不到它们。
/// 所以改动 `crate::liquid` 之后，必须来这里肉眼确认。
fn liquid_glass(dark: bool) {
    let _guard = lock();
    let mut state = populated_state();
    state.set_dark_for_test(dark);
    // ⚠️ **顺序不能反**：`State::default()` 自己会 `theme::set_texture(prefs.texture)`，
    // 先设再建 State 会被它覆盖回亚克力（与 `identical_backdrop_is_dropped` 同一个坑）。
    let _ = state.update(Message::SetTexture(hipercalc_gui::theme::Texture::Glass));

    let snapshot = snap(&state);
    let path = format!("{SNAP_DIR}/liquid-{}", if dark { "dark" } else { "light" });
    assert!(
        snapshot.matches_image(&path).expect("快照比对/写入失败"),
        "玻璃质感（{}）界面与基线不一致：{path}",
        if dark { "深色" } else { "浅色" }
    );
    // 还原默认质感（进程级开关，不还原会污染后面的测试）
    let _ = state.update(Message::SetTexture(hipercalc_gui::theme::Texture::Acrylic));
}

#[test]
fn liquid_glass_dark_snapshot() {
    liquid_glass(true);
}

#[test]
fn liquid_glass_light_snapshot() {
    liquid_glass(false);
}

/// **玻璃质感下的设置面板**（用户反馈的 bug："设置里的按钮在玻璃质感下选中
/// 会变成白色且不显示文字"）。
///
/// 这张快照是该修复的**视觉证据**：玻璃模式 + 打开设置，能看到
/// - 设置面板本体跟随玻璃质感（半透明）；
/// - 所有 chip 垫液态玻璃层；
/// - 选中的 chip 是 **accent 蓝文字**（修复前是"白底白字"直接消失）。
#[test]
fn liquid_glass_settings_snapshot() {
    let _guard = lock();
    let mut state = populated_state();
    state.set_dark_for_test(true);
    // ⚠️ 顺序不能反：先建 State 再切玻璃（见 `liquid_glass` 里的说明）
    let _ = state.update(Message::SetTexture(hipercalc_gui::theme::Texture::Glass));
    let _ = state.update(Message::ToggleSettings);

    let snapshot = snap(&state);
    let path = format!("{SNAP_DIR}/liquid-settings.png");
    assert!(
        snapshot.matches_image(&path).expect("快照比对/写入失败"),
        "玻璃质感（设置面板）界面与基线不一致：{path}"
    );
    let _ = state.update(Message::SetTexture(hipercalc_gui::theme::Texture::Acrylic));
}

/// **结构化输入框**：Σ 的四个槽位是四个**不同**的位置（本轮改动的视觉证据）。
///
/// 旧模型（文本 + 字符偏移光标）在这里有两个硬伤，都拍成了回归测试：
/// 1. 四个空参数框算出来是**同一个**文本偏移 ⇒ 点哪个都"没反应"；
/// 2. 光标能停在 `(` 那个位置上 ⇒ 光标无处可画、直接消失，接着打字变成 `∑x()`。
///
/// 这张图要看三件事：
/// - 主体 `k²`、变量 `k`、下限 `1` 各就各位，**上限**是一个可点的空方框；
/// - 光标（细竖线）画在**上限那个槽里**，而不是浮在括号上；
/// - 整块排版**变高**了，输入区跟着长高而不是把 Σ 裁掉。
#[test]
fn structured_input_bigop_snapshot() {
    let _guard = lock();
    let mut state = populated_state();
    state.set_dark_for_test(true);
    // 上限留空 ⇒ 光标优先落在第一个空槽（= 上限），正是要展示的那个位置
    state.set_input_for_test("∑(k^2,k,1,)");

    let path = format!("{SNAP_DIR}/input-bigop.png");
    let snapshot = snap(&state);
    assert!(
        snapshot.matches_image(&path).expect("快照比对/写入失败"),
        "结构化输入框（Σ）与基线不一致：{path}"
    );
}

/// **玻璃质感 + 背景**：四周折射与按键折射的**唯一视觉证据**。
///
/// ⚠️ 另外那几张玻璃快照里 `bg_pixels` 是 `None`（离屏测试没有窗口可抓屏），
/// 于是着色器走 `has_bg = 0` 那条路 —— **折射根本没参与渲染**，那几张图
/// 证明不了"边缘折射"。这里注入一张高对比的合成背景（16px 棋盘），
/// 让面板四周与按键边缘的弯曲、色散真的显形。
///
/// ## 为什么必须是棋盘
///
/// 弯曲体现为"条纹被推歪"。纯色或渐变背景里，把 uv 挪几个像素看不出来；
/// 棋盘一旦被挪，格子边立刻错开 —— 肉眼一眼就能判断折射有没有生效、
/// 弯的方向对不对。
#[test]
fn liquid_glass_refraction_snapshot() {
    let _guard = lock();
    let mut state = populated_state();
    state.set_dark_for_test(true);
    // ⚠️ 顺序不能反：先建 State 再切玻璃（见 `liquid_glass` 里的说明）
    let _ = state.update(Message::SetTexture(hipercalc_gui::theme::Texture::Glass));

    // 合成背景：**双色网格 + 一条斜线**（用户建议："把测试里的截图背景改为两种颜色
    // 相间的网格，来测试玻璃质感的效果"）。
    //
    // 为什么是网格 + 斜线：网格能看出"哪一段被弯了多少"，斜线能看出
    // "颜色交界有没有锯齿"——这两个正是玻璃质感最容易糊弄过去的地方。
    //
    // ⚠️ `width / height` 必须**真的等于** `rgba` 的尺寸 —— 着色器按
    // `width * 4` 当行距上传，给错了整张图会错位（这里曾经踩过：见
    // `Backdrop::rw` 那段"纹理像素尺寸 ≠ 抓屏区域尺寸"的说明）。
    let (w, h) = (240u32, 180u32);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            // 双色网格：24px 一格，冷暖强烈对比（弯折与色散都吃得出来）
            let hi = ((x / 24) + (y / 24)) % 2 == 0;
            // ⚠️ 严格**两种颜色**（用户要求）：棋盘自身的对角台阶就足以看出锯齿。
            let c = if hi { [36u8, 64, 140] } else { [238, 158, 52] };
            let i = ((y * w + x) * 4) as usize;
            rgba[i] = c[0];
            rgba[i + 1] = c[1];
            rgba[i + 2] = c[2];
            rgba[i + 3] = 255;
        }
    }
    let _ = state.update(Message::BgCaptured(Some(
        hipercalc_gui::backdrop::Backdrop {
            x: 0,
            y: 0,
            width: w,
            height: h,
            // 抓屏"区域"就是整块视口 ⇒ uv 映射退化成恒等（与离屏渲染一致）
            rw: VIEWPORT.0 as u32,
            rh: VIEWPORT.1 as u32,
            rgba,
        },
    )));

    let snapshot = snap(&state);
    let path = format!("{SNAP_DIR}/liquid-refraction.png");
    assert!(
        snapshot.matches_image(&path).expect("快照比对/写入失败"),
        "玻璃质感（折射）界面与基线不一致：{path}"
    );
    let _ = state.update(Message::SetTexture(hipercalc_gui::theme::Texture::Acrylic));
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
    snap_sized(state, VIEWPORT.0, VIEWPORT.1)
}

/// 指定尺寸渲染（窗口被拉宽/收窄时用）
fn snap_sized(state: &State, w: f32, h: f32) -> iced_test::simulator::Snapshot {
    use iced_test::Simulator;

    let mut ui = Simulator::with_size(
        iced_test::core::Settings::default(),
        iced::Size::new(w, h),
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

// ─────────────────────────────────────────────────────────────────────────
// README 素材（不是回归基线）
// ─────────────────────────────────────────────────────────────────────────

/// **合成背景：粗斜线**（用户 2026-10-28 要求）。
///
/// 不用真机截图的原因：`允许被截图 = 关` 时窗口**根本拍不到**
///（`WDA_EXCLUDEFROMCAPTURE`），开着拍又会把自己的桌面内容（别人的窗口、
/// 聊天记录）带进开源仓库 —— 不合适。
///
/// 为什么是斜线：渐变/光斑看不出"折射把线弯成什么角度"，而 45° 粗条
/// 一眼就能判断折痕在哪、方向有没有反过来。颜色刻意压得暗一些，
/// 免得把玻璃上的字冲淡（试过调亮，整屏发灰）。
fn stripes_like(w: u32, h: u32) -> Vec<u8> {
    let mut out = vec![0u8; (w * h * 4) as usize];
    // 两组斜线（45° 与 -45°），间距按"短边"算，缩放窗口时看起来一致
    let short = w.min(h) as f32;
    let period = short * 0.22;
    let width = period * 0.42;
    for y in 0..h {
        let fy = y as f32;
        for x in 0..w {
            let fx = x as f32;
            // 主族：↗ 深蓝；副族：↘ 暖橙
            let a = ((fx + fy) / period).fract();
            let b = ((fx - fy + 10.0 * period) / (period * 1.7)).fract();
            // ⚠️ 别调亮：试过 52/100/176，玻璃一叠加整屏发灰、白字对比度掉得很厉害。
            // 现在这组是"能看出斜线、又不抢界面"的档位。
            let mut c = [20.0f32, 27.0, 44.0];
            if a * period < width {
                c = [34.0, 70.0, 132.0];
            }
            if b * (period * 1.7) < width * 0.45 {
                c = [
                    c[0] * 0.55 + 190.0 * 0.45,
                    c[1] * 0.55 + 120.0 * 0.45,
                    c[2] * 0.55 + 60.0 * 0.45,
                ];
            }
            let i = ((y * w + x) * 4) as usize;
            out[i] = c[0].clamp(0.0, 255.0) as u8;
            out[i + 1] = c[1].clamp(0.0, 255.0) as u8;
            out[i + 2] = c[2].clamp(0.0, 255.0) as u8;
            out[i + 3] = 255;
        }
    }
    out
}

/// 把界面渲染成 README 用的产品图，写到 `docs/`。
///
/// 两张图**各展示一半**：
/// - 深色 = **玻璃质感**（能看出折射、色散、边缘高光）；
/// - 浅色 = **界面本体**（质感关闭）—— 实测"浅色 + 玻璃 + 亮背景"会把深色文字
///   冲成灰色（`tint_k` 只有 0.36，64% 的背景透上来），当产品图很难看，
///   所以浅色这张刻意不带背景。
///
/// ⚠️ **默认 `#[ignore]`**：它不是回归基线，是"生成素材"的工具。
/// 重生成：
///
/// ```bash
/// rm docs/gui-*.png
/// cargo test -p hipercalc-gui --test snapshot -- --ignored readme_shots
/// # ⚠️ `Snapshot::matches_image` 会**自动加 `-wgpu` 后缀**，产物是
/// # `docs/gui-dark-wgpu.png`；把它缩一半、量化 256 色后改名成 `gui-dark.png`
/// ```
///
/// （`matches_image` 在**文件不存在时**才写盘，所以必须先删。）
#[test]
#[ignore = "生成 README 素材，不是回归基线"]
fn readme_shots() {
    let _guard = lock();
    let (w, h) = (480u32, 360u32);
    for (dark, path) in [
        (true, "../../docs/gui-dark.png"),
        (false, "../../docs/gui-light.png"),
    ] {
        let mut state = populated_state();
        state.set_dark_for_test(dark);
        if dark {
            // ✅ 顺序不能反：先建 State 再切玻璃
            let _ = state.update(Message::SetTexture(hipercalc_gui::theme::Texture::Glass));
            // 产品图想要"看得出是玻璃、但斜线还认得出"的模糊量：
            // 0.32 会把 45° 粗条糊成色块（试过），0.12 还能看出线的走向
            let _ = state.update(Message::SetGlassBlur(0.12));
            let _ = state.update(Message::BgCaptured(Some(
                hipercalc_gui::backdrop::Backdrop {
                    x: 0,
                    y: 0,
                    width: w,
                    height: h,
                    rw: VIEWPORT.0 as u32,
                    rh: VIEWPORT.1 as u32,
                    rgba: stripes_like(w, h),
                },
            )));
        }
        let snapshot = snap(&state);
        // 文件已存在时这里返回 false（不覆盖）—— 重生成前记得先删
        let _ = snapshot.matches_image(path).expect("写 README 素材失败");
    }
    let _ = Message::SetGlassBlur(0.0);
}
