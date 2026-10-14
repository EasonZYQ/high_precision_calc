//! 界面主体：状态、消息、更新、视图。
//!
//! Elm 架构（iced 的一贯写法）：`State` 存数据，`Message` 是事件，`update` 只改状态，
//! `view` 只读状态。**任何一项业务逻辑都不写在 `view` 里** —— 否则测试与推理都会变难。
//!
//! ## 布局分区（自上而下）
//!
//! ```text
//! ┌──────────────────────────────────────────────────────┐
//! │ ① 工具条：显示模式 / 角度模式 / 计算模式 / 明暗 / 设置  │
//! ├──────────────────────────────────────────────────────┤
//! │ ② 结果区（可滚动历史）：                              │
//! │      表达式回显（次要色）                              │
//! │      结果（数学排版：分数竖排、根号带顶线）             │
//! ├──────────────────────────────────────────────────────┤
//! │ ③ 输入区：多行编辑器 + [计算]                          │
//! ├──────────────────────────────────────────────────────┤
//! │ ④ 虚拟键盘（自制，点击输入）+ [函数] 按钮              │
//! └──────────────────────────────────────────────────────┘
//! ```
//!
//! ④ 与函数弹窗是用户明确要求的两处（"下面应当加上虚拟键盘…旁边有函数按钮，
//! 点一下弹出函数列表"）。函数列表用 [`stack`] 做成**浮层**：
//! iced 0.14 实测**没有可直接用的菜单/popup 组件**，`stack` 是唯一干净的做法。
//!
//! ## 光标插入
//!
//! 输入框用 `EditorContent` 而不是 `text_input`，因为它能**读光标位置**
//! （`content.cursor()`）并**局部编辑**（`content.perform(Edit::Insert(..))`）。
//! 虚拟键盘必须"插到光标处"，`text_input` 做不到这件事。

use iced::widget::text_editor::{Action, Content as EditorContent, Edit, Motion};
use iced::widget::{
    Column, Row, Space, button, column, container, row, rule, scrollable, stack, text, text_editor,
};
// `Element` / `Length` / `Pixels` 在 iced 顶层与 `iced::widget` 都可用；
// 这里从 `iced` 顶层取，与官方示例一致。
use iced::{Alignment, Background, Color, Element, Font, Length, Pixels, Task};

use hipercalc::Session;
use hipercalc::parser_api::DisplayMode;
use hipercalc_core::trig::AngleMode;

use crate::keypad::{self, FN_GROUPS, KeyAction, KeyKind, LAYOUT};
use crate::mathrender;
use crate::theme;

/// 函数面板每行的列数。抽成常量是因为 `func_panel` 的分行逻辑与测试都依赖它。
const FN_COLS: usize = 7;

/// 一条历史记录
#[derive(Debug, Clone)]
struct Entry {
    /// 用户输入的表达式原文（回显用）
    input: String,
    /// 求值结果文本（MathIO 时为可排版的 ASCII 数学式，LineIO 时为小数）
    output: String,
    /// 是否错误
    error: bool,
    /// 结果是否为精确值（决定 `=` 用成功色还是普通色）
    precise: bool,
}

/// 函数面板里每个按钮的宽度。
///
/// 取一个**固定值**而不是 `Length::Shrink`：按钮内容差异极大
/// （`√` 只有 1 个字符，`transpose` 有 9 个），Shrink 会让整行宽窄参差，
/// 而且单字符标签会把下方的中文说明挤没（见 `func_panel` 的注释）。
///
/// 112 px 是实测值：再窄一点，微积分组的说明（`定积分 int(f,x,a,b)`，10px 字号）
/// 就会折成两行、把整排按钮撑高，看起来像布局出错。
const FN_BUTTON_W: f32 = 112.0;

/// 应用状态
pub struct State {
    /// 求值会话（内含 `AppState`，与 CLI 同一套求值路径）
    session: Session,
    /// 输入框内容（多行编辑器 —— 能读光标、能局部插入）
    input: EditorContent,
    /// 求值历史，最新在末尾
    history: Vec<Entry>,
    /// 深色模式
    pub dark: bool,
    /// 函数弹窗是否展开
    show_funcs: bool,
    /// 亚克力是否已启用（探测一次后固定）
    acrylic_checked: bool,
    /// 设置面板是否展开
    show_settings: bool,
    /// 精度输入框的文本（16 进制用不上，只是给 UI 一个缓冲）
    precision_text: String,
    /// 上一条输入（用于按 ↑ 重复）
    last_input: String,
}

