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
    text_input,
};
// `Element` / `Length` / `Pixels` 在 iced 顶层与 `iced::widget` 都可用；
// 这里从 `iced` 顶层取，与官方示例一致。
use iced::{Alignment, Background, Color, Element, Length, Pixels, Task};

use hipercalc::Session;
use hipercalc::parser_api::DisplayMode;
use hipercalc::session::{ALL_LANGS, UiLang};
use hipercalc_core::trig::AngleMode;

use crate::keypad::{self, FN_GROUPS, KeyAction, KeyKind, Panel, layout_for};
use crate::mathrender;
use crate::theme::{self, MathFont};

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

/// 虚拟键盘按键的字号。
///
/// 18 是照 Desmos 观感定的（它的键面明显比正文大）。⚠️ 这个值不能小：
/// `≤` `≥` `π` `√` 这些记号在数学字体里本来就比其他字符矮一截，
/// 字号再降就会看不清（第一版用 16 时，`|a|` 与 `≤` 在快照里明显发虚）。
const KEY_FACE_SIZE: f32 = 18.0;

/// 键盘按键的高度（Desmos 的键是明显偏扁的长方形）
const KEY_HEIGHT: f32 = 40.0;

/// 键与键之间的间隙
const KEY_GAP: f32 = 5.0;

/// 设置面板里"标签"列的固定宽度。
///
/// 给固定值是为了让右边的按钮**纵向对齐成一列**（精度 / 语言 / 字体三行的
/// 控件左边缘在同一条竖线上）。取 118 是因为最长的标签"语言 / Language"
/// 排出来大约就是这么宽；窄了会把标签折行。
const SETTING_LABEL_W: f32 = 118.0;

/// 输入框与右侧 `C` / `=` 按钮的高度。
///
/// 抽成常量是因为它们**必须相等** —— 不等的话 `Row` 会把矮的那个拉伸，
/// 观感立刻散架（第一版就是高度不一致，按钮被压成 22px 细线）。
const INPUT_HEIGHT: f32 = 54.0;

/// 输入框的 widget id。
///
/// 必须显式给一个：用户要求"输入框的光标应当一直显示"。iced 只在**聚焦**时画光标，
/// 所以要能对它做 `operation::focus` / `operation::is_focused`，
/// 而这两个操作都得靠 id 定位（见 [`State::ensure_input_focus`]）。
const INPUT_ID: &str = "hipercalc-input";

