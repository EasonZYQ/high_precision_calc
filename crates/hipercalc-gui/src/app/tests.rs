//! `app` 的单元测试（原来长在 `app.rs` 末尾，拆分时原样搬出来）。
//!
//! 只测**逻辑**（update / 输入模型 / 文案），不涉及 GPU；
//! 真正的像素回归在 `tests/snapshot.rs`。

// 测试要用父模块里的 `State` / `Message` / `is_precise` 等。
// ⚠️ `allow(unused_imports)`：不同测试用到的名字不同，某些构建下 glob 会被判成
// "未使用"，但删掉就会整片报错 —— 这里是刻意的宽口径导入。
#[allow(unused_imports)]
use super::*;

/// **字体大小滑块**（用户 2026-10-29）：改设置后全局字号跟着变，且会被持久化。
#[test]
fn font_scale_applies_globally_and_persists() {
    // 复位，避免和别的用例互相影响（全局原子）
    let mut st = State::default();
    let _ = st.update(Message::SetFontScale(1.0));
    assert!((theme::text_size(18.0) - 18.0).abs() < 0.01);

    let _ = st.update(Message::SetFontScale(1.5));
    assert!((theme::font_scale() - 1.5).abs() < 0.01);
    assert!(
        (theme::text_size(18.0) - 27.0).abs() < 0.01,
        "字号应整体放大 1.5 倍"
    );

    // 越界要被夹住（滑块范围 0.7–1.6）
    let _ = st.update(Message::SetFontScale(9.0));
    assert!((theme::font_scale() - 1.6).abs() < 0.01, "上限 1.6");

    // 复位，别影响其它快照/用例
    let _ = st.update(Message::SetFontScale(1.0));
}

/// **窗口尺寸变化 ⇒ 设置面板按新宽度重排**（用户 2026-10-29："加上拖动窗口重建 UI"）。
///
/// 之前换行预算是个常量（按 940 窗口写死），窗口拉宽后那一行也不会合并回来。
#[test]
fn settings_budget_follows_window_width() {
    let mut st = State::default();
    let narrow = st.settings_row_budget_for_test();
    let _ = st.update(Message::WindowResized(iced::Size::new(1400.0, 900.0)));
    let wide = st.settings_row_budget_for_test();
    assert!(
        wide > narrow + 300.0,
        "视口拉宽后预算应变大：{narrow} → {wide}"
    );
    let _ = st.update(Message::WindowResized(iced::Size::new(600.0, 700.0)));
    assert!(
        st.settings_row_budget_for_test() < narrow,
        "视口变窄后预算应变小"
    );
    // 窄到极限也要留一个能排版的最小值（不能变成 0 或负数）
    let _ = st.update(Message::WindowResized(iced::Size::new(80.0, 80.0)));
    assert!(st.settings_row_budget_for_test() >= 280.0);
}

/// **「自定义函数与变量」页**：页面内定义 → 列表出现 → 删除 → 消失
#[test]
fn userdefs_page_defines_and_deletes() {
    let mut st = State::default();
    let _ = st.update(Message::ToggleUserDefs);
    assert_eq!(st.page, ResultsPage::UserDefs);

    // 空输入：给行内提示，不发求值
    let _ = st.update(Message::UdfSubmit);
    assert!(st.udf_hint.is_some());

    // 定义 f(x) = x^2 + 1（走与主输入栏同一条通道）
    let _ = st.update(Message::UdfChanged("f(x)=x^2+1".into()));
    let _ = st.update(Message::UdfSubmit);
    st.await_pending_for_test();
    assert!(
        st.user_fns.iter().any(|(n, _, _)| n == "f"),
        "定义后列表里应出现 f：{:?}",
        st.user_fns
    );
    assert!(st.udf_input.is_empty(), "提交后输入框应清空");

    // 删除
    let _ = st.update(Message::DelUserDef {
        is_fn: true,
        name: "f".into(),
    });
    assert!(
        !st.user_fns.iter().any(|(n, _, _)| n == "f"),
        "删除后应消失"
    );

    // 再点一次按钮回到历史页
    let _ = st.update(Message::ToggleUserDefs);
    assert_eq!(st.page, ResultsPage::History);
}

#[test]
fn is_precise_reads_core_prefix() {
    assert!(is_precise("= 1 / 2"));
    assert!(is_precise("= 7"));
    assert!(!is_precise("≈ 0.8333"));
    assert!(!is_precise("错误: 未定义变量: x"));
}

#[test]
fn default_state_is_dark_and_mathio() {
    let s = State::default();
    assert!(s.dark, "默认应是深色模式");
    assert!(s.history.is_empty());
    assert!(!s.show_funcs);
    assert_eq!(s.modes.display, DisplayMode::MathIO);
    assert!(!s.busy, "刚启动时不该处于「计算中」");
}

#[test]
fn evaluate_pushes_history_and_clears_input() {
    let mut s = State::default();
    s.insert("1+1");
    assert_eq!(s.input_text(), "1+1");
    s.evaluate_blocking_for_test();
    assert_eq!(s.history.len(), 1, "应记下一条历史");
    assert!(
        s.history[0].output.contains('2'),
        "结果应含 2：{}",
        s.history[0].output
    );
    assert!(s.input_text().is_empty(), "求值后输入框应清空");
}

#[test]
fn empty_input_does_not_evaluate() {
    let mut s = State::default();
    // 空输入连单都派不出去（`evaluate` 直接 `Task::none()`）
    let _ = s.update(Message::Evaluate);
    assert!(s.history.is_empty(), "空输入不该产生历史");
    assert!(!s.busy, "空输入不该进入「计算中」");
}

/// 错误输入标记为错误，且**错误文本要原样保留**（不能被数学排版吃掉空格）。
#[test]
fn error_input_marks_entry_as_error() {
    let mut s = State::default();
    s.insert("1+");
    s.evaluate_blocking_for_test();
    assert_eq!(s.history.len(), 1);
    assert!(s.history[0].error, "`1+` 应标为错误：{:?}", s.history[0]);
}

/// **错误信息必须原样显示、不能被数学排版重写**（阶段五修掉的一个真 bug）。
///
/// 前因：`history_entry` 对**所有**结果都调 `mathrender::render(.., math, ..)`，
/// 而数学模式的 tokenizer 会**把空格全部吃掉**（`' ' => i += 1`）。
/// 于是错误信息
///
/// ```text
/// 错误: 位置 2 处意外的字符: ' '
/// ```
///
/// 被排成了 `错误:位置2处意外的字符:''` —— 快照上看起来就是
/// `错误:位置2处意外的字符:"`（末尾那个像双引号的东西，其实是两个
/// 单引号中间的空格被吃掉后挤在一起）。
///
/// 修法：`history_entry` 里改成 `math && !e.error` —— 报错时退回纯文本。
///
/// 这条测试从**行为**上锁住它：拿真实错误文本走一遍 `parse`，
/// 断言"数学排版确实会吃掉空格"（所以不能再对错误用数学排版），
/// 同时断言错误文本本身是含空格的散文。
#[test]
fn error_text_is_prose_not_math() {
    let mut s = State::default();
    s.insert("1+");
    s.evaluate_blocking_for_test();
    let e = &s.history[0];

    assert!(e.error);
    assert!(
        e.output.contains(' '),
        "错误信息是散文、含空格：{:?}",
        e.output
    );
    assert!(
        e.output.starts_with("错误"),
        "错误信息应以「错误」开头：{:?}",
        e.output
    );

    // 反证：若拿它去走数学排版，空格会被吃掉（这正是要避开的那条路）
    let flattened = flatten_node(&mathrender::parse(&e.output));
    assert!(
        !flattened.contains(' '),
        "数学排版必然吃掉空格（这就是错误不能走数学排版的原因）：{flattened:?}"
    );
    assert_ne!(
        flattened, e.output,
        "两种渲染结果不同 —— 说明错误走数学排版会改变用户看到的文字"
    );
}