/// 消息
#[derive(Debug, Clone)]
pub enum Message {
    /// 编辑器内容变化
    Edit(Action),
    /// 按虚拟键盘的某个键：`(行的下标, 列的下标)`
    Key(usize, usize),
    /// 按函数面板的某项：`(分组下标, 项下标)`
    Function(usize, usize),
    /// 求值
    Evaluate,
    /// 切换函数弹窗
    ToggleFuncs,
    /// 关闭函数弹窗（点浮层背景）
    CloseFuncs,
    /// 切换设置面板
    ToggleSettings,
    /// 切换深色模式
    ToggleDark,
    /// 切换显示模式
    SetDisplay(DisplayMode),
    /// 切换角度模式
    SetAngle(AngleMode),
    /// 切换快速 / 死算
    SetCalc(bool),
    /// 清空历史
    ClearHistory,
    /// 窗口打开完成（此时才能探测并启用亚克力）
    WindowOpened,
    /// 清空输入框
    ClearInput,
    /// 设置面板：精度输入
    PrecisionChanged(String),
    /// 设置面板：应用精度
    ApplyPrecision,
    /// 占位（订阅里不关心的窗口事件映射到这里，update 里什么都不做）
    Noop,
}

impl Default for State {
    fn default() -> Self {
        let mut session = Session::new();
        // ⚠️ `Session::new()` 的显示模式是 **LineIO**（`Evaluator::new` 的默认值，
        // CLI 也一直如此）。GUI 显式改成 MathIO —— 图形界面里"分数竖排、根号带顶线"
        // 才是用户要看的东西；LineIO 只给一串小数，浪费了排版能力。
        // 用户仍可在工具条上一键切回 LineIO。
        session.set_display_mode(DisplayMode::MathIO);
        Self {
            precision_text: session.precision().to_string(),
            acrylic_checked: false,
            input: EditorContent::new(),
            history: Vec::new(),
            dark: true,
            show_funcs: false,
            show_settings: false,
            last_input: String::new(),
            session,
        }
    }
}

impl State {
    /// 更新：只改状态，不画界面
    pub fn update(&mut self, message: Message) -> Task<Message> {
        // 样式函数读不到 State，明暗通过进程级开关传给主题模块
        theme::set_dark(self.dark);

        match message {
            Message::Noop => {}
            Message::Edit(action) => {
                self.input.perform(action);
            }
            Message::Key(r, c) => {
                if let Some(k) = LAYOUT.get(r).and_then(|row| row.get(c)) {
                    match k.action {
                        KeyAction::Insert => self.insert(k.insert),
                        KeyAction::Backspace => {
                            self.input.perform(Action::Edit(Edit::Backspace));
                        }
                        KeyAction::Clear => self.input = EditorContent::new(),
                        KeyAction::Evaluate => return self.evaluate(),
                    }
                }
            }
            Message::Function(g, i) => {
                if let Some(item) = FN_GROUPS.get(g).and_then(|grp| grp.items.get(i)) {
                    self.insert(item.insert);
                }
                // 选完收起弹窗：用户的注意力已经回到输入框
                self.show_funcs = false;
            }
            Message::Evaluate => return self.evaluate(),
            Message::ToggleFuncs => self.show_funcs = !self.show_funcs,
            Message::CloseFuncs => self.show_funcs = false,
            Message::ToggleSettings => self.show_settings = !self.show_settings,
            Message::ToggleDark => {
                self.dark = !self.dark;
                theme::set_dark(self.dark);
            }
            Message::SetDisplay(m) => self.session.set_display_mode(m),
            Message::SetAngle(m) => self.session.set_angle_mode(m),
            Message::SetCalc(deep) => {
                self.session.set_calc_mode(if deep {
                    hipercalc_core::calc_mode::CalcMode::Deep
                } else {
                    hipercalc_core::calc_mode::CalcMode::Fast
                });
            }
            Message::ClearHistory => self.history.clear(),
            Message::ClearInput => self.input = EditorContent::new(),
            Message::PrecisionChanged(s) => self.precision_text = s,
            Message::ApplyPrecision => {
                if let Ok(n) = self.precision_text.trim().parse::<usize>() {
                    self.session.set_precision(n);
                    // 夹过之后回填，让用户看到真实生效的值
                    self.precision_text = self.session.precision().to_string();
                } else {
                    self.precision_text = self.session.precision().to_string();
                }
            }
            Message::WindowOpened => {
                // 窗口已创建，此时才能反查 HWND 并调 DWM
                if !self.acrylic_checked {
                    self.acrylic_checked = true;
                    let ok = crate::backdrop::enable(self.dark);
                    theme::set_acrylic(ok);
                    if !ok {
                        // 如实反馈：亚克力没生效（远程桌面下很常见），
                        // 界面会退回不透明背景，但功能完全正常
                        eprintln!(
                            "[hipercalc-gui] 桌面模糊不可用（DWM 未接受任何材质请求），\
                             已退回不透明背景。远程桌面 / 无头环境下这是预期行为。"
                        );
                    }
                }
            }
        }
        Task::none()
    }