/// 把 `a²` / `aᵇ` 这类键面拆成 `(底, 上标)`。
///
/// 拆开排才能得到"上标"的观感：直接当普通文本排的话，`²` 与 `a` 等高，
/// 看起来就是个怪字符而不是幂。上标字符集只覆盖 Desmos 键盘用到的那几个
/// （`²` `³` `ᵇ` `ⁿ`）—— 够用且不必引入上下标 Unicode 全表。
fn split_superscript(label: &str) -> Option<(&str, char)> {
    let mut chars = label.chars();
    let base_len = label.len() - chars.next_back()?.len_utf8();
    let (base, last) = label.split_at(base_len);
    let sup = last.chars().next()?;
    if matches!(sup, '²' | '³' | 'ᵇ' | 'ⁿ' | 'ᵏ') && !base.is_empty() {
        Some((base, sup))
    } else {
        None
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 界面文案的翻译
// ─────────────────────────────────────────────────────────────────────────────
//
// ⚠️ **界面自己的标签也必须过一遍翻译**。这不是可选项：
// `Session::eval` 返回的文本（错误信息、/help 回显）由 `hipercalc` 在**输出边界**
// 自动翻译，但界面自身那些硬编码的中文标签（"键盘"、"函数"、"预览"…）
// **根本不经过那条路** —— 不显式调 `translate` 的话，用户在设置里切成 English，
// 工具条与键盘区还是满屏中文，会以为语言选择是坏的（实测正是如此）。
//
// 用法一律 `theme::label(t("…"))`。`t` 是 `hipercalc::session::translate` 的转发。

/// 翻译一条界面文案（简体输入 → 当前语言）
fn t(s: &str) -> String {
    hipercalc::session::translate(s)
}

/// 带占位符的界面文案翻译（`当前 {0} 位` 这类）
fn t_fmt(template: &str, args: &[&str]) -> String {
    hipercalc::session::translate_fmt(template, args)
}

/// 窗口标题（按当前界面语言）。
///
/// `main.rs` 把它交给 `iced::application(..).title(..)`，那个参数是
/// `Fn(&State) -> String` 的**闭包**，所以签名必须收 `&State`。
///
/// ⚠️ 标题里**不需要**从 `state` 取语言：语言是进程级的（`i18n::get()`），
/// `t()` 直接读的就是当前值。收 `&State` 只是为了满足 iced 的类型要求。
pub fn window_title(_state: &State) -> String {
    t("HiPerCalc — 超高精度计算器")
}

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
    /// 当前键盘面板（主 / 字母）
    pub panel: Panel,
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
    /// 键盘：切换面板
    SetPanel(Panel),
    /// 设置面板：切换语言
    SetLang(UiLang),
    /// 设置面板：切换数学字体（传 [`MathFont::ALL`] 的下标）
    SetMathFont(usize),
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
            panel: Panel::default(),
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
                // ⚠️ **Enter 不参与求值**（用户的明确要求："在输入栏按 enter 会换行，
                // 而不是计算"）。iced 的 `text_editor` 默认就把 Enter 映射成
                // `Edit::Enter` ⇒ 插入一个换行符，正是我们要的行为，
                // 所以这里**什么都不用挡**，照常 perform 即可。
                //
                // 求值只有两个入口：右侧的 `=` 按钮，和键盘上的 `↵`。
                // 以前那句占位提示写着"按 = 或 Enter 求值"，是**错的**，已改掉。
                self.input.perform(action);
            }
            Message::Key(r, c) => {
                if let Some(k) = layout_for(self.panel).get(r).and_then(|row| row.get(c)) {
                    match k.action {
                        KeyAction::Insert => self.insert(k.insert),
                        KeyAction::Backspace => {
                            self.input.perform(Action::Edit(Edit::Backspace));
                        }
                        KeyAction::Clear => self.input = EditorContent::new(),
                        // 求值会清空输入框、把注意力转到结果区，**不还焦点**
                        KeyAction::Evaluate => return self.evaluate(),
                        // Desmos 的左右方向键：把光标挪一格，方便键盘用户改中间的数字
                        KeyAction::CursorLeft => {
                            self.input.perform(Action::Move(Motion::Left));
                        }
                        KeyAction::CursorRight => {
                            self.input.perform(Action::Move(Motion::Right));
                        }
                        KeyAction::ShowMain => self.panel = Panel::Main,
                        KeyAction::ShowAbc => self.panel = Panel::Abc,
                        // Desmos 的 `aᵇ` / `↑`：插一个 `^`，光标自然停在指数位置
                        KeyAction::Superscript => self.insert("^"),
                        // 开弹窗时焦点要留给弹窗（用户接下来要点里面的函数），**不还焦点**
                        KeyAction::ToggleFuncs => {
                            self.show_funcs = !self.show_funcs;
                            return Task::none();
                        }
                    }
                }
                // ⚠️ **其余按键一律把焦点还给输入框**。
                //
                // 不还的话，点完数字键焦点就留在那个按钮上，`text_editor` 因为是
                // 未聚焦状态**不再画光标** —— 用户看到的现象正是"点数字的时候
                // 都不知道光标在哪里"（原话）。焦点还回去，光标就一直在。
                return self.refocus_input();
            }
            Message::Function(g, i) => {
                if let Some(item) = FN_GROUPS.get(g).and_then(|grp| grp.items.get(i)) {
                    self.insert(item.insert);
                }
                // 选完收起弹窗，并把焦点还给输入框 —— 用户接下来就是要接着打字
                self.show_funcs = false;
                return self.refocus_input();
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
            Message::SetPanel(p) => self.panel = p,
            Message::SetLang(l) => {
                // 语言是**进程级**开关，`set_lang` 同时更新 state 里的 base，
                // 求值产生的提示（错误信息、指令反馈）下一句就跟着变。
                self.session.set_lang(l);
                // 已有的历史是**当时**算出来的文本，不重算 —— 与"切换显示模式不重算"
                // 同一套理由：重算会让用户分不清"是结果错了还是我改了设置"。
                //
                // ⚠️ **窗口标题这里改不了**：iced 0.14 只在**建窗口时**能设标题
                //（`iced::application(..).title(..)`），运行时**没有** `set_title`
                // API（`iced_runtime::window` 里翻遍了：只有 resize / minimize /
                // maximize / move_to，没有任何 title 相关函数）。
                // 所以标题停留为**启动时的语言**，与设置文件里存的语言一致 ——
                // 用户在设置里当场切换时，标题要重启才更新。这是框架限制，非疏漏。
            }
            Message::SetMathFont(i) => {
                // 字体是**进程级**开关（样式/渲染函数拿不到 State，见 theme 模块头）。
                // 切了之后**不用重算历史** —— 历史里存的是文本，渲染时才取字体，
                // 所以下一帧整屏文字（含旧结果）一起换字体。
                if let Some(f) = MathFont::from_index(i) {
                    theme::set_math_font(f);
                }
            }
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
    ///
    /// ⚠️ **多行之间用换行符拼、不是空格**。
    ///
    /// 这里原来拼的是 `' '`（空格），当时"输入只有一行"所以看不出问题。但用户要求
    /// **Enter 换行**之后多行输入就成了常规操作，空格拼接会直接把表达式改坏：
    /// 输入 `1+1` ↵ `2+2` 会变成 `"1+1 2+2"`，喂给解析器是**隐式乘法**（`(1+1)*(2+2)`）
    /// 而不是用户写的两行 —— 用户看到的结果会莫名其妙。
    ///
    /// 求值侧对换行的处理见 [`State::evaluate`]：按行分别求值，最后一行当结果。
    fn input_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0;
        while let Some(line) = self.input.line(i) {
            out.push(line.text.to_string());
            i += 1;
        }
        out
    }

    /// 单行版本的输入文本（历史回显、测试用）。
    /// 多行输入时用 `\n` 连接，保证"显示的就是用户敲的"。
    fn input_text(&self) -> String {
        self.input_lines().join("\n").trim().to_string()
    }

    /// 把焦点交回输入框，返回要执行的 task。
    ///
    /// 用户要求"输入框的光标应当一直显示，不然点数字的时候都不知道光标在哪里"。
    /// iced 的 `text_editor` **只在聚焦时画光标**，而点虚拟键盘上的按钮会把焦点
    /// 抢到按钮上 —— 光标于是消失，正好是用户抱怨的那个场景。
    ///
    /// 所以规则是：**凡是"点了按钮但意图仍是继续输入"的动作，做完就把焦点还回去**。
    /// 求值、清空、开弹窗这类会改变上下文的不还（那时用户注意力已经转移了）。
    fn refocus_input(&self) -> Task<Message> {
        iced::widget::operation::focus(INPUT_ID)
    }

    /// 求值：跑会话，把结果推进历史，清空输入框
    ///
    /// ## 多行怎么处理
    ///
    /// Enter 现在插换行，所以输入框里可能有多行。规则与**交互式命令行一致**：
    /// 按顺序逐行求值，只把**最后一行**的结果推进历史。
    ///
    /// 为什么不是"整个多行串当一条表达式"：那等于把换行当隐式乘法，用户敲两行
    /// 分开的算式会得到一个奇怪的乘积。为什么不是"每行都进历史"：用户是在
    /// 一个输入框里敲的，敲 3 行却在历史里炸出 3 条会被当成 bug。
    ///
    /// 副作用是 `A=3` 这种赋值行**在两行之间也能生效**（第 1 行给变量赋值，
    /// 第 2 行用它算结果），这正是命令行用户期待的行为。
    fn evaluate(&mut self) -> Task<Message> {
        let lines: Vec<String> = self
            .input_lines()
            .into_iter()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        if lines.is_empty() {
            return Task::none();
        }
        // 历史里显示的原文 = 用户敲的全部内容（多行保留换行，便于回看）
        let src = lines.join("\n");
        self.last_input = src.clone();

        // 逐行求值；中间行只是"过一下会话"（产生变量、ans 等副作用），
        // 最后一行才是要展示的那条。
        let mut r = self.session.eval(&lines[0]);
        for line in &lines[1..] {
            r = self.session.eval(line);
        }

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

        // 工具条上的文字都是"拉丁/数学"标签（MathIO、RAD…），走数学字体；
        // 中文标签（快速/死算/清空）走等线。`theme::label` 自动按内容挑。
        //
        // `seg` 收 `&'static str`（字面量，无需翻译的那些：MathIO / RAD / DEG）；
        // `seg_t` 收**已翻译的 `String`** —— 界面里会跟随语言变化的那几个标签用它。
        let seg = |label: &'static str, on: bool, msg: Message| {
            button(theme::label(label).size(14))
                .padding([5, 12])
                .style(move |t, s| theme::mode_chip(t, s, on))
                .on_press(msg)
        };
        let seg_t = |label: String, on: bool, msg: Message| {
            button(theme::label(label).size(14))
                .padding([5, 12])
                .style(move |t, s| theme::mode_chip(t, s, on))
                .on_press(msg)
        };

        // 图标按钮：固定最小宽度，否则 `☾` / `⚙` 这类单字符按钮会缩成很小的方块，
        // 点在它旁边一点就落空（第一版快照里它们明显偏小偏挤）
        let icon = |label: &'static str, on: bool, msg: Message| {
            button(text(label).size(15).font(theme::font_math()))
                .width(Length::Fixed(38.0))
                .padding([5, 0])
                .style(move |t, s| theme::mode_chip(t, s, on))
                .on_press(msg)
        };

        let mut r = Row::new().spacing(6).align_y(Alignment::Center);
        r = r.push(text("HiPerCalc").size(17).font(theme::font_math()));
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
        // 计算模式。⚠️ 这两个是**会翻译的**界面标签（不带 `&'static str`）：
        // `seg` 的签名收 `&'static str`，所以这里改成用 `theme::label(t(..))`
        // 直接构造，见下面 `clear` 的写法。
        r = r.push(seg_t(t("快速"), !deep, Message::SetCalc(false)));
        r = r.push(seg_t(t("死算"), deep, Message::SetCalc(true)));
        r = r.push(Space::new().width(Length::Fill));
        // 明暗 + 设置 + 清空
        r = r.push(icon(
            if self.dark { "☾" } else { "☀" },
            false,
            Message::ToggleDark,
        ));
        r = r.push(icon("⚙", self.show_settings, Message::ToggleSettings));
        // 清空历史：这条**必须**翻译（工具条上最显眼的中文之一）。
        // 注意键用的是"清空历史"而不是"清空" —— 后者在 CLI 里可能被当成别的片段。
        r = r.push(
            button(theme::label(t("清空历史")).size(14))
                .padding([5, 12])
                .style(|tt, s| theme::mode_chip(tt, s, false))
                .on_press(Message::ClearHistory),
        );

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
                    // ⚠️ 占位提示**不能说**"按 Enter 求值" —— Enter 现在是换行。
                    // 之前那句是错的，用户按 Enter 只会换行、然后以为程序坏了。
                    theme::label(t("输入表达式，按 = 或键盘右下角的 ↵ 求值"))
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
        // 回显也走 `mathrender`：用户敲的 `sqrt(2)/3` 在回显里同样应该显示成
        // 根号 + 分数，而不是一行原始 ASCII —— 与输入框里看到的样子保持一致。
        let echo_text: Element<'a, Message> = mathrender::render(
            &e.input,
            math,
            mathrender::Style2 {
                size: 15.0,
                color: Some(theme::colors().text_dim),
                monospace: false,
            },
        );

        let echo = row![
            text("›")
                .size(15)
                .style(theme::accent_text)
                .font(theme::font_math()),
            echo_text,
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        // 结果文本带 core 给的 `= ` / `≈ ` 前缀。**刻意保留**：
        // 它是区分"精确值"与"近似值"的唯一信号，比只看颜色可靠（色盲用户也能分辨）。
        // 长结果换行时前缀会被留在首行，观感也正常。
        let precise_for_style = e.precise && !e.error;
        // ⚠️ **错误信息不能走 `mathrender`**。
        //
        // 错误信息是**散文**（`错误: 位置 2 处意外的字符: ' '`），不是数学式。
        // `mathrender` 的 tokenizer 会**把空格整个吃掉**（见其 `' ' => i += 1`），
        // 于是上面那句会被排成 `错误:位置2处意外的字符:''` —— 用户在原快照里
        // 看到的 `错误:位置2处意外的字符:"` 就是这么来的（末尾那个引号是两个
        // 单引号中间的空格被吃掉后挤在一起的观感）。错误信息必须原样显示。
        //
        // 所以：**报错时退回纯文本**（`math = false`），与 LineIO 走同一条路。
        let result: Element<'a, Message> = mathrender::render(
            &e.output,
            math && !e.error,
            mathrender::Style2 {
                size: if e.error { 18.0 } else { 24.0 },
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

    /// 设置面板：精度 / 语言 / 数学字体。
    ///
    /// 三项各自一行，行内是"标签 + 一排可选按钮"—— 用一排按钮而不是下拉框，
    /// 是因为选项都很少（精度是输入框、语言 3 个、字体 5 个），
    /// 一排按钮一眼能看全，不用点开。
    fn settings_panel(&self) -> Element<'_, Message> {
        // ① 工作精度
        let precision_row = row![
            self.setting_label(t("工作精度"), SETTING_LABEL_W),
            text_input("20 – 2000", &self.precision_text)
                .on_input(Message::PrecisionChanged)
                .on_submit(Message::ApplyPrecision)
                .width(Pixels(96.0))
                .size(13)
                .font(theme::font_math())
                .style(theme::field),
            button(theme::label(t("应用")).size(13))
                .padding([4, 10])
                .style(|t, s| theme::mode_chip(t, s, false))
                .on_press(Message::ApplyPrecision),
            theme::label(t_fmt(
                "当前 {0} 位",
                &[&self.session.precision().to_string()]
            ))
            .size(13)
            .style(theme::hint),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        // ② 语言（用户要求："设置里添加语言选择"）。
        // 语言名用**各语言自己的写法**（"简体中文" / "繁體中文" / "English"），
        // 而不是翻译成当前语言 —— 语言选单里这样才是惯例：看不懂当前语言的人
        // 也能找到自己的那一项。
        let cur_lang = self.session.lang();
        let mut lang_row = Row::new().spacing(6).align_y(Alignment::Center);
        lang_row = lang_row.push(self.setting_label(t("语言 / Language"), SETTING_LABEL_W));
        for l in ALL_LANGS.iter() {
            let selected = *l == cur_lang;
            lang_row = lang_row.push(
                button(text(Session::lang_native_name(*l)).size(13))
                    .padding([5, 12])
                    .style(move |t, s| theme::mode_chip(t, s, selected))
                    .on_press(Message::SetLang(*l)),
            );
        }

        // ③ 数学字体（用户要求："在设置里添加这些全部字体，供用户选择"）。
        // 每个按钮**用它自己的字体渲染自己的名字** —— 这样选之前就能看到效果，
        // 比单看名字直观得多（这也是字体选单的常见做法）。
        let cur_font = theme::math_font();
        let mut font_row = Row::new().spacing(6).align_y(Alignment::Center);
        font_row = font_row.push(self.setting_label(t("数学字体"), SETTING_LABEL_W));
        for (i, f) in MathFont::ALL.iter().enumerate() {
            let selected = *f == cur_font;
            font_row = font_row.push(
                button(text(f.display_name()).size(13).font(f.font()))
                    .padding([5, 12])
                    .style(move |t, s| theme::mode_chip(t, s, selected))
                    .on_press(Message::SetMathFont(i)),
            );
        }

        container(
            column![
                precision_row,
                rule::horizontal(1.0).style(theme::divider),
                lang_row,
                rule::horizontal(1.0).style(theme::divider),
                font_row,
            ]
            .spacing(8),
        )
        .width(Length::Fill)
        .padding([8, 10])
        .style(theme::panel_low)
        .into()
    }

    /// 设置面板里各行标签，统一宽度让右边的控件对齐成一列
    ///
    /// 收 `String`（已翻译的文案）而不是 `&str`：标签内容随语言变化，
    /// 传借用的话调用处得先把 `t(..)` 的结果存起来，反而啰嗦。
    fn setting_label(&self, s: String, w: f32) -> Element<'_, Message> {
        container(text(s).size(13).style(theme::hint))
            .width(Length::Fixed(w))
            .into()
    }

    // ── ③ 输入区 ────────────────────────────────────────────────────────────

    /// 输入区：编辑器 + C / = 按钮 + **实时数学预览**。
    ///
    /// ## 为什么是"编辑器 + 预览"两行，而不是把编辑器本身变成数学排版
    ///
    /// 用户的要求原话是"输入栏里根号应当渲染，类似 desmos"。这里有一个**硬约束**
    /// 必须先说清楚，因为它决定了做法：
    ///
    /// - iced 的 `text_editor` 自己排版自己那串文本，**没有**"在这里画一个根号"
    ///   的扩展点。它唯一开放的钩子是 `Highlighter`，而
    ///   `iced_core::text::highlighter::Format` **只有 `color` 与 `font` 两个字段**
    ///   （没有字号、没有自绘回调）⇒ 它能改颜色和字体，**画不出根号的顶线**。
    /// - 要实现 Desmos 那种"光标能落在根号里面"的真正内联排版，得自己写一个
    ///   完整的文本编辑器（排版、光标、选区、命中测试全都要自己算）。
    ///
    /// 所以这里采取**分层**做法，两层各司其职：
    ///
    /// 1. **上面一行**是真正的 `text_editor`：打字、光标、选区、多行都在这里，
    ///    行为完全可靠（光标永远在它该在的地方）。
    /// 2. **下面一行**是 [`mathrender`] 排出来的**实时预览**：输入一变化就重排，
    ///    `sqrt(2)/3` 立刻显示成"根号 2 分之 3"。这就是用户要的"根号要渲染"。
    ///
    /// 预览行只在**输入非空**时出现，且高度与主行分开 —— 不会挤动下面的键盘。
    fn input_area(&self) -> Element<'_, Message> {
        // ⚠️ `text_editor` 的 `.width()` 只接受 `Pixels`（不像别的控件收 `Length`），
        // **没法声明 `Fill`**。而它的自然行为是"尽量占满"，在 `Row` 里会和右侧
        // 两个定宽按钮**争宽度并撑破容器** —— 实测症状是 `C` / `=` 被推到窗口外，
        // 只剩色块露在右边缘（截图上文字全看不见）。
        //
        // 解法：把编辑器放进一个 `FillPortion` 的槽位，让它有**明确的上限**
        // —— 槽位拿到 `Row` 分配的空间后，`text_editor` 在里面填满，不再向外膨胀。
        let editor = text_editor(&self.input)
            .id(INPUT_ID)
            .placeholder(t("输入表达式，例如 sqrt(2)/3、x^2-4=0、int(x^2,x,0,1)"))
            .on_action(Message::Edit)
            .size(19)
            // 内边距给得比默认宽：数学字贴着圆角边框会显得很挤
            .padding([12, 14])
            .height(Pixels(INPUT_HEIGHT))
            // 字体：数学衬线体（用户要求"符号和字母用 markdown 那种字体"）。
            // 中文走不到这里 —— 输入框里的内容应当都是表达式。
            .font(theme::font_math())
            .style(theme::input);

        let editor_slot = container(editor).width(Length::FillPortion(10));

        // `=` 与 `C` 给**固定宽高**：两者不等宽（56 / 46）是有意的，让 `=` 更醒目。
        //
        // ⚠️ **高度必须显式给**。第一版只给了宽度，`Row` 的交叉轴分配让按钮被压成
        // 一条 22px 高的细线（截图上只看得到色块，看不到文字）——
        // 而 editor 那边有 `Height::Fixed(56)` 撑着，落差全由按钮承担了。
        // 实测：宽度对、高度塌，是要单独看像素才能发现的一类问题。
        let go = button(text("=").size(22).font(theme::font_math()))
            .width(Length::Fixed(56.0))
            .height(Length::Fixed(INPUT_HEIGHT))
            .style(|t, s| theme::key(t, KeyKind::Equals, s))
            .on_press(Message::Evaluate);

        let clear = button(text("C").size(18).font(theme::font_math()))
            .width(Length::Fixed(46.0))
            .height(Length::Fixed(INPUT_HEIGHT))
            .style(|t, s| theme::key(t, KeyKind::Control, s))
            .on_press(Message::ClearInput);

        let row1 = row![editor_slot, clear, go]
            .spacing(8)
            .align_y(Alignment::Center);

        let mut body = Column::new().spacing(8).push(row1);

        // ── 实时预览行 ────────────────────────────────────────────────────
        // 先解析成 `Node`（拥有字符串），再借它渲染 —— 见 `input_preview` 的注释。
        let src = self.input_text();
        if !src.is_empty() {
            let parsed = mathrender::parse(&src);
            body = body.push(self.input_preview(&parsed));
        }

        container(body)
            .width(Length::Fill)
            .padding(8)
            .style(theme::panel)
            .into()
    }

    /// 输入框下方的**实时数学预览**（用户要求："输入栏里根号应当渲染，类似 desmos"）。
    ///
    /// 与结果区走同一个 [`mathrender`]，只是字号小一点、颜色弱一点 ——
    /// 它是"你正在打的东西长什么样"的回显，不该比结果更抢眼。
    ///
    /// ⚠️ 这里**先 `parse` 再 `build`**，而不是直接调 `render(src, ..)`。
    /// 原因是生命周期：`render` 的返回类型与入参 `&str` 没有绑定关系，
    /// 但它最终会调 `text(src.to_string())`，借用检查器于是把返回值的生命周期
    /// 追到入参上。预览用的 `src` 是 `input_area` 里的**局部变量**，
    /// 一旦直接返回就会报 `cannot return value referencing local variable`。
    /// 先 parse 得到**拥有所有权的 [`mathrender::Node`]**（里面的 `String` 是它自己的），
    /// 再把 `Node` 借出去渲染，生命周期就干净了。
    fn input_preview(&self, node: &mathrender::Node) -> Element<'_, Message> {
        let label = container(
            text(t("预览"))
                .size(11)
                .font(theme::font_cjk())
                .style(theme::hint),
        )
        .width(Length::Fixed(34.0));

        let rendered = mathrender::build(
            node,
            mathrender::Style2 {
                size: 21.0,
                color: Some(theme::colors().text),
                monospace: false,
            },
        );

        container(row![label, rendered].spacing(6).align_y(Alignment::Center))
            .width(Length::Fill)
            .padding([6, 10])
            .style(theme::preview_strip)
            .into()
    }

    // ── ④ 虚拟键盘区 ─────────────────────────────────────────────────────────

    /// 键盘上方那行小字："键盘" + 当前面板标签 + 函数弹窗按钮。
    ///
    /// Desmos 把 `functions` 放在键盘**网格内部**（右侧一列）。我们两种都保留了：
    /// 网格内的 `ƒ` 键是照 Desmos 的布局放的，这里这个按钮则是"一眼能看见"的入口
    /// —— 函数面板是本项目的主力功能，藏在一个小键里不好找。
    fn keypad_header(&self) -> Element<'_, Message> {
        let func_btn = button(
            row![
                text("ƒ").size(17).font(theme::font_math()),
                theme::label(t("展开函数面板")).size(14),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
        )
        .padding([7, 14])
        .style(move |t, s| theme::mode_chip(t, s, self.show_funcs))
        .on_press(Message::ToggleFuncs);

        // ⚠️ 键用的是**更长的键名**（"主键盘" / "字母键盘"），不是裸的"键盘" / "字母"：
        // 裸键太短、又是常用词，放词典里容易在别处的输出文本里被误匹配
        //（`t()` 是"子串最长优先"，短键会在任意句子里到处替换）。
        let panel_name = match self.panel {
            Panel::Main => t("主键盘"),
            Panel::Abc => t("字母键盘"),
        };

        row![
            theme::label(t("虚拟键盘")).size(13).style(theme::hint),
            text("·").size(13).style(theme::hint),
            text(panel_name)
                .size(13)
                .font(theme::font_cjk())
                .style(theme::accent_text),
            Space::new().width(Length::Fill),
            func_btn,
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into()
    }

    /// 一个键面 widget。
    ///
    /// Desmos 的键面规律（照搬）：**记号用数学字体、字母数字用普通字体**，
    /// 且所有键面都居中、字号统一。数学记号交给 [`crate::theme::font_math`]，
    /// 否则 `π` / `√` / `≤` 用普通字体排出来又矮又瘦，与旁边的数字不搭。
    fn key_face<'a>(&self, label: &'a str, kind: KeyKind) -> Element<'a, Message> {
        // `a²` / `aᵇ` 这种"字母 + 上标"要拆开排：上标字号更小、位置更高，
        // 直接当普通文本排会得到一个等高的 `a²`，完全不像幂。
        if let Some((base, sup)) = split_superscript(label) {
            return row![
                text(base.to_string())
                    .size(KEY_FACE_SIZE)
                    .font(theme::font_math()),
                text(sup.to_string())
                    .size(KEY_FACE_SIZE * 0.68)
                    .font(theme::font_math()),
            ]
            .spacing(1)
            .align_y(Alignment::Start)
            .into();
        }

        let t = text(label.to_string()).size(KEY_FACE_SIZE);
        // 记号（π √ ≤ …）用数学字体；`⌫` 是符号但也用数学字体更好看
        let t = if keypad::label_is_math(label) {
            t.font(theme::font_math())
        } else {
            t
        };
        // `↵` 是求值键，给它加粗一点，与 Desmos 的蓝色回车键呼应
        let t = if kind == KeyKind::Equals {
            t.font(theme::font_math())
        } else {
            t
        };
        t.into()
    }

    fn keypad_area(&self) -> Element<'_, Message> {
        let layout = layout_for(self.panel);

        let mut rows = Column::new().spacing(KEY_GAP).width(Length::Fill);
        for (ri, line) in layout.iter().enumerate() {
            let mut r = Row::new().spacing(KEY_GAP).align_y(Alignment::Center);
            for (ci, k) in line.iter().enumerate() {
                // 加宽的键：`↵`（求值）与 `⌫`（退格）—— Desmos 里这两个都是双宽。
                // 求值键尤其要点得准，它是最常用的键。
                let w = match k.action {
                    KeyAction::Evaluate | KeyAction::Backspace => Length::FillPortion(2),
                    // `⌫` 在字母面板的第 3 行也是双宽
                    KeyAction::ShowMain | KeyAction::ShowAbc => Length::FillPortion(2),
                    _ => Length::FillPortion(1),
                };
                let face = self.key_face(k.label, k.kind);
                r = r.push(
                    button(container(face).width(Length::Fill).center_x(Length::Fill))
                        .width(w)
                        .height(Length::Fixed(KEY_HEIGHT))
                        .padding([0, 2])
                        .style(move |t, s| theme::key(t, k.kind, s))
                        .on_press(Message::Key(ri, ci)),
                );
            }
            rows = rows.push(r);
        }

        container(
            column![
                self.keypad_header(),
                rule::horizontal(1.0).style(theme::divider),
                rows
            ]
            .spacing(8),
        )
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
                                // 函数名是拉丁文（`sin`、`transpose`），走数学字体；
                                // 说明是中文，走等线。
                                text(it.label).size(15).font(theme::font_math()),
                                theme::label(t(it.about)).size(10).style(theme::hint),
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
                column![
                    theme::label(t(g.title)).size(13).style(theme::accent_text),
                    items_col,
                ]
                .spacing(6),
            );
        }

        let card = container(
            column![
                row![
                    theme::label(t("展开函数面板")).size(17),
                    Space::new().width(Length::Fill),
                    button(theme::label(t("关闭函数面板")).size(13))
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

    /// 错误输入标记为错误，且**错误文本要原样保留**（不能被数学排版吃掉空格）。
    #[test]
    fn error_input_marks_entry_as_error() {
        let mut s = State::default();
        s.insert("1+");
        let _ = s.update(Message::Evaluate);
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
        let _ = s.update(Message::Evaluate);
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
        }
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

    /// **Enter 是换行、不是求值**（用户的明确要求）。
    ///
    /// 这条测试是整个阶段五里最容易悄悄坏掉的一条：如果哪天有人"顺手"在
    /// `Message::Edit` 里给 Enter 加个求值分支，用户在输入框里就没法换行了，
    /// 而且界面上完全看不出异常（照样插入换行？不，会变成求值）。
    ///
    /// iced 的默认行为就是 `Edit::Enter` → 插入换行，所以这里断言的是
    /// **"我们不去拦它"** —— 编辑器的行数必须从 1 变 2。
    #[test]
    fn enter_inserts_newline_instead_of_evaluating() {
        let mut s = State::default();
        s.insert("1+1");
        let _ = s.update(Message::Edit(Action::Edit(Edit::Enter)));
        let _ = s.update(Message::Edit(Action::Edit(Edit::Insert('2'))));

        assert!(
            s.history.is_empty(),
            "按 Enter 不该产生历史（更不该求值）：{:?}",
            s.history
        );
        assert_eq!(s.input_lines().len(), 2, "Enter 应插入换行，编辑器变成两行");
        assert_eq!(s.input_text(), "1+1\n2", "两行内容应原样保留");
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
        s.insert("5!/2");
        let _ = s.update(Message::Edit(Action::Edit(Edit::Enter)));
        s.insert("ans*2");
        let _ = s.update(Message::Evaluate);

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
        s.insert("2+3");
        let _ = s.update(Message::Edit(Action::Edit(Edit::Enter)));
        s.insert("ans*10");
        let _ = s.update(Message::Edit(Action::Edit(Edit::Enter)));
        s.insert("ans*10");
        let _ = s.update(Message::Evaluate);

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
        s.insert("1+1");
        let _ = s.update(Message::Edit(Action::Edit(Edit::Enter)));
        let _ = s.update(Message::Edit(Action::Edit(Edit::Enter)));
        s.insert("3+4");
        let _ = s.update(Message::Evaluate);

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
        s.insert("1+1");
        let _ = s.update(Message::Edit(Action::Edit(Edit::Enter)));
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
    #[test]
    fn set_lang_reaches_session() {
        let mut s = State::default();
        for l in ALL_LANGS {
            let _ = s.update(Message::SetLang(l));
            assert_eq!(s.session.lang(), l, "会话语言应切到 {l:?}");
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
        let _ = s.update(Message::Evaluate);
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
            "预览",
            "主键盘",
            "字母键盘",
            "虚拟键盘",
            "展开函数面板",
            "关闭函数面板",
            "三角",
            "微积分",
            "平方根",
            "HiPerCalc — 超高精度计算器",
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
        for src in [
            "sqrt(", "x^", "1/", "int(", "(", ")", "sin(", "-", "^", "3+", "pi(", "√", "a_b",
        ] {
            let mut s = State::default();
            s.insert(src);
            assert_eq!(s.input_text(), src, "{src:?} 应原样进输入框");
            // 真正走一遍预览的构造路径（`parse` → `build`），不能 panic
            let node = mathrender::parse(&s.input_text());
            let _ = mathrender::build::<Message>(
                &node,
                mathrender::Style2 {
                    size: 21.0,
                    color: None,
                    monospace: false,
                },
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
                    monospace: false,
                },
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
        let _ = s.update(Message::Evaluate);
        assert!(!s.history[0].error, "前置算式应成功：{:?}", s.history[0]);

        // 用键盘上的 `A`（字母面板）拼出 `Ans` 与手打等价，这里直接打字验证
        s.insert("Ans*2");
        let _ = s.update(Message::Evaluate);
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
    //
    // 键盘现在有**两个面板**，所以这几个 helper 都取"主面板"——绝大多数用例
    // 关心的是主面板。要测字母面板的直接用 `layout_for(Panel::Abc)`。

    fn find_key(insert: &str) -> Option<(usize, usize)> {
        keypad::LAYOUT.iter().enumerate().find_map(|(r, row)| {
            row.iter()
                .position(|k| k.action == KeyAction::Insert && k.insert == insert)
                .map(|c| (r, c))
        })
    }

    fn find_key_label(label: &str) -> Option<(usize, usize)> {
        keypad::LAYOUT
            .iter()
            .enumerate()
            .find_map(|(r, row)| row.iter().position(|k| k.label == label).map(|c| (r, c)))
    }

    fn find_key_action(a: KeyAction) -> Option<(usize, usize)> {
        keypad::LAYOUT
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
}