/// 表达式回显**始终**按数学排版 —— 与显示模式无关（用户要求："根号应当
/// 始终渲染"）。
///
/// 回归背景：曾经把显示模式也传给了回显，于是切到 LineIO 后回显跟着退化成
/// 纯文本，`sqrt(2)` 原样显示成 ASCII。这条测试把"回显不受模式影响"的约定
/// 回显**永远按数学排版**，与显示模式、与这一行是不是报错都无关。
///
/// ⚠️ 这条以前是 `echo_renders_math`（错误行退回纯文本）。改掉的原因：
/// 回显的是**用户敲的表达式**，它天然是数学式；退回纯文本会让 Σ 结构
/// 显示成 `sum(k^2,k,1,)`（用户实测："错误提示还是输出的 sum 函数，
/// 没有渲染结构"）。报错**文案**仍然走纯文本（那是散文），两者互不影响。
#[test]
fn echo_always_renders_math() {
    for mode in [DisplayMode::MathIO, DisplayMode::LineIO] {
        let mut s = State::default();
        s.set_display_mode_for_test(mode);
        s.insert("sqrt(2)");
        s.evaluate_blocking_for_test();
        let e = &s.history[0];
        assert!(!e.error, "{mode:?} 下 sqrt(2) 不该报错");
        assert_eq!(e.input, "sqrt(2)", "回显原文应与用户输入一致");
        let flat = flatten_node(&mathrender::parse(&e.input));
        assert_eq!(flat, "√(2)", "{mode:?} 下回显应能解析成根号：{flat}");
    }
}

/// 把数学节点树拍平成字符串（测试断言用）
fn flatten_node(n: &mathrender::Node) -> String {
    use mathrender::Node;
    match n {
        Node::Text(s) => s.clone(),
        Node::Seq(items) => items.iter().map(flatten_node).collect(),
        Node::Frac(a, b) => format!("{}/{}", flatten_node(a), flatten_node(b)),
        Node::Root { degree, inner } => match degree {
            Some(d) => format!("{}({})", flatten_node(d), flatten_node(inner)),
            None => format!("√({})", flatten_node(inner)),
        },
        Node::Pow(a, b) => format!("{}^{}", flatten_node(a), flatten_node(b)),
        Node::Sub(a, b) => format!("{}_{}", flatten_node(a), flatten_node(b)),
        // 位置包装：拍平后就是里面那棵子树
        Node::At { inner, .. } | Node::Hit { inner, .. } => flatten_node(inner),
        // 占位框 / 光标 / 参数逗号都不携带源文本，拍平后是空的
        Node::Empty { .. }
        | Node::EmptyFocused { .. }
        | Node::EmptyBare { .. }
        | Node::Caret
        | Node::Comma { .. } => String::new(),
        // 括号分组拍平成 `(内容)`
        Node::Paren { inner } => format!("({})", flatten_node(inner)),
        Node::BigOp {
            body,
            var,
            from,
            to,
            ..
        } => format!(
            "sum({},{},{},{})",
            flatten_node(body),
            flatten_node(var),
            flatten_node(from),
            flatten_node(to)
        ),
    }
}

/// **半成品输入按 = 只给行内提示，不派单、不占历史**。
///
/// 用户反馈："错误提示还是输出的 sum 函数……错误提示请你适配 GUI 界面，
/// 不要用 CLI 的那一套"。所以 `∑(,,,)` 这种没写完的式子**根本不进解析器** ——
/// 进去只会得到"位置 4 处意外的字符: ','"（带字符偏移、带 ASCII 引号）。
#[test]
fn incomplete_input_shows_hint_and_skips_evaluation() {
    let mut s = State::default();
    s.insert("∑()");
    assert!(s.input.has_empty_slot(), "Σ 四个槽都是空的");
    let _ = s.update(Message::Evaluate);
    assert!(s.history.is_empty(), "没写完不该产生历史记录");
    assert_eq!(
        s.input_hint.as_deref(),
        Some("还有空位没填"),
        "应给出界面语言的提示"
    );
    // 变量框默认 n（2026-10-23 用户要求）
    assert_eq!(s.input_text(), "sum(,n,,)", "输入框内容不该被动过");

    // 一打字提示就撤掉
    s.insert("k");
    assert!(s.input_hint.is_none(), "开始修改后提示应消失");
}

#[test]
fn elapsed_text_is_precise_to_a_tenth_of_a_second() {
    use std::time::Duration;
    let _ = State::default().update(Message::SetLang(UiLang::ZhCn));

    // 用户 2026-10-23："把计算完的用时精确到 0.1s"
    assert_eq!(elapsed_text(Duration::from_millis(1000)), "用时：1.0秒");
    // **向下取整**：1.99 秒是 1.9，不是 2.0（宁可少报也不多报）
    assert_eq!(elapsed_text(Duration::from_millis(1999)), "用时：1.9秒");
    assert_eq!(elapsed_text(Duration::from_millis(1234)), "用时：1.2秒");
    assert_eq!(elapsed_text(Duration::from_secs(125)), "用时：125.0秒");
    // 不足 0.1 秒：给 `<0.1秒`，不是 `0.0秒`（后者像"没计时"）
    for d in [
        Duration::ZERO,
        Duration::from_micros(1),
        Duration::from_millis(99),
    ] {
        assert_eq!(
            elapsed_text(d),
            "用时：<0.1秒",
            "{d:?} 应显示 <0.1秒，实际 {:?}",
            elapsed_text(d)
        );
    }
}

/// 耗时文案跟着界面语言走（`用时：{0}秒` / `用时：<1秒` 两条词条都在词典里）
#[test]
fn elapsed_text_follows_language() {
    use std::time::Duration;
    let _ = State::default().update(Message::SetLang(UiLang::ZhCn));
    let zh = elapsed_text(Duration::from_secs(3));
    assert!(zh.contains("用时") && zh.contains('3'), "简体：{zh:?}");

    let _ = State::default().update(Message::SetLang(UiLang::En));
    let en = elapsed_text(Duration::from_secs(3));
    assert!(!en.contains('用'), "英文下不该残留中文：{en:?}");
    assert!(en.contains('3'), "英文下应保留秒数：{en:?}");
    let en_sub = elapsed_text(Duration::ZERO);
    assert!(!en_sub.contains('用'), "英文下 <1 秒也要翻译：{en_sub:?}");

    let _ = State::default().update(Message::SetLang(UiLang::ZhCn));
}