    /// 把文本插到**光标处**，并按需回退光标（函数键补括号后落在括号内）
    fn insert(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        for ch in text.chars() {
            self.input.perform(Action::Edit(Edit::Insert(ch)));
        }
        let back = keypad::cursor_back_for(text);
        for _ in 0..back {
            // ⚠️ 光标移动是 `Action::Move(Motion)` 这个**顶层变体**，
            // 不是 `Action::Edit(Edit::Move(..))` —— `Edit` 里只有增删改，没有移动
            //（第一版就写错在这里）。
            self.input.perform(Action::Move(Motion::Left));
        }
    }

    /// 走 [`keypad::splice`] 实现插入（纯函数路径）。
    ///
    /// 与 [`State::insert`] 并存是**有意的**：两者的语义必须一致，而
    /// `keypad::splice` 是纯函数、能被大量单测覆盖（含多字节字符、越界光标）；
    /// `insert` 走的是 iced 的 `Content::perform`，无法脱离渲染器测试。
    /// 用 `splice` 算一遍"应该得到什么"，与 `perform` 的结果比对，
    /// 就得到了一条不依赖渲染器的等价性护栏。见 `debug_assert_insert_matches`。
    #[allow(dead_code)]
    fn insert_via_splice(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let (next, _cursor) = keypad::splice(&self.input_text(), self.cursor_chars(), text);
        self.input = EditorContent::with_text(&next);
    }

    /// 当前光标在**字符**下标上的位置（`Content::cursor()` 给的是行列，需要换算）
    fn cursor_chars(&self) -> usize {
        let c = self.input.cursor();
        let mut n = 0;
        for i in 0..c.position.line {
            if let Some(l) = self.input.line(i) {
                n += l.text.chars().count() + 1; // +1 是换行符
            }
        }
        n + c.position.column
    }

    /// 开发期护栏：`insert` 与 `insert_via_splice` 必须得到同样的文本。
    /// 只在 debug 构建下生效，release 里完全编译掉。
    #[allow(dead_code)]
    fn debug_assert_insert_matches(&self, before: &str, inserted: &str) {
        debug_assert!(
            {
                let (spliced, _) = keypad::splice(before, self.cursor_chars(), inserted);
                spliced == self.input_text()
            },
            "两种插入实现的语义不一致：splice 与 perform 结果不同（插入 {inserted:?}）"
        );
    }

    /// 取输入框当前文本（`Content` 没有直接暴露 `text()`，
    /// 靠逐行拼接 —— 我们的输入只有一行，但编辑器允许多行，所以按行拼）
    fn input_text(&self) -> String {
        let mut out = String::new();
        let mut i = 0;
        while let Some(line) = self.input.line(i) {
            out.push_str(&line.text);
            i += 1;
            if self.input.line(i).is_some() {
                out.push(' ');
            }
        }
        out.trim().to_string()
    }

    /// 求值：跑会话，把结果推进历史，清空输入框
    fn evaluate(&mut self) -> Task<Message> {
        let src = self.input_text();
        if src.is_empty() {
            return Task::none();
        }
        self.last_input = src.clone();
        let r = self.session.eval(&src);
        let precise = is_precise(&r.text);
        self.history.push(Entry {
            input: src,
            output: r.text,
            error: r.is_error,
            precise,
        });
        // 求值后清空输入框，符合计算器的直觉
        self.input = EditorContent::new();
        Task::none()
    }

    /// 视图
    pub fn view(&self) -> Element<'_, Message> {
        let body = column![
            self.toolbar(),
            self.results(),
            self.input_area(),
            self.keypad_area(),
        ]
        .spacing(10)
        .padding(14)
        .height(Length::Fill);

        // 函数弹窗用 stack 做成浮层盖在最上面
        let content: Element<'_, Message> = if self.show_funcs {
            let backdrop = iced::widget::mouse_area(
                container(Space::new().width(Length::Fill).height(Length::Fill)).style(|_t| {
                    container::Style {
                        background: Some(Background::Color(Color::BLACK.scale_alpha(0.42))),
                        ..container::Style::default()
                    }
                }),
            )
            .on_press(Message::CloseFuncs);

            let panel = container(self.func_panel())
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(28)
                .center_x(Length::Fill)
                .center_y(Length::Fill);