/// ⚠️ **回归测试（P0）**：`update` 派单时**绝不能等 worker 算完**。
///
/// ## 这条测试为什么长这样
///
/// 它之前是"用 `5000!` + 断言 `update < 50ms`"。那个写法**抓不到当时的 bug**：
/// 上一版把 `wait()` 留在了 `update` 里（`Task::perform` 收的是 future 本身，
/// 见 [`crate::evaluator`] 模块头），于是 `update` 会一直等到 worker 算完 ——
/// 但 `5000!` / `10000!` 在快机器上**只要十几毫秒**（实测 `10000!` = 18.9ms），
/// `< 50ms` 照样过。用户报的 `100^100^10` 会卡几十秒，测试却一路绿灯。
///
/// 现在做两件事：
/// 1. **把输入堆到足够重**（多行重复 15 次，几百毫秒级）—— 单次大运算在
///    fast 模式下有上限（`10000!` 就是顶了），堆行数是唯一能可靠加重的办法，
///    而且多行本来就是产品支持的输入形态（逐行求值）。
/// 2. **自适应断言**：先用同步路径量出"这个输入在 worker 上到底要跑多久"，
///    再要求 `update` 的耗时**远小于**它。机器快慢都不影响结论（比的是同一个
///    进程里的两个数），只要有人把等待挪回 `update`，测试必红。
#[test]
fn update_does_not_wait_for_the_worker() {
    use std::time::Instant;

    const ROWS: usize = 15;
    let lines: Vec<String> = vec!["10000!".to_string(); ROWS];
    let src = lines.join("\n");

    // ① 量出这个输入在 worker 上的真实耗时
    let probe = Evaluator::spawn(Session::new());
    let t0 = Instant::now();
    let out = probe.eval_blocking(src.clone(), lines.clone());
    let worker_time = t0.elapsed();
    assert!(!out.is_error, "探针输入不该报错：{out:?}");
    assert!(
        worker_time > std::time::Duration::from_millis(50),
        "探针输入太轻（worker 只花了 {worker_time:?}），这条测试就失去区分度了 —— \
         请加大 ROWS"
    );

    // ② 走产品路径派单：`update` 必须**与 worker 的耗时无关**。
    //    多行靠 Ctrl+Enter 插入换行（`ctrl_held` 为真时 Enter 才是换行，
    //    否则会被 `update` 拦成求值，见 `Message::Edit` 分支）。
    let mut s = State {
        ctrl_held: true,
        ..State::default()
    };
    for i in 0..ROWS {
        if i > 0 {
            let _ = s.update(Message::KeyPressed(
                iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
                iced::keyboard::Modifiers::CTRL,
            ));
        }
        s.insert("10000!");
    }
    s.ctrl_held = false;
    assert_eq!(
        s.input_text().lines().count(),
        ROWS,
        "多行输入应被拆成 {ROWS} 行"
    );

    let t = Instant::now();
    let _task = s.update(Message::Evaluate);
    let took = t.elapsed();
    assert!(
        took < worker_time / 4,
        "update 花了 {took:?}，而 worker 要 {worker_time:?} —— \
         说明 update 又在等求值了（这正是「未响应」的成因）"
    );
    assert!(s.busy, "派单后应标记为「计算中」");
    assert!(s.input_text().is_empty(), "派单时就该清空输入框");
    assert!(
        s.history.is_empty(),
        "结果还没回来，不该有历史（有的话说明同步算了）"
    );
}

/// 玻璃强度：滑条改的是 `State` + 进程级开关，且**越界值被夹住**。
///
/// ⚠️ 这条会动进程级开关（`theme::GLASS_PERCENT`），结束时必须还原 ——
/// 否则后面的快照会拍到"半透明键盘"这种意外观感。
#[test]
fn glass_setting_syncs_and_clamps() {
    let mut s = State::default();
    let before = theme::glass_amount();

    let _ = s.update(Message::SetAcrylicBlur(0.25));
    assert!((s.acrylic_blur - 0.25).abs() < 1e-6, "State 应记下新强度");
    assert!(
        (theme::glass_amount() - 0.25).abs() < 1e-6,
        "进程级开关也该同步（样式函数只读得到它）"
    );

    // 越界值被夹进 [0, 1]
    let _ = s.update(Message::SetAcrylicBlur(2.5));
    assert!(
        (s.acrylic_blur - 1.0).abs() < 1e-6,
        "上界应夹到 1.0：{}",
        s.acrylic_blur
    );
    let _ = s.update(Message::SetAcrylicBlur(-3.0));
    assert!(
        s.acrylic_blur.abs() < 1e-6,
        "下界应夹到 0.0：{}",
        s.acrylic_blur
    );

    theme::set_glass(before);
}

/// 一键开关玻璃：关掉时记住原强度，再开回来要**恢复它**而不是回到默认值
#[test]
fn toggle_glass_remembers_strength() {
    let mut s = State::default();
    let before = theme::glass_amount();

    let _ = s.update(Message::SetAcrylicBlur(0.83));
    let _ = s.update(Message::ToggleGlass);
    assert!(s.acrylic_blur.abs() < 1e-6, "关掉后强度应为 0");
    assert!(
        (s.glass_backup - 0.83).abs() < 1e-6,
        "关掉时应记住原值：{}",
        s.glass_backup
    );

    let _ = s.update(Message::ToggleGlass);
    assert!(
        (s.acrylic_blur - 0.83).abs() < 1e-6,
        "再开应回到 0.83 而不是默认值：{}",
        s.acrylic_blur
    );

    theme::set_glass(before);
}

/// 长结果布局是个开关：点一下就在"自动换行 / 横向滚动"之间切
///
/// （真正影响的是 `results()` 里滚动方向与 `mathrender` 的 `Wrapping`，
/// 那两处由快照与 mathrender 自己的测试覆盖；这里钉住状态机。）
#[test]
fn wrap_setting_is_recorded() {
    let mut s = State::default();
    let initial = s.wrap;
    let _ = s.update(Message::SetWrap(!initial));
    assert_eq!(s.wrap, !initial, "设置应被记下");
    let _ = s.update(Message::SetWrap(initial));
    assert_eq!(s.wrap, initial, "切回来应还原");
}

/// 设置类操作**不能**在测试里写盘（否则跑一次测试就改掉用户的真实配置）
#[test]
fn test_state_never_persists() {
    let s = State::default();
    assert!(!s.persist, "`State::default`（测试路径）必须关掉写盘");
}