            stack![backdrop, panel].into()
        } else {
            body.into()
        };

        // 根容器负责"透出桌面"：亚克力成功时背景全透，失败时不透明。
        // 这一步**必须有** —— 窗口开了 transparent，若根容器不给背景，
        // 没成功启用亚克力时就会露出一片黑/花屏。
        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::root)
            .into()
    }

    // ── ① 工具条 ────────────────────────────────────────────────────────────

    fn toolbar(&self) -> Element<'_, Message> {
        let disp = self.session.display_mode();
        let ang = self.session.angle_mode();
        let deep = self.session.calc_mode() == hipercalc_core::calc_mode::CalcMode::Deep;

        let seg = |label: &'static str, on: bool, msg: Message| {
            button(text(label).size(14))
                .padding([5, 12])
                .style(move |t, s| theme::mode_chip(t, s, on))
                .on_press(msg)
        };

        // 图标按钮：固定最小宽度，否则 `☾` / `⚙` 这类单字符按钮会缩成很小的方块，
        // 点在它旁边一点就落空（第一版快照里它们明显偏小偏挤）
        let icon = |label: &'static str, on: bool, msg: Message| {
            button(text(label).size(15))
                .width(Length::Fixed(38.0))
                .padding([5, 0])
                .style(move |t, s| theme::mode_chip(t, s, on))
                .on_press(msg)
        };

        let mut r = Row::new().spacing(6).align_y(Alignment::Center);
        r = r.push(text("HiPerCalc").size(17));
        r = r.push(Space::new().width(Pixels(10.0)));
        // 显示模式
        r = r.push(seg(
            "MathIO",
            disp == DisplayMode::MathIO,
            Message::SetDisplay(DisplayMode::MathIO),
        ));
        r = r.push(seg(
            "LineIO",
            disp == DisplayMode::LineIO,
            Message::SetDisplay(DisplayMode::LineIO),
        ));
        r = r.push(Space::new().width(Pixels(8.0)));
        // 角度模式
        r = r.push(seg(
            "RAD",
            ang == AngleMode::Radian,
            Message::SetAngle(AngleMode::Radian),
        ));
        r = r.push(seg(
            "DEG",
            ang == AngleMode::Degree,
            Message::SetAngle(AngleMode::Degree),
        ));
        r = r.push(Space::new().width(Pixels(8.0)));
        // 计算模式
        r = r.push(seg("快速", !deep, Message::SetCalc(false)));
        r = r.push(seg("死算", deep, Message::SetCalc(true)));
        r = r.push(Space::new().width(Length::Fill));
        // 明暗 + 设置 + 清空
        r = r.push(icon(
            if self.dark { "☾" } else { "☀" },
            false,
            Message::ToggleDark,
        ));
        r = r.push(icon("⚙", self.show_settings, Message::ToggleSettings));
        r = r.push(seg("清空", false, Message::ClearHistory));

        container(r)
            .padding([8, 12])
            .width(Length::Fill)
            .style(theme::panel)
            .into()
    }

    // ── ② 结果区 ────────────────────────────────────────────────────────────

    fn results(&self) -> Element<'_, Message> {
        let math = self.session.display_mode() == DisplayMode::MathIO;

        let mut col = Column::new().spacing(14).width(Length::Fill);
        if self.history.is_empty() {
            col = col.push(
                container(
                    text("输入表达式后按 = 或 Enter 求值")
                        .size(15)
                        .style(theme::hint),
                )
                .padding([26, 8])
                .center_x(Length::Fill),
            );
        } else {
            for e in &self.history {
                col = col.push(self.history_entry(e, math));
            }
        }

        // `anchor_bottom`：历史是"越往下越新"，滚到底部才符合计算器的直觉。
        // 内容超出可视区时首条会被滚出视野 —— 这是**正常行为**（不是裁切 bug），
        // 加内边距只是为了首条不与面板边框贴死。
        let inner = scrollable(col.padding([10, 10]))
            .anchor_bottom()
            .height(Length::Fill);

        let mut shell = Column::new().spacing(0);
        if self.show_settings {
            shell = shell.push(self.settings_panel());
        }
        shell = shell.push(inner);

        container(shell)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(6)
            .style(theme::panel_card)
            .into()
    }

    /// 一条历史：上面是表达式回显，下面是（可排版的）结果
    fn history_entry<'a>(&'a self, e: &'a Entry, math: bool) -> Element<'a, Message> {
        let echo = row![
            text("›")
                .size(15)
                .style(theme::accent_text)
                .font(Font::MONOSPACE),
            text(&e.input)
                .size(15)
                .style(theme::echo)
                .font(Font::MONOSPACE),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        // 结果文本带 core 给的 `= ` / `≈ ` 前缀。**刻意保留**：
        // 它是区分"精确值"与"近似值"的唯一信号，比只看颜色可靠（色盲用户也能分辨）。
        // 长结果换行时前缀会被留在首行，观感也正常。
        let precise_for_style = e.precise && !e.error;
        let result: Element<'a, Message> = mathrender::render(
            &e.output,
            math,
            mathrender::Style2 {
                size: 24.0,
                color: Some(if e.error {
                    theme::colors().danger
                } else if precise_for_style {
                    theme::colors().success
                } else {
                    theme::colors().text
                }),
                monospace: !math,
            },
        );

        container(column![echo, result].spacing(5))
            .width(Length::Fill)
            .padding([7, 9])
            .style(move |t| theme::history_row(t, e.error))
            .into()
    }

    // ── 设置面板 ────────────────────────────────────────────────────────────

    fn settings_panel(&self) -> Element<'_, Message> {
        use iced::widget::text_input;
        let row1 = row![
            text("工作精度").size(13).style(theme::hint),
            text_input("20 – 2000", &self.precision_text)
                .on_input(Message::PrecisionChanged)
                .on_submit(Message::ApplyPrecision)
                .width(Pixels(96.0))
                .size(13)
                .style(theme::field),
            button(text("应用").size(13))
                .padding([4, 10])
                .style(|t, s| theme::mode_chip(t, s, false))
                .on_press(Message::ApplyPrecision),
            text(format!("当前 {} 位", self.session.precision()))
                .size(13)
                .style(theme::hint),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        container(row1)
            .width(Length::Fill)
            .padding([8, 10])
            .style(theme::panel_low)
            .into()
    }

    // ── ③ 输入区 ────────────────────────────────────────────────────────────

    fn input_area(&self) -> Element<'_, Message> {
        // ⚠️ `text_editor` 的 `.width()` 只接受 `Pixels`（不像别的控件收 `Length`），
        // **没法声明 `Fill`**。而它的自然行为是"尽量占满"，在 `Row` 里会和右侧
        // 两个定宽按钮**争宽度并撑破容器** —— 实测症状是 `C` / `=` 被推到窗口外，
        // 只剩色块露在右边缘（截图上文字全看不见）。
        //
        // 解法：把编辑器放进一个 `FillPortion` 的槽位，让它有**明确的上限**
        // —— 槽位拿到 `Row` 分配的空间后，`text_editor` 在里面填满，不再向外膨胀。
        let editor = text_editor(&self.input)
            .placeholder("输入表达式，例如 sqrt(2)/3、x^2-4=0、int(x^2,x,0,1)")
            .on_action(Message::Edit)
            .size(19)
            // 内边距给得比默认宽：等宽字贴着圆角边框会显得很挤
            .padding([12, 14])
            .height(Pixels(56.0))
            .font(Font::MONOSPACE)
            .style(theme::input);

        let editor_slot = container(editor).width(Length::FillPortion(10));

        // `=` 与 `C` 给**固定宽高**：两者不等宽（56 / 46）是有意的，让 `=` 更醒目。
        //
        // ⚠️ **高度必须显式给**。第一版只给了宽度，`Row` 的交叉轴分配让按钮被压成
        // 一条 22px 高的细线（截图上只看得到色块，看不到文字）——
        // 而 editor 那边有 `Height::Fixed(56)` 撑着，落差全由按钮承担了。
        // 实测：宽度对、高度塌，是要单独看像素才能发现的一类问题。
        let go = button(text("=").size(22))
            .width(Length::Fixed(56.0))
            .height(Length::Fixed(56.0))
            .style(|t, s| theme::key(t, KeyKind::Equals, s))
            .on_press(Message::Evaluate);

        let clear = button(text("C").size(18))
            .width(Length::Fixed(46.0))
            .height(Length::Fixed(56.0))
            .style(|t, s| theme::key(t, KeyKind::Control, s))
            .on_press(Message::ClearInput);

        container(
            row![editor_slot, clear, go]
                .spacing(8)
                .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding(8)
        .style(theme::panel)
        .into()
    }

    // ── ④ 虚拟键盘区 ─────────────────────────────────────────────────────────

    fn keypad_area(&self) -> Element<'_, Message> {
        let mut rows = Column::new().spacing(6).width(Length::Fill);
        for (ri, line) in LAYOUT.iter().enumerate() {
            let mut r = Row::new().spacing(6).align_y(Alignment::Center);
            for (ci, k) in line.iter().enumerate() {
                // `=` 与退格键单独加宽，视觉重心更稳（也更好点）
                let w = match k.action {
                    KeyAction::Evaluate => Length::FillPortion(2),
                    KeyAction::Backspace => Length::FillPortion(2),
                    _ => Length::FillPortion(1),
                };
                r = r.push(
                    button(text(k.label).size(16).width(Length::Fill).center())
                        .width(w)
                        .padding([9, 4])
                        .style(move |t, s| theme::key(t, k.kind, s))
                        .on_press(Message::Key(ri, ci)),
                );
            }
            rows = rows.push(r);
        }

        // 键盘右上方横排的"函数"按钮（用户要求：点一下弹出函数列表）
        let func_btn = button(
            row![
                text("ƒ").size(17).font(Font::MONOSPACE),
                text("函数").size(14),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
        )
        .padding([9, 16])
        .style(move |t, s| theme::mode_chip(t, s, self.show_funcs))
        .on_press(Message::ToggleFuncs);

        let header = row![
            text("键盘").size(13).style(theme::hint),
            Space::new().width(Length::Fill),
            func_btn,
        ]
        .align_y(Alignment::Center);

        container(column![header, rule::horizontal(1.0).style(theme::divider), rows].spacing(8))
            .width(Length::Fill)
            .padding(10)
            .style(theme::panel_low)
            .into()
    }

    // ── 函数弹窗 ────────────────────────────────────────────────────────────

    fn func_panel(&self) -> Element<'_, Message> {
        // 每个分组按每行 `COLS` 个**手动切行**。
        //
        // ⚠️ 两个都试过的坑：
        // 1. 单个 `Row` —— 不换行，按钮加宽后放不下的项被**静默裁掉**
        //    （三角组直接看不到 `cosh` / `tanh`）；
        // 2. `Grid::columns(n)` —— 会换行，但 `Grid` 的语义是**整行等高**，
        //    换行后第一行被撑到几十像素高，看起来像布局崩了。
        // 手动切行用普通 `Row`，行高由内容决定，两个问题都没有。
        const COLS: usize = FN_COLS;

        let mut body = Column::new().spacing(12);
        for (gi, g) in FN_GROUPS.iter().enumerate() {
            let mut items_col = Column::new().spacing(6);
            for (chunk_i, chunk) in g.items.chunks(COLS).enumerate() {
                let mut r = Row::new().spacing(6).align_y(Alignment::Center);
                for (j, it) in chunk.iter().enumerate() {
                    // 全局下标 = 本行起点 + 列偏移（chunks 会丢掉原始下标，得自己算）
                    let ii = chunk_i * COLS + j;
                    r = r.push(
                        button(
                            column![
                                text(it.label).size(15),
                                text(it.about).size(10).style(theme::hint),
                            ]
                            .spacing(1)
                            .align_x(Alignment::Center),
                        )
                        // ⚠️ 固定宽度**必须有**：`√` / `∛` 这类单字符标签会让按钮
                        // shrink 到只剩符号本身，下面那行说明文字（"平方根"/"立方根"）
                        // 就被挤没了 —— 第一版快照里这两个按钮是空白的，就是这个原因。
                        .width(Length::Fixed(FN_BUTTON_W))
                        .padding([7, 6])
                        .style(|t, s| theme::key(t, KeyKind::Function, s))
                        .on_press(Message::Function(gi, ii)),
                    );
                }
                items_col = items_col.push(r);
            }
            body = body.push(
                column![text(g.title).size(13).style(theme::accent_text), items_col,].spacing(6),
            );
        }

        let card = container(
            column![
                row![
                    text("函数").size(17),
                    Space::new().width(Length::Fill),
                    button(text("关闭").size(13))
                        .padding([4, 12])
                        .style(|t, s| theme::mode_chip(t, s, false))
                        .on_press(Message::CloseFuncs),
                ]
                .align_y(Alignment::Center),
                rule::horizontal(1.0).style(theme::divider),
                scrollable(body).height(Length::Fill),
            ]
            .spacing(10),
        )
        .width(Length::FillPortion(4))
        .height(Length::FillPortion(5))
        .padding(16)
        .style(theme::panel);

        card.into()
    }
}