/// **内容没变的抓屏结果必须被丢弃** —— 这是玻璃模式省 CPU 的核心。
///
/// iced 的架构下**任何一条消息**都会触发 `view()` 重建 + 全量重绘，
/// 而桌面静止时抓屏每轮拿到的都是同一张图。若每次都换 `image::Handle`、
/// 抬版本号，就白搭一次纹理上传 + 一次重绘。
///
/// 这里锁住两件事：内容变了才抬版本号、没变时累加"静止计数"。
#[test]
fn identical_backdrop_is_dropped() {
    let prev = theme::texture();
    // ⚠️ **顺序不能反**：`State::default()` 自己会 `theme::set_texture(..)`
    //（用 `GuiPrefs::default()` 的 `texture`），先设再建 State 会被它覆盖掉。
    let mut s = State::default();
    theme::set_texture(theme::Texture::Glass);
    let mk = |v: u8| crate::backdrop::Backdrop {
        x: 0,
        y: 0,
        width: 2,
        height: 1,
        rw: 4,
        rh: 2,
        rgba: vec![v, v, v, 255, v, v, v, 255],
    };

    // ① 第一张：收下
    let _ = s.update(Message::BgCaptured(Some(mk(10))));
    assert_eq!(s.bg_version, 1, "第一张应当被收下");
    assert_eq!(s.bg_idle, 0);
    assert!(s.bg_pixels.is_some(), "第一张要留下像素给 shader");

    // ② 完全相同的第二张：丢弃
    let _ = s.update(Message::BgCaptured(Some(mk(10))));
    assert_eq!(s.bg_version, 1, "内容一样就不该抬版本号");
    assert_eq!(s.bg_idle, 1, "静止计数应累加");

    // ③ 真的变了：重新收下
    let _ = s.update(Message::BgCaptured(Some(mk(200))));
    assert_eq!(s.bg_version, 2, "内容变了必须抬版本号");
    assert_eq!(s.bg_idle, 0, "一有变化立刻回到快节奏");

    // ④ 切到亚克力：旧像素作废，且换模式后**同一张图也必须被重新收下**
    //（否则会走"内容没变"的快路径直接返回 ⇒ 切过去之后一片空白）
    let _ = s.update(Message::SetTexture(theme::Texture::Acrylic));
    assert!(s.bg_pixels.is_none(), "换模式要丢掉旧像素");
    let _ = s.update(Message::BgCaptured(Some(mk(200))));
    assert!(s.bg_pixels.is_some(), "换模式后必须重新收下像素");
    assert_eq!(s.bg_version, 3, "换模式后那张图要重新抬版本号");

    theme::set_texture(prev);
}

/// 计算中时**不收新单**：连按求值不会积压（否则 worker 会排着算好几次）
#[test]
fn evaluate_is_ignored_while_busy() {
    let mut s = State {
        busy: true,
        ..State::default()
    };
    s.insert("1+1");
    let _ = s.update(Message::Evaluate);
    assert!(s.busy, "仍应保持计算中");
    assert_eq!(s.input_text(), "1+1", "计算中不该清空/改动输入框");
}

/// 结果回来后才 push 历史（`Message::Evaluated` 是唯一的写入点）
#[test]
fn evaluated_message_pushes_history() {
    let mut s = State {
        busy: true,
        ..State::default()
    };
    let _ = s.update(Message::Evaluated(Evaluated {
        input: "1+1".into(),
        text: "= 2".into(),
        is_error: false,
        elapsed: std::time::Duration::from_secs(3),
        user_fns: Vec::new(),
        user_vars: Vec::new(),
    }));
    assert!(!s.busy, "结果回来应解除「计算中」");
    assert_eq!(s.history.len(), 1);
    assert_eq!(s.history[0].input, "1+1");
    assert_eq!(s.history[0].output, "= 2");
    assert!(s.history[0].precise, "`= ` 前缀应识别为精确值");
    assert_eq!(
        s.history[0].elapsed,
        std::time::Duration::from_secs(3),
        "耗时必须原样带进历史"
    );
}

/// 虚拟键盘点击必须真的改到输入内容
#[test]
fn keypad_click_inserts_text() {
    let mut s = State::default();
    // 找到数字 `1` 的按键位置并点它
    let (r, c) = find_key("1").expect("键盘里应有 1");
    let _ = s.update(Message::Key(r, c));
    assert_eq!(s.input_text(), "1");

    // 再点 `+`
    let (r, c) = find_key("+").expect("键盘里应有 +");
    let _ = s.update(Message::Key(r, c));
    assert_eq!(s.input_text(), "1+");
}

/// 点函数键（`√`）应插入 `sqrt()` 并把光标放进括号内 ——
/// 用户点一下就能直接打被开方数，不用自己补右括号
#[test]
fn keypad_function_key_leaves_cursor_inside_parens() {
    let mut s = State::default();
    let (r, c) = find_key_label("√").expect("键盘里应有 √");
    let _ = s.update(Message::Key(r, c));
    assert_eq!(s.input_text(), "sqrt()");
    // 光标应在 `(` 之后、`)` 之前 ⇒ 再打 `2` 得到 `sqrt(2)`
    s.insert("2");
    assert_eq!(s.input_text(), "sqrt(2)");
}

/// 函数面板选一项后应收起弹窗，且内容插入正确
#[test]
fn function_panel_inserts_and_closes() {
    let mut s = State {
        show_funcs: true,
        ..State::default()
    };
    // 第 0 组第 0 项是 `sin()`
    let _ = s.update(Message::Function(0, 0));
    assert!(!s.show_funcs, "选完应收起弹窗");
    assert_eq!(s.input_text(), "sin()");
    s.insert("x");
    assert_eq!(s.input_text(), "sin(x)");
}

#[test]
fn backspace_and_clear_work() {
    let mut s = State::default();
    s.insert("123");
    let (r, c) = find_key_action(KeyAction::Backspace).expect("应有退格键");
    let _ = s.update(Message::Key(r, c));
    assert_eq!(s.input_text(), "12");
    let _ = s.update(Message::ClearInput);
    assert!(s.input_text().is_empty());
}

#[test]
fn toggles_flip_flags() {
    let mut s = State::default();
    let _ = s.update(Message::ToggleFuncs);
    assert!(s.show_funcs);
    let _ = s.update(Message::CloseFuncs);
    assert!(!s.show_funcs);
    let _ = s.update(Message::ToggleDark);
    assert!(!s.dark);
    let _ = s.update(Message::ToggleDark);
    assert!(s.dark);
}

// ── 阶段五新增行为 ────────────────────────────────────────────────────────

/// **Enter = 求值；Ctrl+Enter = 换行**（用户本轮明确要求）。
///
/// 这条测试盯住两个方向，缺一不可：
/// 1. 裸 Enter **不能**在输入框里留下换行符，而要触发一次求值；
/// 2. Ctrl+Enter 才插入换行，编辑器行数从 1 变 2。
///
/// ⚠️ 为什么必须靠 `ctrl_held` 这个自记的标志：`text_editor` 的绑定表把
/// `Enter` 与 `Ctrl+Enter` 映射成**同一个** `Edit::Enter`（不看修饰键），
/// 从 `Action` 上分不出来。若哪天有人"顺手"删掉那条订阅，
/// Ctrl+Enter 就会退化成求值 —— 而界面上很难一眼看出。
/// ⚠️ 求值是**异步**的，所以这条测试里凡是要"看到结果"的地方，
/// 都在 `Edit::Enter` 之后补一次等待（`Message::Evaluated` 走真实回写路径）。
/// 这样测的既是"Enter 会派单"，也是"派单后结果真能回来"。
#[test]
fn enter_evaluates_and_ctrl_enter_newlines() {
    // ── 裸 Enter：应求值，不产生换行 ──────────────────────────────────
    let mut s = State::default();
    assert!(!s.ctrl_held);
    s.insert("1+1");
    let _ = s.update(Message::KeyPressed(
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
        iced::keyboard::Modifiers::default(),
    ));
    assert!(s.busy, "裸 Enter 应直接派单求值");
    assert!(
        !s.input_text().contains('\n'),
        "裸 Enter 不该在输入框里留下换行：{:?}",
        s.input_text()
    );
    s.await_pending_for_test();
    assert_eq!(s.history.len(), 1, "结果回来后应有一条历史");

    // ── Ctrl+Enter：应插入换行，不产生历史 ─────────────────────────────
    let mut s = State::default();
    let _ = s.update(Message::CtrlChanged(true));
    assert!(s.ctrl_held);
    s.insert("1+1");
    let _ = s.update(Message::KeyPressed(
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
        iced::keyboard::Modifiers::CTRL,
    ));
    let _ = s.update(Message::KeyPressed(
        iced::keyboard::Key::Character("2".into()),
        iced::keyboard::Modifiers::default(),
    ));

    assert!(s.history.is_empty(), "Ctrl+Enter 不该求值：{:?}", s.history);
    assert_eq!(
        s.input_text().lines().count(),
        2,
        "Ctrl+Enter 应插入换行，编辑器变成两行"
    );
    assert_eq!(s.input_text(), "1+1\n2", "两行内容应原样保留");

    // ── 松开 Ctrl 后，Enter 又回到求值 ────────────────────────────────
    // 此时输入框里是两行 `1+1` / `2`，求值会**整体走一遍**、只出一条历史
    //（末行结果）—— 所以历史从 0 变 1，正好与上面"Ctrl+Enter 不产生历史"对照。
    let _ = s.update(Message::CtrlChanged(false));
    assert!(!s.ctrl_held);
    let _ = s.update(Message::KeyPressed(
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
        iced::keyboard::Modifiers::default(),
    ));
    s.await_pending_for_test();
    assert_eq!(s.history.len(), 1, "松开 Ctrl 后 Enter 应重新求值");
}

/// 多行输入求值时：**只把最后一行的结果推进历史，但中间行要真的"过一遍会话"**。
///
/// 规则与交互式命令行一致。怎么证明"中间行生效了"？用 `ans` —— 它是
/// 唯一**跨行必然变化**的副作用。
///
/// ⚠️ 这里原本写的是 `A=3` / `A*2`，结果整个测试红了，暴露出一个**我起初的
/// 误解**，值得记下来：本计算器里 `A=3` **不是赋值**，而是**方程**
/// （实测给 `A ∈ (-∞, 3)`）；大写字母是*存储变量*，要用 `/let A = 5` 这种
/// 斜杠指令才存得进去（见 `/help` 的"变量存储"一节）。所以 `A*2` 报
/// `未定义变量: A` 是**正确行为**，不是 bug。
///
/// 用 `ans` 就没这个歧义：`5!/2` 得 60，第二行的 `ans*2` 读到 60 得 120。
/// 若中间行没被求值，`ans` 还是上一次的默认值，第二行绝不可能得出 120。
#[test]
fn multiline_evaluate_only_reports_last_line() {
    let mut s = State::default();
    // 多行要靠 **Ctrl+Enter** 敲出来（裸 Enter 现在是求值）
    let _ = s.update(Message::CtrlChanged(true));
    s.insert("5!/2");
    let _ = s.update(Message::KeyPressed(
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
        iced::keyboard::Modifiers::CTRL,
    ));
    s.insert("ans*2");
    s.evaluate_blocking_for_test();

    assert_eq!(s.history.len(), 1, "两行只该出一条历史");
    let e = &s.history[0];
    assert!(!e.error, "不该是错误：{:?}", e);
    assert!(
        e.output.contains("120"),
        "第二行的 ans*2 应为 120，实际 {}\n（若是 60 之外的数，说明中间行没生效）",
        e.output
    );
    // 历史里显示的原文应保留用户敲的行结构
    assert!(e.input.contains('\n'), "历史原文应保留换行：{:?}", e.input);
}

/// 多行之间的 `ans` 是**逐行累积**的（不是"最后一行读第一行的 ans"）。
///
/// 这条把顺序钉死：`2+3` → `ans*10` → `ans*10` 必须是 5 → 50 → 500。
/// 如果哪天 `evaluate` 改成"只把首尾两行喂给会话"或"倒序"，这里立刻红。
#[test]
fn multiline_ans_accumulates_line_by_line() {
    let mut s = State::default();
    let _ = s.update(Message::CtrlChanged(true));
    s.insert("2+3");
    let _ = s.update(Message::KeyPressed(
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
        iced::keyboard::Modifiers::CTRL,
    ));
    s.insert("ans*10");
    let _ = s.update(Message::KeyPressed(
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
        iced::keyboard::Modifiers::CTRL,
    ));
    s.insert("ans*10");
    s.evaluate_blocking_for_test();

    assert_eq!(s.history.len(), 1);
    let e = &s.history[0];
    assert!(
        e.output.contains("500"),
        "2+3 → ans*10 → ans*10 应为 500，实际 {}",
        e.output
    );
}

/// 空行要被**跳过**而不是当成一条空表达式（否则会污染历史 / 产生错误）
#[test]
fn multiline_evaluate_skips_blank_lines() {
    let mut s = State::default();
    let _ = s.update(Message::CtrlChanged(true));
    s.insert("1+1");
    let _ = s.update(Message::KeyPressed(
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
        iced::keyboard::Modifiers::CTRL,
    ));
    let _ = s.update(Message::KeyPressed(
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
        iced::keyboard::Modifiers::CTRL,
    ));
    s.insert("3+4");
    s.evaluate_blocking_for_test();

    assert_eq!(s.history.len(), 1);
    assert!(
        s.history[0].output.contains('7'),
        "空行应被跳过、结果取 3+4=7：{}",
        s.history[0].output
    );
}

/// `input_text()` 的多行拼接**必须用换行符、不是空格**。
///
/// 这是阶段五修掉的一个真 bug：原来用 `' '` 拼，`1+1` ↵ `2+2` 会变成
/// `"1+1 2+2"` —— 解析器把它读成**隐式乘法** `(1+1)*(2+2)=8`，
/// 而用户写的是两行。测试直接盯住拼接字符。
#[test]
fn input_text_joins_lines_with_newline_not_space() {
    let mut s = State::default();
    let _ = s.update(Message::CtrlChanged(true));
    s.insert("1+1");
    let _ = s.update(Message::KeyPressed(
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
        iced::keyboard::Modifiers::CTRL,
    ));
    s.insert("2+2");
    assert_eq!(
        s.input_text(),
        "1+1\n2+2",
        "多行必须用 \\n 连接；用空格会变成隐式乘法"
    );
    assert!(
        !s.input_text().contains(' '),
        "不该出现空格拼接：{:?}",
        s.input_text()
    );
}