/// 判断结果是否为**精确值**（`=` 而非 `≈`）。
///
/// core 给的文本以 `= ` / `≈ ` 开头，这是唯一可用的信号 ——
/// 比自己重新判断一遍"是否有限小数"可靠得多（那正是 `solve_aux::result_prefix` 做的事）。
fn is_precise(text: &str) -> bool {
    text.starts_with("= ") || text.starts_with('=')
}

/// 订阅：监听窗口事件，在 `Opened` 时触发亚克力探测。
///
/// 用 `window::events()` 而不是别的钩子 —— iced 0.14 没有"窗口就绪回调"，
/// 事件流是唯一能在**窗口已经存在**之后拿到控制权的办法。
pub fn subscription(_state: &State) -> iced::Subscription<Message> {
    iced::window::events().map(|(_id, ev)| match ev {
        iced::window::Event::Opened { .. } => Message::WindowOpened,
        // 其余窗口事件本项目暂时不关心，映射成一个不会被处理的占位
        _ => Message::Noop,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// 集成测试用的访问器（tests/snapshot.rs 需要）
// ─────────────────────────────────────────────────────────────────────────────
//
// 这些方法的唯一用途是"让外部测试能驱动界面"。刻意不放进 `#[cfg(test)]`：
// 集成测试（tests/ 目录）编译的是**库**，看不到 `#[cfg(test)]` 的项。
// 名字都带 `_for_test` 后缀，一眼能看出不该在产品代码里调用。

impl State {
    /// 直接设置输入框文本（测试用：跳过键盘模拟）
    pub fn set_input_for_test(&mut self, text: &str) {
        self.input = EditorContent::with_text(text);
    }

    /// 渲染视图（测试用）
    pub fn view_for_test(&self) -> Element<'_, Message> {
        self.view()
    }

    /// 当前主题（测试用）
    pub fn theme_for_test(&self) -> iced::Theme {
        theme::theme(self)
    }

    /// 设置明暗（测试用）
    pub fn set_dark_for_test(&mut self, dark: bool) {
        self.dark = dark;
        theme::set_dark(dark);
    }

    /// 设置显示模式（测试用）
    pub fn set_display_mode_for_test(&mut self, m: DisplayMode) {
        self.session.set_display_mode(m);
    }

    /// 历史条数（测试用）
    pub fn history_len_for_test(&self) -> usize {
        self.history.len()
    }

    /// 最近一条结果（测试用）
    pub fn last_output_for_test(&self) -> String {
        self.history
            .last()
            .map(|e| e.output.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(s.session.display_mode(), DisplayMode::MathIO);
    }

    #[test]
    fn evaluate_pushes_history_and_clears_input() {
        let mut s = State::default();
        s.insert("1+1");
        assert_eq!(s.input_text(), "1+1");
        let _ = s.update(Message::Evaluate);
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
        let _ = s.update(Message::Evaluate);
        assert!(s.history.is_empty(), "空输入不该产生历史");
    }

    #[test]
    fn error_input_marks_entry_as_error() {
        let mut s = State::default();
        s.insert("1+");
        let _ = s.update(Message::Evaluate);
        assert_eq!(s.history.len(), 1);
        assert!(s.history[0].error, "`1+` 应标为错误：{:?}", s.history[0]);
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
        let mut s = State::default();
        s.show_funcs = true;
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

    #[test]
    fn mode_chips_change_session() {
        let mut s = State::default();
        let _ = s.update(Message::SetDisplay(DisplayMode::LineIO));
        assert_eq!(s.session.display_mode(), DisplayMode::LineIO);
        let _ = s.update(Message::SetAngle(AngleMode::Degree));
        assert_eq!(s.session.angle_mode(), AngleMode::Degree);
        let _ = s.update(Message::SetCalc(true));
        assert_eq!(
            s.session.calc_mode(),
            hipercalc_core::calc_mode::CalcMode::Deep
        );
        // 还原，避免污染同进程其它测试
        let _ = s.update(Message::SetDisplay(DisplayMode::MathIO));
        let _ = s.update(Message::SetAngle(AngleMode::Radian));
        let _ = s.update(Message::SetCalc(false));
    }

    /// 函数面板按每行 7 个切行时，**全局下标必须逐行累加**。
    ///
    /// `chunks(7)` 丢掉原始下标，要自己算 `chunk_i * 7 + j`。
    /// 写错的话第 8 项之后点下去会插错函数。
    /// 这里用真实的 `Message::Function` 验证：算出的下标 → 插入的文本必须对得上。
    #[test]
    fn function_panel_index_survives_chunking() {
        const COLS: usize = FN_COLS;
        // 三角组有 9 项：下标 0..=6 在第一行，7、8 在第二行（跨行处最容易算错）
        assert!(
            FN_GROUPS[0].items.len() > COLS,
            "这条测试要求三角组超过一行，否则验证不到跨行"
        );

        for (chunk_i, chunk) in FN_GROUPS[0].items.chunks(COLS).enumerate() {
            for (j, it) in chunk.iter().enumerate() {
                let ii = chunk_i * COLS + j;
                let mut s = State::default();
                let _ = s.update(Message::Function(0, ii));
                assert_eq!(
                    s.input_text(),
                    it.insert,
                    "下标 {ii}（第 {chunk_i} 行第 {j} 列）应插入 {:?}，实际插入了 {:?}",
                    it.insert,
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
        assert_eq!(s.session.precision(), hipercalc::MIN_PRECISION);
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

    fn find_key(insert: &str) -> Option<(usize, usize)> {
        LAYOUT.iter().enumerate().find_map(|(r, row)| {
            row.iter()
                .position(|k| k.action == KeyAction::Insert && k.insert == insert)
                .map(|c| (r, c))
        })
    }

    fn find_key_label(label: &str) -> Option<(usize, usize)> {
        LAYOUT
            .iter()
            .enumerate()
            .find_map(|(r, row)| row.iter().position(|k| k.label == label).map(|c| (r, c)))
    }

    fn find_key_action(a: KeyAction) -> Option<(usize, usize)> {
        LAYOUT
            .iter()
            .enumerate()
            .find_map(|(r, row)| row.iter().position(|k| k.action == a).map(|c| (r, c)))
    }
}