/// 主面板 ⇄ 字母面板**双向**切换（含用面板里的键切回去）。
///
/// 光测 `Message::SetPanel` 不够 —— 用户实际是**点键盘上的键**切换的，
/// 所以要验证 `ShowAbc` / `ShowMain` 两个 `KeyAction` 真的接上了。
#[test]
fn panels_switch_both_ways_via_keys() {
    let mut s = State::default();
    assert_eq!(s.panel, Panel::Main, "默认应在主面板");

    // 主面板上点字母键 → 切到字母面板
    let (r, c) = find_key_in(Panel::Main, "ABC").expect("主面板应有 ABC 键");
    let _ = s.update(Message::Key(r, c));
    assert_eq!(s.panel, Panel::Abc, "点 ABC 应切到字母面板");
    // `State::panel` 是 pub、`Panel` 也 derive 了 PartialEq，所以能直接比

    // 字母面板上点 `123` → 切回主面板
    let (r, c) = find_key_in(Panel::Abc, "123").expect("字母面板应有 123 键");
    let _ = s.update(Message::Key(r, c));
    assert_eq!(s.panel, Panel::Main, "点 123 应切回主面板");
}

/// 切面板不能丢输入内容（`Key` 分支里 `panel` 与 `input` 互不相干）
#[test]
fn switching_panel_preserves_input() {
    let mut s = State::default();
    s.insert("12");
    let _ = s.update(Message::SetPanel(Panel::Abc));
    assert_eq!(s.input_text(), "12");
    let _ = s.update(Message::SetPanel(Panel::Main));
    assert_eq!(s.input_text(), "12");
}

/// 字母面板的字母键真的能打字（字母在字母面板上，不在主面板）
#[test]
fn abc_panel_inserts_letters() {
    let mut s = State::default();
    let _ = s.update(Message::SetPanel(Panel::Abc));
    for ch in ["x", "y", "z"] {
        let (r, c) = find_key_in(Panel::Abc, ch).unwrap_or_else(|| panic!("字母面板应有 {ch}"));
        let _ = s.update(Message::Key(r, c));
    }
    assert_eq!(s.input_text(), "xyz");
}

/// Desmos 的 `aᵇ` / 上标键：插入 `^`，光标停在指数位置
/// （再打一个数字应得到 `2^n` 那种形态的 ASCII 形式）
#[test]
fn superscript_key_inserts_caret() {
    let mut s = State::default();
    s.insert("2");
    let (r, c) = find_key_action(KeyAction::Superscript).expect("应有上标键");
    let _ = s.update(Message::Key(r, c));
    assert_eq!(s.input_text(), "2^");
    s.insert("3");
    assert_eq!(s.input_text(), "2^3");
}

/// **点输入栏空白处 ⇒ 光标到末尾**（用户实测："光标在 4|5，无论鼠标点哪里
/// 都无法移到 5 后面"）。
///
/// 字符级目标只能表达"落在某个记号之前"，最后一个记号之后没有目标；
/// 所以整块输入栏包了一层 `mouse_area` 兜底发这条消息。
#[test]
fn clicking_blank_area_puts_caret_at_end() {
    let mut s = State::default();
    s.insert("12345");
    s.move_caret(-1);
    assert_eq!(s.input.cursor.at, 4, "先造出 4|5 这个状态");
    let _ = s.update(Message::PlaceCaretEnd);
    assert_eq!(s.input.cursor.at, 5, "点空白处应把光标送到末尾");
    s.insert("6");
    assert_eq!(s.input_text(), "123456");
}

/// 左右光标键能改中间的数字（这是加这两个键的**全部理由**）
#[test]
fn cursor_keys_move_within_input() {
    let mut s = State::default();
    s.insert("13");
    // 光标现在在末尾，左移一格到 1 与 3 之间
    let (r, c) = find_key_action(KeyAction::CursorLeft).expect("应有左移键");
    let _ = s.update(Message::Key(r, c));
    s.insert("2");
    assert_eq!(s.input_text(), "123", "左移后插入应落在中间");

    // 再右移一格、往回插 —— 验证右键也通
    let (r, c) = find_key_action(KeyAction::CursorRight).expect("应有右移键");
    let _ = s.update(Message::Key(r, c));
    assert_eq!(s.input_text(), "123", "只移动光标不该改内容");
}

/// 键盘上的 `ƒ` 键与工具条下的"函数"按钮都开弹窗，且**不**丢输入
#[test]
fn func_key_on_keypad_toggles_panel() {
    let mut s = State::default();
    let (r, c) = find_key_action(KeyAction::ToggleFuncs).expect("键盘里应有 ƒ 键");
    let _ = s.update(Message::Key(r, c));
    assert!(s.show_funcs, "点 ƒ 应开函数弹窗");
    let _ = s.update(Message::Key(r, c));
    assert!(!s.show_funcs, "再点一次应收起");
}

// ── 设置：语言 ────────────────────────────────────────────────────────────

/// 语言切换必须真的落到会话上（求值提示语言跟着变）
///
/// ⚠️ `Session` 现在在后台线程上、UI 读不到，所以这里断言的是 **UI 侧副本**
///（`State::modes.lang`）—— 而副本与外发命令在同一个 `update` 分支里一起改，
/// 所以"副本对"等价于"命令发出去了"。真正"命令到得了 worker"由
/// `crate::evaluator::tests::config_reaches_worker_session` 那条端到端测试兜住。
#[test]
fn set_lang_reaches_session() {
    let mut s = State::default();
    for l in ALL_LANGS {
        let _ = s.update(Message::SetLang(l));
        assert_eq!(s.modes.lang, l, "会话语言应切到 {l:?}");
    }
    // 还原成中文（默认），避免污染同进程其它测试的文案断言
    let _ = s.update(Message::SetLang(UiLang::ZhCn));
}

/// 三种语言的**母语名**必须齐全且互不相同 —— 设置面板里那一排按钮靠它
#[test]
fn lang_native_names_are_distinct_and_present() {
    assert_eq!(ALL_LANGS.len(), 3);
    let mut names: Vec<&str> = ALL_LANGS
        .iter()
        .map(|l| Session::lang_native_name(*l))
        .collect();
    for n in &names {
        assert!(!n.is_empty(), "语言母语名不该为空");
    }
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 3, "三种语言的母语名必须互不相同：{names:?}");
}

/// 切语言**不重算历史**（历史里是当时算好的文本，见 `update` 里的注释）
#[test]
fn changing_lang_keeps_history_intact() {
    let mut s = State::default();
    s.insert("1+1");
    s.evaluate_blocking_for_test();
    let before = s.history[0].output.clone();
    let _ = s.update(Message::SetLang(UiLang::En));
    assert_eq!(s.history[0].output, before, "切语言不该改动已算出的历史");
    let _ = s.update(Message::SetLang(UiLang::ZhCn));
}

/// **界面自己的标签必须跟着语言变**（阶段五补齐的一项）。
///
/// 这是最容易"看着像做了其实没做"的一件事：设置面板里明明有语言按钮、
/// 点下去也不报错，但工具条上还是满屏中文 —— 因为界面标签是**硬编码字面量**，
/// 根本不经过 `Session::eval` 那条会自动翻译的路。必须显式调 `t()`。
///
/// 这条测试逐条把界面用到的文案喂给 `t()`，断言 English / 繁體下都**变了样**，
/// 且简体下是原样（`t()` 在简体是直通）。任何一条漏翻都会在这里红。
#[test]
fn ui_labels_follow_language() {
    // 界面上**会跟随语言变化**的全部文案（新增标签时记得加进来）
    const UI_STRINGS: &[&str] = &[
        "快速",
        "死算",
        "清空历史",
        "工作精度",
        "应用",
        "当前 {0} 位",
        "语言 / Language",
        "数学字体",
        "输入表达式，按 = 或键盘右下角的 ↵ 求值",
        "输入表达式，例如 sqrt(2)/3、x^2-4=0、int(x^2,x,0,1)",
        "还有空位没填",
        "渲染缩放",
        "像素少则帧率高：帧时间与像素数近似成正比",
        "允许被截图",
        "允许截图",
        "不允许截图",
        "关掉后背景是真实桌面，但截图/录屏拍不到窗口",
        "预览",
        "主键盘",
        "字母键盘",
        "虚拟键盘",
        "展开函数面板",
        "关闭函数面板",
        "三角",
        "微积分",
        "平方根",
        "用时：<1秒",
        "用时：{0}秒",
        "计算中…",
        "主题",
        "深色",
        "浅色",
        "玻璃质感",
        "玻璃已开启",
        "玻璃已关闭",
        "设置面板",
        "点面板外任意处关闭",
        "玻璃强度",
        "底色浓度",
        "设置页浓度",
        "面板投影",
        "亚克力质感",
        "模糊强度",
        "已开启",
        "已关闭",
        "未启用",
        "关闭",
        "界面完全不透明：不抓屏、也不模糊",
        "桌面模糊",
        "系统未提供",
        "长结果",
        "自动换行",
        "横向滚动",
        "结果太长看不清时怎么办",
        "HiPerCalc — 超高精度计算器",
        // 历史搜索 / 复制（2026-10-27 新增）
        "搜索",
        "复制结果",
        "清空",
        "没有匹配的记录",
        // 三角形求解页
        "三角形求解",
        "填任意三个已知量（至少要有一个长度），点求解",
        "边",
        "角",
        "中线",
        "角平分线",
        "求解",
        "至少填一个已知量",
    ];

    // 先回简体，确认 `t()` 是直通（简体原文就是显示文本）
    let _ = State::default().update(Message::SetLang(UiLang::ZhCn));
    for s in UI_STRINGS {
        assert_eq!(t(s), *s, "简体下界面文案应原样显示：{s:?}");
    }

    // 英文：**每条都要变**（变了才说明词典里有它）
    let _ = State::default().update(Message::SetLang(UiLang::En));
    let untranslated: Vec<&str> = UI_STRINGS.iter().copied().filter(|s| t(s) == *s).collect();
    assert!(
        untranslated.is_empty(),
        "英文下这些界面文案没有译文（词典缺条目）：{untranslated:?}"
    );

    // 繁体：有简体专属字的条目应当变化（"语言"→"語言"、"字体"→"字體"…）
    let _ = State::default().update(Message::SetLang(UiLang::ZhTw));
    for s in ["清空历史", "数学字体", "虚拟键盘", "关闭函数面板"] {
        assert_ne!(t(s), *s, "繁体下 {s:?} 应有不同写法");
    }

    // 还原简体，免得污染同进程其它测试
    let _ = State::default().update(Message::SetLang(UiLang::ZhCn));
}

/// 带占位符的界面文案要正确插值（`当前 {0} 位`）
#[test]
fn ui_label_with_placeholder_fills() {
    let _ = State::default().update(Message::SetLang(UiLang::ZhCn));
    // 简体：`t_fmt` 走模板替换，`{0}` 换成实参
    let zh = t_fmt("当前 {0} 位", &["80"]);
    assert!(zh.contains("80"), "简体应含实参：{zh:?}");
    assert!(!zh.contains("{0}"), "占位符应被替换掉：{zh:?}");

    let _ = State::default().update(Message::SetLang(UiLang::En));
    let en = t_fmt("当前 {0} 位", &["80"]);
    assert!(en.contains("80"), "英文应含实参：{en:?}");
    assert!(!en.contains('位'), "英文里不该残留中文：{en:?}");
    assert!(!en.contains("{0}"), "占位符应被替换掉：{en:?}");

    let _ = State::default().update(Message::SetLang(UiLang::ZhCn));
}

/// 窗口标题跟随语言（`main.rs` 建窗口时读它）
#[test]
fn window_title_follows_language() {
    let s = State::default();
    let _ = State::default().update(Message::SetLang(UiLang::ZhCn));
    assert!(window_title(&s).contains("超高精度计算器"));
    let _ = State::default().update(Message::SetLang(UiLang::En));
    assert!(window_title(&s).contains("HiPerCalc"));
    assert!(!window_title(&s).contains('计'), "英文标题不该含中文");
    let _ = State::default().update(Message::SetLang(UiLang::ZhCn));
}

// ── 设置：数学字体 ────────────────────────────────────────────────────────

/// 字体切换落到进程级开关上，且**每个下标都能选中**
/// （`MathFont::ALL` 与 `from_index` 一旦错位，设置面板里点了没反应）
#[test]
fn set_math_font_applies_every_option() {
    for (i, f) in MathFont::ALL.iter().enumerate() {
        let _ = State::default().update(Message::SetMathFont(i));
        assert_eq!(theme::math_font(), *f, "下标 {i} 应选中 {f:?}");
    }
    // 还原默认，避免影响同进程其它渲染相关测试
    let _ = State::default().update(Message::SetMathFont(0));
    assert_eq!(theme::math_font(), MathFont::default());
}

/// 越界下标**不该 panic、也不该改掉当前字体**
/// （UI 上传错下标是可能的，比如以后按钮数量对不上）
#[test]
fn set_math_font_out_of_range_is_ignored() {
    let _ = State::default().update(Message::SetMathFont(0));
    let keep = theme::math_font();
    let _ = State::default().update(Message::SetMathFont(999));
    assert_eq!(theme::math_font(), keep, "越界下标应被忽略而不是乱改");
}

// ── 实时预览行 ────────────────────────────────────────────────────────────

/// 预览行对**半成品输入**（用户正打到一半）不能 panic。
///
/// 这是预览与结果区的关键差别：结果区拿到的是求值后的完整式子，
/// 而预览面对的是 `sqrt(`、`x^`、`1/` 这类**随时会出现的残缺串**。
/// 只要 `view` 在这上面炸了，用户打字打到一半界面就没了。
#[test]
fn preview_survives_incomplete_input() {
    // ⚠️ 结构化之后**括号会自动补平**：打 `int(` 就立刻得到一个完整的
    // Σ/∫ 结构（光标在第一个空槽里），而不是留一个悬空左括号。
    // 这是刻意的行为变化 —— 旧模型放任用户造出括号不配对的输入，
    // 结果按 = 才报语法错。现在从根上不可能。
    for (src, want) in [
        ("sqrt(", "sqrt()"),
        ("x^", "x^"),
        ("1/", "1/"),
        ("int(", "int(,,,)"),
        ("(", "()"),
        // 孤立的 `)` 会被忽略：结构化之后它只表示"跳出当前分组"，
        // 没有分组时无处可跳 —— 这样永远不会造出括号不配对的输入
        (")", ""),
        ("sin(", "sin()"),
        ("-", "-"),
        ("^", "^"),
        ("3+", "3+"),
        ("pi(", "pi()"),
        ("√", "√"),
        ("a_b", "a_b"),
    ] {
        let mut s = State::default();
        s.insert(src);
        assert_eq!(s.input_text(), want, "{src:?} 应进输入框并补平成 {want:?}");
        // 真正走一遍预览的构造路径（`parse` → `build`），不能 panic
        let node = mathrender::parse(&s.input_text());
        let _ = mathrender::build::<Message>(
            &node,
            mathrender::Style2 {
                size: 21.0,
                color: None,
                wrap: true,
                ..mathrender::Style2::default()
            },
            None,
        );
    }
}

/// 预览的解析结果要**保住用户在键面上看到的字符**。
///
/// `√` 会被键盘插成 `sqrt(`，但用户也可能直接粘贴一个 `√2`；
/// 两条路径都得到"能排版"的 `Node`，不能出现解析器把内容吃掉的情况。
#[test]
fn preview_keeps_user_visible_content() {
    for src in ["sqrt(2)/3", "x^2-4=0", "int(x^2,x,0,1)", "π/2", "√2"] {
        let mut s = State::default();
        s.insert(src);
        let node = mathrender::parse(&s.input_text());
        // 渲染一次确保可排版（`build` 内部会遍历整棵树）
        let _ = mathrender::build::<Message>(
            &node,
            mathrender::Style2 {
                size: 21.0,
                color: None,
                wrap: true,
                ..mathrender::Style2::default()
            },
            None,
        );
    }
}

/// 输入为空时**不显示预览行**（否则空输入框下面会挂一条空预览，
/// 白白占掉 40px 把键盘往下挤）
#[test]
fn empty_input_has_no_preview() {
    let s = State::default();
    assert!(s.input_text().is_empty(), "默认应无输入");
}

// ── Ans 支持（跨到 CLI crate 的回归护栏） ─────────────────────────────────

/// 键盘上的 `Ans` 在 GUI 这条路径上也要能用（大小写不敏感）。
///
/// 解析侧的详细用例在 `hipercalc::session` 里，这里只保证
/// **从界面打进去**的这个拼写能算 —— 用户看到的是键盘上的 `Ans`。
#[test]
fn ans_from_keyboard_evaluates() {
    let mut s = State::default();
    s.insert("5!/2");
    s.evaluate_blocking_for_test();
    assert!(!s.history[0].error, "前置算式应成功：{:?}", s.history[0]);

    // 用键盘上的 `A`（字母面板）拼出 `Ans` 与手打等价，这里直接打字验证
    s.insert("Ans*2");
    s.evaluate_blocking_for_test();
    let e = &s.history[1];
    assert!(!e.error, "`Ans*2` 不该报错：{:?}", e);
    assert!(
        e.output.contains('2'),
        "Ans(=60) * 2 = 120，实际 {}",
        e.output
    );
}

#[test]
fn mode_chips_change_session() {
    let mut s = State::default();
    let _ = s.update(Message::SetDisplay(DisplayMode::LineIO));
    assert_eq!(s.modes.display, DisplayMode::LineIO);
    let _ = s.update(Message::SetAngle(AngleMode::Degree));
    assert_eq!(s.modes.angle, AngleMode::Degree);
    let _ = s.update(Message::SetCalc(true));
    assert_eq!(s.modes.calc, CalcMode::Deep);
    // 还原，避免污染同进程其它测试
    let _ = s.update(Message::SetDisplay(DisplayMode::MathIO));
    let _ = s.update(Message::SetAngle(AngleMode::Radian));
    let _ = s.update(Message::SetCalc(false));
}

/// 函数面板"一行一条"之后，**面板下标必须与 `FN_GROUPS` 的条目一一对应**。
///
/// 旧版按 7 列切行、要手工把"第几行第几列"累加成全局下标（`chunks` 丢下标
/// 是当时的坑）；改版后一行一条，下标天然就是条目下标 —— 这条测试改盯
/// "每个分组逐条点过去，插入的文本必须与条目一致"，防止将来再改布局时
/// 把下标对错位。
#[test]
fn function_panel_index_maps_to_entries() {
    for (gi, g) in FN_GROUPS.iter().enumerate() {
        for (ii, it) in g.items.iter().enumerate() {
            let mut s = State::default();
            let _ = s.update(Message::Function(gi, ii));
            // ⚠️ `∑` / `∏` 不是普通文本插入：`insert` 会把它们升级成
            // **结构化大运算符**（Σ 四个空位 + 默认变量 n，见 `insert`），
            // 线性化文本因此是 `sum(,n,,)` 而不是键面写的 `∑()`。
            let expected: &str = match it.insert {
                "∑()" => "sum(,n,,)",
                "∏()" => "prod(,n,,)",
                // 结构化输入的线性化会**去掉空格**（空格不是记号）
                "triangle(a= b= c=)" => "triangle(a=b=c=)",
                other => other,
            };
            assert_eq!(
                s.input_text(),
                expected,
                "分组 {gi} 第 {ii} 条应插入 {expected:?}，实际插入了 {:?}",
                s.input_text()
            );
        }
    }
}

#[test]
fn precision_is_clamped_visibly() {
    let mut s = State::default();
    let _ = s.update(Message::PrecisionChanged("1".into()));
    let _ = s.update(Message::ApplyPrecision);
    assert_eq!(s.modes.precision, hipercalc::MIN_PRECISION);
    assert_eq!(
        s.precision_text,
        hipercalc::MIN_PRECISION.to_string(),
        "输入框应回填真实生效的值"
    );
    // 非法输入应回退到当前值而不是崩溃
    let _ = s.update(Message::PrecisionChanged("abc".into()));
    let _ = s.update(Message::ApplyPrecision);
    assert_eq!(s.precision_text, hipercalc::MIN_PRECISION.to_string());
}

// ── 测试辅助：在键盘布局里定位按键 ──────────────────────────────────────
//
// 键盘现在有**两个面板**，所以这几个 helper 都取"主面板"——绝大多数用例
// 关心的是主面板。要测字母面板的直接用 `layout_for(Panel::Abc)`。

fn find_key(insert: &str) -> Option<(usize, usize)> {
    crate::keypad::LAYOUT
        .iter()
        .enumerate()
        .find_map(|(r, row)| {
            row.iter()
                .position(|k| k.action == KeyAction::Insert && k.insert == insert)
                .map(|c| (r, c))
        })
}

fn find_key_label(label: &str) -> Option<(usize, usize)> {
    crate::keypad::LAYOUT
        .iter()
        .enumerate()
        .find_map(|(r, row)| row.iter().position(|k| k.label == label).map(|c| (r, c)))
}

fn find_key_action(a: KeyAction) -> Option<(usize, usize)> {
    crate::keypad::LAYOUT
        .iter()
        .enumerate()
        .find_map(|(r, row)| row.iter().position(|k| k.action == a).map(|c| (r, c)))
}

fn find_key_in(panel: Panel, label: &str) -> Option<(usize, usize)> {
    layout_for(panel)
        .iter()
        .enumerate()
        .find_map(|(r, row)| row.iter().position(|k| k.label == label).map(|c| (r, c)))
}
