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

use std::sync::Arc;

use iced::Font;
use iced::widget::{
    Column, Row, Space, Stack, button, container, row, rule, scrollable, slider, stack, text,
    text_input,
};
// `Element` / `Length` / `Pixels` 在 iced 顶层与 `iced::widget` 都可用；
// 这里从 `iced` 顶层取，与官方示例一致。
use iced::{Alignment, Background, Color, Element, Length, Pixels, Task};

use hipercalc::Session;
use hipercalc::parser_api::DisplayMode;
use hipercalc::session::{ALL_LANGS, GuiPrefs, UiLang};
use hipercalc_core::calc_mode::CalcMode;
use hipercalc_core::trig::AngleMode;

use crate::evaluator::{Command, Evaluated, Evaluator};
use crate::expr;
use crate::keypad::{FN_GROUPS, KeyAction, KeyKind, Panel, layout_for};
use crate::liquid;

mod results;
mod settings;
mod triangle;
mod userdefs;
mod view;
use crate::mathrender;
use crate::theme::{self, MathFont};

/// **键面上那些"不是数学记号"的 UI 图标**：数学字体里没有它们的字形，
/// 硬套会渲染成豆腐块（`⌫` 实测如此）。这些键保持默认字体。
const UI_ICON_KEYS: &[&str] = &["⌫"];

/// **函数面板浮层**的尺寸与锚定（Desmos 式小面板，用户 2026-10-24 拍板）。
///
/// 固定宽度是"目录可扫读"的前提：一行一条、签名+说明+问号右对齐，
/// 宽度抖动的列表没法扫。高度同理 —— 比键盘略高一点，底部锚在 `ƒ` 键上方。
const FN_PANEL_W: f32 = 464.0;
const FN_PANEL_H: f32 = 380.0;
/// 面板一行放**几张卡片**（用户 2026-10-24："一行一个函数太浪费空间，改为一行三个"）。
const FN_COLS: usize = 3;
/// 浮层底部距窗口底边的距离 = **整个键盘区的高度**（含容器 padding 与分区标题）。
/// 键盘区 ≈ 标题行 26 + 分隔线 1 + 4 行键（4×40 + 3×5）+ 两级 spacing ≈ 235，
/// 留 250 让面板底边正好悬在"展开函数面板"按钮上方。改键盘行数时这里要跟着调。
const FN_PANEL_BOTTOM_PAD: f32 = 250.0;
/// 浮层右缘距窗口右边的距离（对齐 `ƒ` 按钮附近，照 Desmos 的锚定观感）。
const FN_PANEL_RIGHT_PAD: f32 = 26.0;

/// 一条历史记录
#[derive(Debug, Clone)]
struct Entry {
    /// 用户输入的表达式原文（回显用）
    input: String,
    /// 求值结果文本（MathIO 时为可排版的 ASCII 数学式，LineIO 时为小数）
    output: String,
    /// 是否错误
    error: bool,
    /// **只是提示，不是错误**（目前唯一的来源是「计算已中断」）。
    ///
    /// 用户 2026-10-24："计算已中断不是错误，只需要提示就行了" ——
    /// 它确实是用户自己按的取消，不该按错误染红、也不该像失败那样抢注意力，
    /// 所以单独一个标记走 `hint` 色。
    notice: bool,
    /// 结果是否为精确值（决定 `=` 用成功色还是普通色）
    precise: bool,
    /// 这一条算完花了多久（worker 侧的实测值，见 [`crate::evaluator`]）。
    ///
    /// 界面把它格式化成 `用时：N秒` 贴在结果行**最右侧**（用户要求）。
    /// 存 `Duration` 而不是格式化后的字符串：语言可能在之后被切换，
    /// 而"用时"这两个字是**界面文案**、要跟着语言走 ——
    /// 所以格式化推迟到 `history_entry` 里做。
    elapsed: std::time::Duration,
}

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

/// 把**当前模式的模糊强度**（0–1）换算成降采样倍数（1–16）。
///
/// ## 为什么用指数而不是线性
///
/// 降采样倍数每翻一倍，模糊程度才明显上一档；线性映射会让滑条后半段
/// "拧了没反应"。用 `2^(4a)` 正好把 0–1 均分成 5 档：
/// `a=0 → 1`、`0.25 → 2`、`0.5 → 4`、`0.75 → 8`、`1 → 16`。
///
/// ## 为什么这里是函数而不是常量
///
/// 用户要求"玻璃模式的模糊强度要跟亚克力**真正分开**"。之前写死 `BG_DOWNSCALE = 8`，
/// 两个模式的滑条调了完全没反应。现在读 [`theme::glass_amount`] ——
/// 它返回的正是**当前模式**的强度，所以亚克力和玻璃各调各的，互不影响。
fn blur_factor() -> u32 {
    let a = theme::glass_amount().clamp(0.0, 1.0);
    // **1 = 不模糊**（背景清晰），16 = 最强。
    //
    // ⚠️ 上一版我把它锁到最小 2，理由是"0 时 4px 模糊在 940px 宽的图上看不见，
    // 观感就是关掉了"。但用户明确纠正："模糊强度为 0 时**不是**不模糊，
    // 而是直接关闭所有模糊，这是 bug" —— 也就是说 **0 就该是"不模糊"**，
    // 用户想看的是清晰的桌面。现在 1 真的跳过 blur（见 `capture_blurred`）。
    // ⚠️ **恒定 1**：模糊已经挪到着色器里做（`theme::blur_px` + 浮点半径采样）。
    // 这里保留函数只是为了不改动调用点 —— 降采样会把"清晰度上限"锁死，
    // 而用户要求"模糊 0 时中间要非常清晰"，那只能是全分辨率。
    let _ = a;
    1
}

/// 历史区滚动容器的 Id（`snap_to` 用它定位）
const HISTORY_ID: &str = "hipercalc-history";

/// 输入框与右侧 `C` / `=` 按钮的高度。
///
/// 抽成常量是因为它们**必须相等** —— 不等的话 `Row` 会把矮的那个拉伸，
/// 观感立刻散架（第一版就是高度不一致，按钮被压成 22px 细线）。
const INPUT_HEIGHT: f32 = 72.0;

/// 输入框的 widget id。
///
/// 必须显式给一个：用户要求"输入框的光标应当一直显示"。iced 只在**聚焦**时画光标，
/// 所以要能对它做 `operation::focus` / `operation::is_focused`，
/// 而这两个操作都得靠 id 定位（见 [`State::ensure_input_focus`]）。
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
/// 窗口的**基准尺寸**（逻辑像素，`ui_scale = 1.0` 时）。
///
/// `main.rs` 建窗口用它；快照测试的 `VIEWPORT` 与它保持一致。
pub const WINDOW_W: f32 = 940.0;
pub const WINDOW_H: f32 = 900.0;

pub fn window_title(_state: &State) -> String {
    t("HiPerCalc — 超高精度计算器")
}

/// 拿到"最新那个窗口"的 id 再执行一个窗口命令。
///
/// iced 的窗口命令都要 `window::Id`，而 `State` 里刻意**不存** id
/// （多窗口以后会变，且 id 是运行时分配、测试里也不存在）。`latest()` 是
/// 异步查一次，代价可以忽略。
fn win_task(f: impl Fn(iced::window::Id) -> Task<Message> + Send + 'static) -> Task<Message> {
    // ⚠️ 这里用的是 `impl Task<Option<T>>` 上那个**特化**的 `and_then`：
    // 它只在真的拿到窗口时才继续，闭包直接收 `Id`（不是 `Option<Id>`）。
    iced::window::latest().and_then(f)
}

/// 抓屏数据的进程级快照：
/// `(像素, 抓屏区域原点x, y, 纹理像素宽, 高, 抓屏区域物理宽, 高)`，见 [`State::bg_pixels`]。
///
/// ⚠️ **两组宽高是两回事**（change_log81 的教训）：纹理像素宽高是降采样后的
/// （喂给 `write_texture`），抓屏区域宽高是窗口 + 外扩余量的**物理**尺寸
/// （喂给 [`crate::backdrop::uv_map`]）。上一版把前者当后者用，
/// scale/offset 按降采样倍数翻倍 ⇒ 背景整体错位且被压扁。
type BgPixels = (std::sync::Arc<Vec<u8>>, i32, i32, u32, u32, u32, u32);

/// 应用状态
/// 结果区当前显示哪一页。
///
/// 三选一（而不是三个布尔）：同时只会有一页可见，用枚举把"不可能的状态"表达掉。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ResultsPage {
    /// 历史列表（默认）
    History,
    /// 三角形求解页
    Triangle,
    /// 自定义函数与变量页
    UserDefs,
}

impl ResultsPage {
    /// 点一次按钮：已经是这页 ⇒ 回到历史；否则切到这页。
    fn toggled(self, target: ResultsPage) -> ResultsPage {
        if self == target {
            ResultsPage::History
        } else {
            target
        }
    }
}

pub struct State {
    /// 求值会话句柄。
    ///
    /// ⚠️ **它只是"通道"，不是求值器本身**：真正的 `Session` 在
    /// [`crate::evaluator`] 起的后台线程里，独占持有。这样 `update` 里**不可能**
    /// 再顺手跑一次同步求值把界面冻住（早期版本正是那样，`100^100^10` 会让
    /// Windows 给标题栏挂上「(未响应)」）。
    evaluator: Evaluator,
    /// worker 手里的会话是否正在算（决定按钮要不要禁用、是否显示「计算中」）
    busy: bool,
    /// 本次计算**开始的时刻**（用于"计算中 3.2s"的秒表）。
    ///
    /// 用 `Instant` 而不是自增计数：`view` 每帧读一次就能算出精确到 0.1 秒的耗时，
    /// 不需要一个每秒跳一次的消息在后台空转。
    busy_since: Option<std::time::Instant>,
    /// 在飞的那一次求值的**原始请求**（输入原文 + 已拆好的行）。
    ///
    /// 产品路径其实用不到它（结果经 `Task::perform` → [`Message::Evaluated`] 回来），
    /// 留着只是为了让测试能"补等一次"（见 [`State::await_pending_for_test`]）。
    ///
    /// ⚠️ 这里存的是**请求本身**，不是"结果信箱" —— 早期版本存的是
    /// `Arc<Mutex<Option<Evaluated>>>`，靠一条搬运线程填、靠自旋读，
    /// 而那段自旋被误放进了 `update`（详见 [`crate::evaluator`] 模块头的「走过的弯路」）。
    /// 现在结果只走 `oneshot` 一条路，测试要复现就重放一次请求 —— 简单且不可能卡住。
    pending: Option<(String, Vec<String>)>,
    /// 求值历史，最新在末尾
    history: Vec<Entry>,
    /// 历史搜索关键词（**子串匹配**，大小写不敏感；空串 = 不过滤）
    search: String,
    /// 结果区当前显示哪一页（历史 / 三角求解 / 自定义函数与变量）。
    ///
    /// ⚠️ 原来是 `triangle_open: bool`；再加一个布尔就会出现"两个 true 同时开"的
    /// 自相矛盾状态，所以一次性收敛成枚举。
    page: ResultsPage,
    /// 「自定义函数与变量」页：当前是**函数模式**（true）还是变量模式
    udf_is_fn: bool,
    /// 名称（函数名 `f` 或变量名 `A`）
    udf_name: String,
    /// 参数表（函数模式用，`x, y, z` 任意个）
    udf_params: String,
    /// 表达式（函数体）或变量的值
    udf_expr: String,
    /// 列表搜索（与计算栏历史搜索同一套：大小写不敏感的子串匹配）
    udf_search: String,
    /// 该页的行内提示（定义失败的报错就显示在这里）
    udf_hint: Option<String>,
    /// 自定义函数快照（每轮求值回包更新，见 `Evaluated::user_fns`）
    user_fns: Vec<(String, Vec<String>, String)>,
    /// 存储变量快照
    user_vars: Vec<(String, String)>,
    /// 三角形页的十二个输入（顺序见 [`triangle::TRI_KEYS`]：
    /// a b c / A B C / mA mB mC / tA tB tC）
    tri_vals: [String; 12],
    /// 三角形页的**求解结果**（就地显示，不跳回历史页）。
    ///
    /// 用户 2026-10-29："结果也在求解界面显示，不要跳回来"。
    /// 结果仍照常进历史（那份记录不变），这里多一份就地展示。
    tri_result: Option<String>,
    /// 界面文字缩放（跟随设置滑块；持久化在 `font_scale` 键里）
    font_scale: f32,
    /// 当前窗口的**逻辑尺寸**。
    ///
    /// 只服务于"需要按宽度决定布局"的地方 —— 目前是设置面板的换行预算与
    /// 最大高度（用户 2026-10-29："加上拖动窗口重建 UI"：窗口变宽时那一行要
    /// **合并回一行**，而不是永远按 940 的固定预算折行）。
    viewport: (f32, f32),
    /// 输入框内容（多行编辑器 —— 能读光标、能局部插入）
    /// 输入栏的**文本**。
    ///
    /// 输入框的内容 —— **结构化表达式树 + 槽位光标**（见 [`crate::expr`]）。
    ///
    /// 为什么不是 `String`：数学结构（分数、根号、Σ）在文本里是**折叠**的 ——
    /// `∑(,,,)` 的四个参数在源文本里都是"括号内同一处"，于是点哪个格子都一样、
    /// 光标还能落到括号那个"不属于任何节点"的位置上直接消失。
    /// 换成树之后每个插槽都是**天然合法的光标位置**，这两个 bug 从模型上就不存在了。
    ///
    /// 求值时用 [`expr::Doc::source`] 序列化回 MathIO 文本 —— 显示与计算解耦，
    /// 求值侧完全不知道输入框长什么样。
    input: expr::Doc,
    /// "式子没写完"之类的**行内提示**（显示在输入栏下面），`None` = 不显示。
    ///
    /// ⚠️ 不塞进历史：没写完不是一次"计算"，没必要占一条记录、也不该染红。
    /// 任何一次输入/删除都会把它清掉（见 [`State::insert`] / [`State::backspace`]）。
    input_hint: Option<String>,
    /// 深色模式
    pub dark: bool,
    /// 窗口当前是否最大化（决定标题栏第三个按钮画"最大化"还是"还原"）。
    ///
    /// 由 `window::is_maximized` 回报（[`State::sync_max_state`]），**不是**本地猜测：
    /// 双击标题栏、Win+↑、拖到屏幕顶部都会最大化，本地记的位必然漂。
    maximized: bool,
    /// 窗口的**当前几何**（逻辑像素）：`(x, y, w, h)`，只记非最大化时的值。
    ///
    /// 由 [`Message::WindowMoved`] / [`Message::WindowResized`] 更新，
    /// 退出时（[`crate::chrome::ChromeMessage::Close`]）写盘 —— 每次移动都写盘
    /// 会把磁盘打满（拖动窗口每秒几十个事件），所以**只在关闭时落盘**。
    win_geom: (f32, f32, f32, f32),
    /// 退出时是否最大化（与 [`State::maximized`] 分开记：后者是"现在"的状态，
    /// 这个是要写进设置的那一份；两者只在关闭那一刻同步）。
    win_max_pref: bool,
    /// 上次把窗口几何写盘的时刻（**节流**用）。
    ///
    /// 拖动/缩放窗口每秒几十个事件，逐个写盘是不能接受的；而只靠"关闭按钮"落盘
    /// 又不保险 —— Alt+F4、任务栏关闭、任务管理器结束都不会走自绘按钮。
    /// 折中：几何变化后**最多每 2 秒写一次**，代价可忽略、丢失窗口最多 2 秒。
    geom_saved_at: Option<std::time::Instant>,
    /// 是否有一次"延迟补写"已排在队里（保证最多只有一个）
    geom_save_pending: bool,
    /// 玻璃（liquid glass）强度：`0.0` = 关闭（不透明纯色），`1.0` = 最强。
    ///
    /// ⚠️ 与 [`theme::set_glass`] **双写**：样式函数的签名是 `Fn(&Theme) -> Style`、
    /// 拿不到 `State`，所以这类全局观感只能存进程级开关（与 `dark` 同一套路）。
    /// 这里留一份是为了让设置面板的滑条有值可读、也让单测能断言。
    /// 亚克力模式的**模糊强度**（原「玻璃强度」）。
    ///
    /// 只驱动背景抓屏的模糊程度 —— 面板透明度由 `acrylic_tint` 负责（用户规格拆开的）。
    acrylic_blur: f32,
    /// 关掉玻璃前记住的强度 —— 点"开启"时恢复到它，而不是回到写死的默认值
    glass_backup: f32,
    /// 玻璃**底色的浓度**：`0.0` = 完全透明，`1.0` = 完全不透明。
    ///
    /// 与 [`State::glass`] 的分工（用户要求两者都要）：强度拧的是"整体玻璃效果"，
    /// 这一个只拧"面板底色有多实"。同样与 [`theme::set_glass_tint`] 双写。
    /// 亚克力模式的**底色浓度**（面板有多不透明）
    acrylic_tint: f32,
    /// 当前**质感模式**（亚克力 / 玻璃 / 关闭，三选一互斥）
    texture: theme::Texture,
    /// 玻璃模式的模糊强度（与亚克力独立）
    glass_blur: f32,
    /// 玻璃模式的底色浓度（与亚克力独立）
    glass_tint2: f32,
    /// **设置面板自己**的浓度（亚克力模式下）：0 = 全透，1 = 与主面板同档。
    ///
    /// 与 [`theme::set_dialog_tint`] 双写（样式函数拿不到 State）。
    acrylic_dialog_tint: f32,
    /// **设置面板自己**的浓度（玻璃模式下）—— 两种质感各存一份，互不干扰。
    glass_dialog_tint: f32,
    /// 面板**投影**强度：`0.0` = 完全无阴影。与 [`theme::set_shadow`] 双写，
    /// 同一套理由（样式函数拿不到 `State`）。
    shadow: f32,
    /// 结果行的布局策略：`true` = 长内容自动折行，`false` = 不折行、横向滚动。
    ///
    /// 用户报过「结果太长，右边被挡住看不到后半段」。原因是 iced 默认的
    /// `Wrapping::Word` 只在**词边界**折行，而 100 位的整数中间没有词边界 ⇒
    /// 既不折行、容器又只开了垂直滚动 ⇒ 横向溢出被裁。详见 [`mathrender::Style2::wrap`]。
    wrap: bool,
    /// 历史区是否停在**底部**（新结果要不要自动滚下去，看它）。
    ///
    /// 用户明确要求："当历史已经拉到最新的计算，那么再计算新的就应该自动滑动到最下面，
    /// 如果原来不在最新的计算，就不需要滚动" —— 翻旧记录时不能被强行拽回底部。
    at_bottom: bool,
    /// 抓屏得到的**原始像素** `(RGBA8, 抓屏原点x, y, w, h)`（`None` = 还没抓到）。
    ///
    /// DWM 的亚克力在这套架构下不可用（wgpu 全客户区自绘把它盖住了），
    /// 所以"真的能看到桌面"只能自己抓屏 + 模糊（见 `crate::backdrop`），
    /// 再由 [`crate::refraction`] 画出来。
    ///
    /// ⚠️ 抓屏区域比窗口**大一圈**（`backdrop::margin`），原点也一并记下 ——
    /// 窗口挪动时按原点差平移采样（`backdrop::uv_map`），拖动全程零抓屏。
    ///
    /// ⚠️ 为什么存**像素**而不是 `image::Handle`：两种质感现在都走折射 shader，
    /// shader 得自己 `write_texture`，而 `image::Handle` 是不透明句柄、拿不回像素；
    /// 何况 `image` widget 走 iced 的图集缓存 —— 新句柄必然 cache miss，上传时
    /// 还要 `device.poll(Wait)` 同步等 GPU，且缓存**永不淘汰**。
    ///
    /// ⚠️ **必须是 `Arc`**：`draw()` 每帧都跑，`Arc::clone` 只加一次引用计数。
    bg_pixels: Option<BgPixels>,
    /// **抓屏那一刻的窗口矩形** `(x, y, w, h)`（屏幕物理像素）。
    ///
    /// 拖动检测要用它〔见 `bg_cycle`〕：拿"当前窗口矩形"和它比，位置差非零
    /// 且尺寸不变 ⇒ 窗口正在被拖动，可以跳过这一轮抓屏。
    ///
    /// ⚠️ 不能拿 `bg_pixels` 里的原点/宽高代替：那里存的是**抓屏区域的**原点
    /// （比窗口原点小一个 `margin`）和**降采样后的**宽高（比窗口小一个倍数），
    /// 两组数字根本不在同一个坐标系里。
    bg_win: Option<(i32, i32, u32, u32)>,
    /// 抓屏数据的**版本号**，每次 `BgCaptured`（且内容真的变了）自增。
    ///
    /// 折射 shader 的 `prepare()` 每帧都调，但抓屏最快 500ms 才来一帧新数据 ——
    /// 靠版本号比对就能跳过中间那些帧的 `write_texture`（纯浪费）。
    bg_version: u64,
    /// 自上一轮 `bg_cycle` 以来，窗口是否**又**动过。
    ///
    /// 这是"拖动中"的判据（光看位置差不够：拖完停下之后位置差仍然非零，
    /// 只靠它会永远跳过抓屏 ⇒ 桌面上放个视频背景就再也不刷新了）。
    /// 只有"移动事件还在持续到达"才算真的在拖。
    bg_moved: bool,
    /// 连续多少次抓屏**内容与上次完全一致**。
    ///
    /// 桌面静止时这是常态。用来做**自适应轮询**：一直没变就把节奏放慢，
    /// 一有变化立刻回到最快 —— "实时"和"省电"两边都要。
    bg_idle: u32,
    /// ⚠️ **抓屏链是否已经在跑**。
    ///
    /// 这是本次修复的核心。抓屏循环是"抓 → 睡 → 再抓"的 **Task 链**，
    /// 而每一条 `CaptureBg` 消息都会**再续一条** —— 于是任何一次
    /// `Moved` / `Resized` 窗口事件都会**永久**多出一条链。
    ///
    /// 实测：程序一启动就有 **3 条**（winit 建窗时会连发几次 `Resized`），
    /// 65 秒里一直是 3 条并发抓屏；用户拖一下窗口就是几十条。
    /// 每条链除了每轮做一次 40–95ms 的 GDI 抓屏，**还常驻占着一条 runtime
    /// 线程池线程睡觉**（`std::thread::sleep` 不释放线程）。链一多，线程池就被
    /// 睡满 ⇒ 别的 `Task` 排不上队 ⇒ 界面延迟越来越高、且**永远不恢复**。
    ///
    /// 现在外部请求只置 `bg_dirty`，链**有且只有一条**。
    bg_running: bool,
    /// 有外部事件（窗口移动 / 尺寸变化 / 换质感）要求"尽快再抓一次"。
    bg_dirty: bool,
    /// 是否把设置写回状态文件。
    ///
    /// ⚠️ **测试构造出来的 `State` 必须关掉它**（[`State::default`] 里是 `false`）——
    /// 否则每条"改一下设置"的用例都会去写用户真实的 `~/.hipercalc_state`，
    /// 跑一次测试就把人家的配置改了。只有产品路径（[`State::load`]）才打开。
    persist: bool,
    /// 函数弹窗是否展开
    show_funcs: bool,
    /// **渲染缩放**（窗口尺寸比例，1.0 = 940×860）
    ui_scale: f32,
    /// **允许被截图/录屏**：`false` 时窗口排除在捕获之外（背景=真实桌面，但截图拍不到）
    allow_capture: bool,
    /// 亚克力是否已启用（探测一次后固定）
    acrylic_checked: bool,
    /// 设置面板是否展开
    show_settings: bool,
    /// 精度输入框的文本（16 进制用不上，只是给 UI 一个缓冲）
    precision_text: String,
    /// 模式类设置的**界面侧副本**。
    ///
    /// worker 独占 `Session`，UI 读不到它的内部状态，所以显示模式 / 角度模式 /
    /// 计算模式 / 语言 / 精度都在 UI 侧留一份。改设置时**两处一起改**：
    /// 本副本（让 `view` 立刻画出新选中态）+ 一条 [`Command`] 发给 worker
    ///（让它手里的 `Session` 真按新设置求值）。两者在同一个 `update` 分支里
    /// 更新，不会失步。
    modes: Modes,
    /// 上一条输入（用于按 ↑ 重复）
    last_input: String,
    /// 当前键盘面板（主 / 字母）
    pub panel: Panel,
    /// Ctrl 键当前是否按下 —— 用来区分 Enter（求值）与 Ctrl+Enter（换行）。
    ///
    /// ⚠️ 必须自己跟踪，**不能等 `text_editor` 告诉我们**：`text_editor` 的
    /// 绑定表把 `Enter` 与 `Ctrl+Enter` **映射成同一个** `Binding::Enter`
    ///（见 `iced_widget/src/text_editor.rs` 的 `binding()`：
    /// `Named::Enter => Some(Self::Enter)`，不区分修饰键），
    /// 所以从 `Action` 上根本分不出 Ctrl 按没按。
    /// 只能靠键盘订阅里的 `ModifiersChanged` 自己记一份状态。
    ctrl_held: bool,
}

/// 模式类设置在**界面侧**的副本（见 [`State::modes`] 的说明）
#[derive(Debug, Clone, Copy, PartialEq)]
struct Modes {
    display: DisplayMode,
    angle: AngleMode,
    calc: CalcMode,
    lang: UiLang,
    precision: usize,
}

impl Default for Modes {
    fn default() -> Self {
        Self {
            // ⚠️ `Session::new()` 的显示模式是 **LineIO**（`Evaluator::new` 的默认值，
            // CLI 也一直如此）。GUI 显式用 MathIO —— 图形界面里"分数竖排、根号带顶线"
            // 才是用户要看的东西；LineIO 只给一串小数，浪费了排版能力。
            // 用户仍可在工具条上一键切回 LineIO。
            display: DisplayMode::MathIO,
            angle: AngleMode::Radian,
            calc: CalcMode::Fast,
            lang: UiLang::ZhCn,
            // `bigfloat::DEFAULT_PRECISION`（80），与 CLI 的初始值同一个来源
            precision: hipercalc_core::bigfloat::DEFAULT_PRECISION,
        }
    }
}

impl Modes {
    /// 从会话里把模式读回来（`State::load` 恢复配置后用）。
    ///
    /// 为什么不直接让 `restored()` 回传一份 `Modes`：`Modes` 是**界面侧**的概念
    /// （它是"worker 手里那份设置在 UI 的副本"），让 `hipercalc` 去认识它会把
    /// 依赖方向倒过来。读一次而已，代价可以忽略。
    fn from_session(s: &Session) -> Self {
        Self {
            display: s.display_mode(),
            angle: s.angle_mode(),
            calc: s.calc_mode(),
            lang: s.lang(),
            precision: s.precision(),
        }
    }
}

/// 消息
#[derive(Debug, Clone)]
pub enum Message {
    /// 编辑器内容变化
    /// 剪贴板读回来了（Ctrl+V 的第二段）
    Pasted(Option<String>),
    /// 按键（自己处理，不再转给 `text_editor`）
    KeyPressed(iced::keyboard::Key, iced::keyboard::Modifiers),
    /// 按虚拟键盘的某个键：`(行的下标, 列的下标)`
    Key(usize, usize),
    /// 按函数面板的某项：`(分组下标, 项下标)`
    Function(usize, usize),
    /// 求值
    Evaluate,
    /// 后台求值完成（由 [`crate::evaluator`] 的 future 回传）
    Evaluated(Evaluated),
    /// **取消当前计算**（用户点了「取消」按钮 / 输入栏的 `=` 变成取消时）
    Cancel,
    /// **起一个独立的命令行版进程**（设置按钮旁边那个 `>_`）
    LaunchCli,
    /// **把历史上第 n 条的「原式」插到光标前**（结果旁边那个插入键）
    InsertHistory(usize),
    /// **复制历史第 n 条的结果**到剪贴板（行尾悬停出现的复制按钮）
    CopyEntry(usize),
    /// 搜索框内容变化
    SearchChanged(String),
    /// 几何节流的"末尾补写"（由 [`State::save_geom_throttled`] 的延迟任务发回）
    SaveGeomNow,
    /// 切换「三角形求解」页（结果区）
    ToggleTriangle,
    /// 开关「自定义函数与变量」页
    ToggleUserDefs,
    /// 切换函数 / 变量模式（true = 函数）
    UdfMode(bool),
    /// 名称变化
    UdfName(String),
    /// 参数表变化（函数模式）
    UdfParams(String),
    /// 表达式 / 值变化
    UdfExpr(String),
    /// 搜索框变化
    UdfSearch(String),
    /// 页面内提交定义（走与主输入栏**同一条**求值通道）
    UdfSubmit,
    /// 删除一条用户定义（函数或变量）
    DelUserDef {
        /// true = 自定义函数，false = 存储变量
        is_fn: bool,
        name: String,
    },
    /// 三角形页某个输入框变化：`(字段下标 0..6, 内容)`
    TriChanged(usize, String),
    /// 三角形页：求解
    TriSolve,
    /// 三角形页：清空十二个输入
    TriClear,
    /// 设置：界面文字大小（0.7–1.6）
    SetFontScale(f32),
    /// 计算中的**秒表跳动**（订阅每 100ms 发一条，只为刷新"计算中 3.2s"）
    Tick,
    /// 点了排版层里的**空参数框**：把光标挪到源文本的第 n 个字符处
    /// 点击算式里的一个字：`right` 表示点的是**右半段**（光标落到该字符之后）
    PlaceCaret { at: usize, right: bool },
    /// 点输入栏的**空白处** ⇒ 光标送到整份输入的最后面。
    ///
    /// 为什么需要它：字符级点击目标只能表达"落在某个记号之前"，
    /// 最后一个记号**之后**那个位置没有目标可点（用户实测："光标在 4|5，
    /// 无论点哪里都无法移到 5 后面"）。这一条是兜底。
    PlaceCaretEnd,
    /// 点了排版层的空白处：只把焦点还给编辑器，不动光标
    FocusInput,
    /// 切换函数弹窗
    ToggleFuncs,
    /// 关闭函数弹窗（点浮层背景）
    CloseFuncs,
    /// 切换设置面板
    ToggleSettings,
    /// 关闭设置浮层（点面板外的任意位置）
    CloseSettings,
    /// 切换深色模式
    ToggleDark,
    /// 拖动玻璃强度滑条。⚠️ 拖动会**高频**触发，所以这条只改内存 + 重绘，
    /// 写盘交给松手时的 [`Message::CommitGlass`]。
    SetAcrylicBlur(f32),
    /// 拖动玻璃**底色浓度**滑条（同样只重绘，松手才写盘）
    SetAcrylicTint(f32),
    /// 切换质感模式（亚克力 / 玻璃 / 关闭）
    SetTexture(theme::Texture),
    /// 玻璃模式的模糊强度
    SetGlassBlur(f32),
    /// 玻璃模式的底色浓度
    SetGlassTint2(f32),
    /// 设置面板的浓度（亚克力模式下）—— 用户要求两种质感各配一个
    SetAcrylicDialogTint(f32),
    /// 设置面板的浓度（玻璃模式下）
    SetGlassDialogTint(f32),
    /// 拖动**投影强度**滑条（同上）
    SetShadow(f32),
    /// 玻璃滑条（强度或底色）松手 —— 这时才写配置
    CommitGlass,
    /// 一键开关玻璃：关 = 强度归零并记住原值，开 = 恢复到原值
    ToggleGlass,
    /// 长结果布局：`true` = 自动换行，`false` = 横向滚动
    SetWrap(bool),
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
    /// **外部**请求抓一帧自绘模糊背景（窗口移动 / 尺寸变化 / 换质感模式）。
    ///
    /// ⚠️ 语义是"请尽快再抓一次"，**不是**"再起一条循环"。
    /// 真正的循环由 [`Message::BgTick`] 承担，全局只有一条。
    CaptureBg,
    /// 窗口**被移动**了（附带新位置：只更新"记住的位置"，不重抓背景 —— 背景靠
    /// uv 偏移跟手，见 [`State::bg_cycle`] 里"拖动中跳过抓屏"的说明）。
    WindowMoved(iced::Point),
    /// 窗口**尺寸变化**（附带新尺寸：既要重抓背景，也要更新"记住的大小"）。
    WindowResized(iced::Size),
    /// 自绘窗口栏发来的意图（拖动 / 缩放 / 三个按钮）。
    ///
    /// 只用一个透传变体，而不是拆成五条消息：窗口栏的意图与 `app` 的处理
    /// 是**一对一**的，拆开只会让 match 变长。
    Chrome(crate::chrome::ChromeMessage),
    /// 窗口的最大化状态（查询回来后同步给标题栏图标）。
    WindowMaxState(bool),
    /// 抓屏链的**内部续期**（只有链自己会发，外部不许发）。
    ///
    /// 与 `CaptureBg` 分开是"单链"能成立的关键：`CaptureBg` 在链已经跑着时
    /// 直接返回（只置 `bg_dirty`），而 `BgTick` 无条件续 —— 于是无论外部
    /// 触发多少次，循环都只有一条。
    BgTick,
    /// 抓回来了
    BgCaptured(Option<crate::backdrop::Backdrop>),
    /// 历史区滚动（用来记住"用户是不是停在底部"）
    HistoryScrolled(iced::widget::scrollable::Viewport),
    /// **渲染缩放**（改窗口物理像素量；iced 没法降渲染分辨率，这是唯一确定见效的旋钮）
    SetUiScale(f32),
    /// **允许被截图/录屏**开关（见 `GuiPrefs::capture` 的说明：两个目标不能同时满足）
    SetCaptureOk(bool),
    /// 桌面模糊探测完成（`backdrop::probe` 的回传）
    AcrylicProbed(bool),
    /// 键盘修饰键状态变化（关键：用来区分 Enter 与 Ctrl+Enter）
    CtrlChanged(bool),
    /// 占位（订阅里不关心的窗口事件映射到这里，update 里什么都不做）
    Noop,
}

impl Default for State {
    fn default() -> Self {
        // ⚠️ 这里**刻意不读** `~/.hipercalc_state`（与 `State::load` 的分工）：
        // 几十条单测与快照测试都用它，若它去读用户主目录下的真实配置，
        // 测试就不可重复了（换台机器、或用户改过一次设置，基线全变）。
        let modes = Modes::default();
        let mut session = Session::new();
        // 把 UI 侧的初始模式**同步给 worker**（`Session::new()` 本身是 LineIO，
        // 而 GUI 默认 MathIO —— 不同步的话界面上高亮着 MathIO、算出来却是小数）
        session.set_display_mode(modes.display);
        session.set_precision(modes.precision);
        // 最后一个 `false` = **不写盘**（见 `State::persist` 字段的说明）
        Self::with_session(session, modes, GuiPrefs::default(), false)
    }
}

/// 产品路径的入口：构造 + **启动时探测桌面模糊**。
///
/// 探测走 Task（[`backdrop::probe`]）：一来 `EnumWindows` 反查需要窗口
/// **已经创建**（这里拿不到 HWND），二来探测是阻塞的 FFI 调用，
/// 不该占 UI 线程 —— 丢进 Task 后 runtime 会在线程池里跑。
pub fn boot() -> (State, Task<Message>) {
    let mut s = State::load();
    let dark = s.dark;
    // ⚠️ 首帧走 `bg_request` 而不是"自己发一条抓屏 Task" —— 后者会和链并存，
    // 又变成两条。这里直接把唯一的链起起来（`Off` 模式会被 `bg_cycle` 拒掉）。
    let bg = s.bg_request();
    (
        s,
        Task::batch([
            Task::perform(crate::backdrop::probe(dark), Message::AcrylicProbed),
            bg,
        ]),
    )
}

impl State {
    /// **产品路径**的构造：从 `~/.hipercalc_state` 恢复用户的配置。
    ///
    /// 用户明确要求："打开时应当从用户配置里读取信息"。恢复的东西包括
    /// 显示模式 / 角度 / 精度 / 语言 / `/let` 变量（CLI 侧那几个键）
    /// 以及 GUI 自己的深色、玻璃强度、行布局。
    ///
    /// `main.rs` 把它交给 `iced::application(..)`，所以窗口一建出来就是用户上次的样子。
    pub fn load() -> Self {
        let (session, prefs) = Session::restored();
        let modes = Modes::from_session(&session);
        // 最后一个 `true` = 允许把用户改的设置写回文件
        Self::with_session(session, modes, prefs, true)
    }

    /// 共用构造：把会话交给后台线程，并把进程级开关同步好。
    fn with_session(session: Session, modes: Modes, prefs: GuiPrefs, persist: bool) -> Self {
        // ⚠️ 进程级开关要在**第一次 `view` 之前**设好。`theme::DARK` 与
        // `theme::GLASS_PERCENT` 的静态初值只是"能跑"，不一定是用户的选择 ——
        // 若等第一次 `update` 再同步，首帧就会用错颜色（一闪而过，但快照会拍到）。
        theme::set_dark(prefs.dark);
        // 启动即按设置里的字号走（否则第一次开窗口字号是默认值，要等用户拖滑块）
        theme::set_font_scale(prefs.font_scale);
        theme::set_glass(prefs.glass);
        theme::set_glass_tint(prefs.glass_tint);
        theme::set_texture(theme::Texture::from_key(&prefs.texture));
        // 非 Windows 的玻璃模式走"窗口透传"（没有抓屏 ⇒ 由合成器把桌面透上来）。
        // 打一行日志，用户报"还是看不到背景"时能立刻确认走的是哪条路。
        if theme::desktop_passthrough() {
            eprintln!(
                "[hipercalc-gui] 玻璃透传模式：本平台没有抓屏实现，窗口对桌面透明 \
                 （需要合成器；KWin 可给窗口加「强制模糊」规则）"
            );
        }
        theme::set_glass2_blur(prefs.glass_blur);
        theme::set_glass2_tint(prefs.glass_tint2);
        theme::set_shadow(prefs.shadow);
        // 截图开关是"进程级 + Win32 亲和性"，启动时也要同步一次
        crate::backdrop::set_capture_ok(prefs.capture);
        Self {
            precision_text: modes.precision.to_string(),
            acrylic_checked: false,
            input: expr::Doc::default(),
            input_hint: None,
            history: Vec::new(),
            search: String::new(),
            page: ResultsPage::History,
            udf_is_fn: true,
            udf_name: String::new(),
            udf_params: String::new(),
            udf_expr: String::new(),
            udf_search: String::new(),
            udf_hint: None,
            user_fns: Vec::new(),
            user_vars: Vec::new(),
            tri_vals: Default::default(),
            tri_result: None,
            font_scale: prefs.font_scale,
            viewport: (WINDOW_W, WINDOW_H),
            dark: prefs.dark,
            ui_scale: prefs.ui_scale.clamp(0.5, 1.0),
            allow_capture: prefs.capture,
            acrylic_blur: prefs.glass,
            texture: theme::Texture::from_key(&prefs.texture),
            glass_blur: prefs.glass_blur,
            glass_tint2: prefs.glass_tint2,
            // 关着启动时，"开启"要回到出厂默认而不是 0
            glass_backup: if prefs.glass > 0.0 {
                prefs.glass
            } else {
                GuiPrefs::default().glass
            },
            wrap: prefs.wrap,
            // 启动时视作在底部（`anchor_bottom` 也会把初始位置放到底部）
            at_bottom: true,
            bg_pixels: None,
            bg_win: None,
            bg_version: 0,
            bg_moved: false,
            bg_idle: 0,
            bg_running: false,
            bg_dirty: false,
            acrylic_tint: prefs.glass_tint,
            acrylic_dialog_tint: prefs.acrylic_dialog_tint,
            glass_dialog_tint: prefs.glass_dialog_tint,
            shadow: prefs.shadow,
            persist,
            show_funcs: false,
            show_settings: false,
            maximized: prefs.win_max,
            win_geom: (prefs.win_x, prefs.win_y, prefs.win_w, prefs.win_h),
            win_max_pref: prefs.win_max,
            geom_saved_at: None,
            geom_save_pending: false,
            last_input: String::new(),
            panel: Panel::default(),
            ctrl_held: false,
            busy: false,
            busy_since: None,
            pending: None,
            modes,
            evaluator: Evaluator::spawn(session),
        }
    }
    /// 更新：只改状态，不画界面
    pub fn update(&mut self, message: Message) -> Task<Message> {
        // 样式函数读不到 State，明暗通过进程级开关传给主题模块
        theme::set_dark(self.dark);

        match message {
            // 点排版层空白处：现在没有独立编辑器抢焦点了，什么都不用做
            Message::FocusInput => {}
            // 点空参数框：直接把光标挪过去（**精确到字符偏移**，不再靠 Motion 逼近）
            Message::PlaceCaret { at, right } => self.place_caret(at, right),
            Message::PlaceCaretEnd => self.input.end(),
            Message::KeyPressed(key, modifiers) => return self.handle_key(key, modifiers),
            Message::Noop => {}
            Message::Key(r, c) => {
                if let Some(k) = layout_for(self.panel).get(r).and_then(|row| row.get(c)) {
                    match k.action {
                        KeyAction::Insert => self.insert(k.insert),
                        KeyAction::Backspace => self.backspace(),
                        KeyAction::Clear => self.clear_input(),
                        // 求值会清空输入框、把注意力转到结果区，**不还焦点**
                        KeyAction::Evaluate => return self.evaluate(),
                        // Desmos 的左右方向键：把光标挪一格，方便键盘用户改中间的数字
                        KeyAction::CursorLeft => self.move_caret(-1),
                        KeyAction::CursorRight => self.move_caret(1),
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
                return Task::none();
            }
            Message::Function(g, i) => {
                if let Some(item) = FN_GROUPS.get(g).and_then(|grp| grp.items.get(i)) {
                    self.insert(item.insert);
                }
                // 选完收起弹窗，并把焦点还给输入框 —— 用户接下来就是要接着打字
                self.show_funcs = false;
                return Task::none();
            }
            Message::Evaluate => return self.evaluate(),
            Message::ToggleFuncs => self.show_funcs = !self.show_funcs,
            Message::CloseFuncs => self.show_funcs = false,
            Message::ToggleSettings => self.show_settings = !self.show_settings,
            Message::CloseSettings => self.show_settings = false,
            Message::ToggleDark => {
                self.dark = !self.dark;
                theme::set_dark(self.dark);
                self.persist_prefs();
            }
            Message::CopyEntry(i) => {
                // 用户 2026-10-27："行尾悬停出现复制图标，点它复制该条结果"。
                //
                // 复制的是**结果文本**（不带 `= ` / `≈ ` 前缀 —— 那是界面上用来
                // 区分精确/近似的记号，粘到别处只会碍事）。
                if let Some(e) = self.history.get(i) {
                    let text = copy_payload(&e.output);
                    return iced::clipboard::write(text);
                }
            }
            Message::SearchChanged(q) => {
                self.search = q;
            }
            Message::ToggleTriangle => {
                self.page = self.page.toggled(ResultsPage::Triangle);
            }
            Message::ToggleUserDefs => {
                self.page = self.page.toggled(ResultsPage::UserDefs);
                self.udf_hint = None;
            }
            Message::UdfMode(is_fn) => {
                self.udf_is_fn = is_fn;
                self.udf_hint = None;
            }
            Message::UdfName(v) => {
                self.udf_name = v;
                self.udf_hint = None;
            }
            Message::UdfParams(v) => {
                self.udf_params = v;
                self.udf_hint = None;
            }
            Message::UdfExpr(v) => {
                self.udf_expr = v;
                self.udf_hint = None;
            }
            Message::UdfSearch(v) => self.udf_search = v,
            Message::UdfSubmit => {
                let name = self.udf_name.trim().to_string();
                let expr = self.udf_expr.trim().to_string();
                if name.is_empty() {
                    self.udf_hint = Some(t("先填名称"));
                    return Task::none();
                }
                if expr.is_empty() {
                    self.udf_hint = Some(t("先填表达式或值"));
                    return Task::none();
                }
                // 函数 ⇒ `f(x, y) = …`（参数任意个）；变量 ⇒ `/let A = …`
                // （不写成 `/let` 的话 `A=3` 会被当成方程去解）。
                let src = if self.udf_is_fn {
                    let params = self.udf_params.trim().trim_end_matches(',').to_string();
                    if params.is_empty() {
                        self.udf_hint = Some(t("函数至少要有一个参数，例如 x"));
                        return Task::none();
                    }
                    format!("{name}({params}) = {expr}")
                } else {
                    format!("/let {name} = {expr}")
                };
                // 借用主输入栏的求值通道；但**不动输入框内容**（用户可能正打到一半），
                // 也不切页 —— 成功后靠回包刷新列表，失败则把报错留在本页。
                let saved = self.input.clone();
                self.udf_name.clear();
                self.udf_params.clear();
                self.udf_expr.clear();
                let task = self.evaluate_lines(vec![src]);
                self.input = saved;
                return task;
            }
            Message::DelUserDef { is_fn, name } => {
                // 先就地更新快照（删的是自己刚列出来的行，不必等下一轮回包）
                if is_fn {
                    self.user_fns.retain(|(n, _, _)| n != &name);
                    self.evaluator.config(Command::DelFn(name));
                } else {
                    self.user_vars.retain(|(n, _)| n != &name);
                    self.evaluator.config(Command::DelVar(name));
                }
            }
            Message::TriChanged(i, v) => {
                if let Some(slot) = self.tri_vals.get_mut(i) {
                    *slot = v;
                }
            }
            Message::TriClear => {
                self.tri_vals = Default::default();
                self.tri_result = None;
            }
            Message::SetFontScale(k) => {
                let k = k.clamp(0.7, 1.6);
                self.font_scale = k;
                theme::set_font_scale(k);
                self.persist_prefs();
            }
            Message::TriSolve => {
                // 拼成 core 认的 `triangle(a=3, A=30)`：**只带填了的字段**，
                // 空字段不写（写了 `a=` 会被当成"给了个空值"直接报错）。
                let args: Vec<String> = crate::app::triangle::TRI_KEYS
                    .iter()
                    .zip(self.tri_vals.iter())
                    .filter(|(_, v)| !v.trim().is_empty())
                    .map(|(k, v)| format!("{k}={}", v.trim()))
                    .collect();
                if args.is_empty() {
                    self.input_hint = Some(t("至少填一个已知量"));
                } else {
                    let src = format!("triangle({})", args.join(", "));
                    // 走**正式求值通道**（后台 worker），结果照常进历史 ——
                    // 这一页只是"帮我把参数填对"，不另开一套计算路径。
                    self.input = expr::Doc::from_text(&src);
                    // ⚠️ **不要跳回历史页**（用户 2026-10-29）：结果就地显示在
                    // 这一页下方（`tri_result`），历史里那份记录照常保留。
                    self.input_hint = None;
                    return self.evaluate();
                }
            }
            Message::SaveGeomNow => {
                self.geom_save_pending = false;
                self.geom_saved_at = Some(std::time::Instant::now());
                self.persist_prefs();
            }
            Message::InsertHistory(i) => {
                // 用户 2026-10-24："计算完的式子旁边增加插入键，插入输入栏里光标前面"。
                //
                // 插的是**原式**（`Entry::input`，用户当时敲的那串），不是结果 ——
                // 想再算一遍、或者在某一步上改一改，要的是式子。
                // 走 `insert_key` 而不是 `insert_text`：它会把 `sum(k^2,k,1,10)`
                // 这类模板重新解析成**结构**，插进去就是可编辑的 Σ，而不是一坨文本。
                if let Some(e) = self.history.get(i) {
                    let src = e.input.clone();
                    self.input.insert_key(&src);
                    self.input_hint = None;
                    // 不用 `operation::focus`：输入框是**自绘的结构化编辑器**，
                    // 光标是我们自己的模型（`Doc::cursor`），键盘走窗口级订阅，
                    // 不存在"抢焦点"这回事。
                }
            }
            Message::LaunchCli => {
                // 起**独立进程**：GUI 关掉也不影响它（用户明确要求）。
                // 失败时不打扰用户 —— 只在 stderr 留一行（开发时能看见）。
                if let Err(e) = crate::cli::spawn_cli() {
                    eprintln!("[hipercalc-gui] 启动命令行失败: {e}");
                }
            }
            Message::Cancel => {
                // 只置标志：真正的退出发生在求解器的检查点（`core::cancel::check`），
                // 结果会以「计算已中断」回来 ⇒ 走正常的 `Evaluated` 通道收尾。
                hipercalc_core::cancel::request();
            }
            // 秒表消息本身不做事 —— 它唯一的作用是"让 iced 重绘一帧"，
            // 于是 `view` 里那句 `busy_since.elapsed()` 就被重新读一次。
            Message::Tick => {}
            Message::SetUiScale(k) => {
                self.ui_scale = k.clamp(0.5, 1.0);
                self.persist_prefs();
                // 立刻改窗口物理尺寸：像素量按 k² 下降（帧时间近似线性）
                return iced::window::latest().and_then(move |id| {
                    iced::window::resize(id, iced::Size::new(WINDOW_W * k, WINDOW_H * k))
                });
            }
            Message::SetCaptureOk(ok) => {
                self.allow_capture = ok;
                crate::backdrop::set_capture_ok(ok);
                // 亲和性一变，之前抓到的帧就"作废"了（里面有没有自己不一样）
                self.bg_pixels = None;
                self.bg_win = None;
                self.bg_idle = 0;
                theme::set_bg_ready(false);
                self.persist_prefs();
                return self.bg_request();
            }
            Message::SetTexture(t) => {
                self.texture = t;
                theme::set_texture(t);
                // 设置页浓度是**按模式**存的：切质感时把对应那份推给 theme
                theme::set_dialog_tint(match t {
                    theme::Texture::Glass => self.glass_dialog_tint,
                    _ => self.acrylic_dialog_tint,
                });
                // ⚠️ **模式一变，旧像素必须整体作废**。
                //
                // 否则会撞上这样的 bug：换模式后抓到的像素恰好与上次相同 ⇒ 走
                // "内容没变"的快路径直接返回 ⇒ **切过去之后没有背景**。
                self.bg_pixels = None;
                self.bg_win = None;
                self.bg_idle = 0;
                theme::set_bg_ready(false);
                // ⚠️ **切质感必须落盘**：这里以前直接 `return`，于是"关掉质感"
                // 只存在于本进程 —— 用户实测"关闭前设了关闭质感，再次打开又变成
                // 玻璃质感"。`persist_prefs` 里已经有 `texture` 这个键，只是没被调用。
                self.persist_prefs();
                // 模式变了：请链尽快再抓一帧（`Off` 会被 `bg_cycle` 拒掉并停链）。
                // ⚠️ 走 `bg_request` 而不是自己发一条抓屏 Task —— 后者会多出一条链。
                return self.bg_request();
            }
            Message::SetGlassBlur(v) => {
                self.glass_blur = v.clamp(0.0, 1.0);
                theme::set_glass2_blur(self.glass_blur);
                // 模糊强度变了 ⇒ 抓屏的降采样倍数变了 ⇒ 下一张一定与现在不同。
                // 把"静止计数"清掉，让轮询回到 500ms，拖滑条时背景跟得上。
                self.bg_idle = 0;
            }
            Message::SetGlassTint2(v) => {
                self.glass_tint2 = v.clamp(0.0, 1.0);
                theme::set_glass2_tint(self.glass_tint2);
            }
            Message::SetAcrylicBlur(v) => {
                // 拖动中：只改内存 + 重绘。写盘留给 `CommitGlass` ——
                // 滑条一次拖动会发出几十条消息，每条都写文件既慢又浪费。
                self.acrylic_blur = v.clamp(0.0, 1.0);
                if self.acrylic_blur > 0.0 {
                    self.glass_backup = self.acrylic_blur;
                }
                theme::set_glass(self.acrylic_blur);
                // 与 `SetGlassBlur` 同理：降采样倍数变了，轮询要回到快节奏
                self.bg_idle = 0;
            }
            Message::SetAcrylicTint(v) => {
                // 与 `SetGlass` 同一套路：拖动只重绘，落盘在 `CommitGlass`
                self.acrylic_tint = v.clamp(0.0, 1.0);
                theme::set_glass_tint(self.acrylic_tint);
            }
            Message::SetAcrylicDialogTint(v) => {
                self.acrylic_dialog_tint = v.clamp(0.0, 1.0);
                theme::set_dialog_tint(self.acrylic_dialog_tint);
            }
            Message::SetGlassDialogTint(v) => {
                self.glass_dialog_tint = v.clamp(0.0, 1.0);
                theme::set_dialog_tint(self.glass_dialog_tint);
            }
            Message::SetShadow(v) => {
                self.shadow = v.clamp(0.0, 1.0);
                theme::set_shadow(self.shadow);
            }
            Message::CommitGlass => self.persist_prefs(),
            Message::ToggleGlass => {
                if self.acrylic_blur > 0.0 {
                    // 关：记住当前值，好让用户再点一次能回到原样
                    self.glass_backup = self.acrylic_blur;
                    self.acrylic_blur = 0.0;
                } else {
                    // 开：回到上次的强度；若从没调过（备份是 0）就用出厂默认
                    self.acrylic_blur = if self.glass_backup > 0.0 {
                        self.glass_backup
                    } else {
                        GuiPrefs::default().glass
                    };
                }
                theme::set_glass(self.acrylic_blur);
                self.persist_prefs();
            }
            Message::SetWrap(on) => {
                self.wrap = on;
                self.persist_prefs();
            }
            Message::SetDisplay(m) => {
                self.modes.display = m;
                self.evaluator.config(Command::SetDisplay(m));
                self.persist_prefs();
            }
            Message::SetAngle(m) => {
                self.modes.angle = m;
                self.evaluator.config(Command::SetAngle(m));
                self.persist_prefs();
            }
            Message::SetCalc(deep) => {
                let m = if deep { CalcMode::Deep } else { CalcMode::Fast };
                self.modes.calc = m;
                self.evaluator.config(Command::SetCalc(m));
                self.persist_prefs();
            }
            Message::ClearHistory => self.history.clear(),
            Message::Pasted(text) => {
                // Ctrl+V：把剪贴板内容插到光标处（换行折成空格 —— 表达式不该跨行）
                if let Some(t) = text {
                    let one_line = t.replace(['\r', '\n'], " ");
                    self.insert(&one_line);
                }
            }
            Message::ClearInput => self.clear_input(),
            Message::SetPanel(p) => self.panel = p,
            Message::SetLang(l) => {
                // ⚠️ 语言**只能**在 UI 线程上同步设，不能发给 worker 代劳。
                //
                // 原因：语言是**进程级**全局（`i18n` 里的 `AtomicU8`），而界面自己的
                // 每一处标签都走 `t()` / `t_fmt()` 直接读那个全局值 —— 就在这一帧的
                // `view` 里读。发给 worker 是**异步**的：worker 要等下一次请求才执行，
                // 于是用户点完语言按钮后**本帧的界面还是旧语言**，什么时候翻过来
                // 取决于下一次求值，看起来就像坏掉了。
                //
                // 而且让 worker 也去改那个全局值会引入**跨线程写全局**的竞态
                //（实测：一个用例刚还原成中文，被另一个用例的 worker 延迟改回英文）。
                // 所以 `evaluator::Command` 里**没有** `SetLang` —— 从类型上就堵住这条路。
                hipercalc::session::set_ui_lang(l);
                self.modes.lang = l;
                self.persist_prefs();
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
            Message::CtrlChanged(held) => self.ctrl_held = held,
            Message::ApplyPrecision => {
                // 夹取到合法区间。`Session::set_precision` 自己也会夹，
                // 但 worker 独占会话、UI 读不回来，所以这里用同一套边界先夹一次，
                // 保证界面上回填的数字与 worker 实际采用的一致。
                let n = match self.precision_text.trim().parse::<usize>() {
                    Ok(n) => n.clamp(hipercalc::MIN_PRECISION, hipercalc::MAX_PRECISION),
                    Err(_) => self.modes.precision,
                };
                self.modes.precision = n;
                self.evaluator.config(Command::SetPrecision(n));
                // 夹过之后回填，让用户看到真实生效的值
                self.precision_text = n.to_string();
                self.persist_prefs();
            }
            Message::Evaluated(out) => {
                self.busy = false;
                self.busy_since = None;
                self.pending = None;
                let precise = is_precise(&out.text);
                // 「计算已中断」是**用户自己按的取消**，不是失败：按提示样式显示，
                // 不算 error（否则整行染红、还带错误底色）。
                let cancelled = out.text.contains(hipercalc_core::cancel::ERROR_CANCELLED);
                // 三角页开着 ⇒ 结果同时就地显示（不跳页）
                self.user_fns = out.user_fns.clone();
                self.user_vars = out.user_vars.clone();
                if self.page == ResultsPage::Triangle {
                    self.tri_result = Some(out.text.clone());
                }
                self.history.push(Entry {
                    input: out.input,
                    output: out.text,
                    error: out.is_error && !cancelled,
                    notice: cancelled,
                    precise,
                    elapsed: out.elapsed,
                });
                // ⚠️ **贴底跟随**：只有"原本就在底部"时才自动滚下去。
                // 用户翻旧记录时（不在底部）不该被强行拽走。
                if self.at_bottom {
                    return iced::widget::operation::snap_to(
                        HISTORY_ID,
                        iced::widget::scrollable::RelativeOffset { x: 0.0, y: 1.0 },
                    );
                }
            }
            Message::CaptureBg => {
                // **玻璃关掉就不抓了**（省 CPU），并把旧图丢掉。
                //
                // ⚠️ 判据是**模式**（`Off`），不是模糊强度 —— 用户实测反馈：
                // "模糊强度为 0 还是纯色，完全无法看到桌面"。强度 0 只是"不模糊"，
                // 背景**仍要显示**；之前拿 `glass_amount() <= 0.0` 判断，
                // 强度一归零就把整个抓屏循环停掉、背景层也跳过，于是只剩纯色。
                if theme::texture() == theme::Texture::Off {
                    if self.bg_pixels.is_some() {
                        self.bg_pixels = None;
                        self.bg_win = None;
                        theme::set_bg_ready(false);
                    }
                    self.bg_running = false;
                    // 玻璃关着也要同步最大化状态（标题栏图标不能停在旧样子）
                    return Self::sync_max_state();
                }
                // ⚠️ **链已经在跑就只打个标记，绝不再起第二条**。
                //
                // 这是"延迟越来越高、且永远不恢复"的根因（详见 [`State::bg_running`]
                // 的注释）：实测启动即有 3 条链并发抓屏，拖一下窗口就是几十条。
                //
                // 顺带同步最大化状态：`Resized` 是这里唯一的触发源，
                // 而最大化/还原必然伴随尺寸变化 —— 两件事合在一条消息里做，
                // 省掉一个额外的订阅。
                return Task::batch([self.bg_request(), Self::sync_max_state()]);
            }
            // 链的内部续期：无条件再跑一轮（唯一的循环入口）
            Message::BgTick => return self.bg_cycle(),
            // 窗口移动：只重绘（view 里现算 uv 偏移），不重抓屏。
            //
            // 但**要留下"还在动"的痕迹**：`bg_moved` 让 `bg_cycle` 知道本轮可以
            // 跳过抓屏；`bg_dirty` 让"停下来之后的那一轮"立刻补抓一帧
            //（否则要等 `bg_idle` 把间隔放慢到 2s，拖完会有明显的一段旧背景）。
            Message::WindowMoved(pos) => {
                self.bg_moved = true;
                self.bg_dirty = true;
                // 最大化时窗口位置是屏幕左上角，那不是"用户想要的位置" ⇒ 只在
                // 普通状态下记。大小同理（见 `WindowResized`）。
                if !self.maximized {
                    self.win_geom.0 = pos.x;
                    self.win_geom.1 = pos.y;
                    return self.save_geom_throttled();
                }
            }
            Message::WindowResized(size) => {
                // 布局用的视口尺寸：**每次 resize 都更新**，设置面板据此重排
                //（最大化也更新 —— 最大化时窗口更大，那一行本该合并回一行）
                self.viewport = (size.width, size.height);
                if !self.maximized {
                    self.win_geom.2 = size.width;
                    self.win_geom.3 = size.height;
                    let save = self.save_geom_throttled();
                    return Task::batch([save, self.bg_request()]);
                }
                // 尺寸变了，抓屏区域也要跟着变（最大化时不记几何，但仍要重抓）
                return self.bg_request();
            }
            // ── 自绘窗口栏 ────────────────────────────────────────────────
            Message::Chrome(action) => return self.chrome(action),
            Message::WindowMaxState(max) => {
                // 只在与当前不同时改 —— 每次改都会触发一次重建 view/重绘，
                // 而窗口缩放期间 `Resized` 会连着来好几十条。
                if self.maximized != max {
                    self.maximized = max;
                }
            }
            Message::BgCaptured(bd) => {
                if let Some(b) = bd {
                    // ── ① 内容没变就**什么都不做** ─────────────────────────
                    //
                    // 这是本次省 CPU 最关键的一步。原来每一轮抓屏都无条件
                    // `Handle::from_rgba(..)` —— iced 的 `image::Handle` 每建一次
                    // 就是一个**全新 id**，图集缓存必然 miss，于是每次都要把
                    // 几十万像素重新拷一遍、重新传进图集。桌面静止时这纯属白干。
                    //
                    // 比一次 memcmp（~0.1ms）就能判定"没变"，代价可以忽略。
                    //
                    // ⚠️ 这里可以**整张**比较：抓到的帧里"我们自己的窗口"那块
                    // 已经被 `backdrop::fill_self_rect` 用周围颜色填平了，
                    // 所以界面重绘**不会**让内容变化（否则会自激 50Hz 空转重建）。
                    let same = match &self.bg_pixels {
                        Some((px, _, _, w, h, _, _)) => {
                            *w == b.width && *h == b.height && px[..] == b.rgba[..]
                        }
                        None => false,
                    };
                    if same {
                        self.bg_idle += 1;
                        // ⚠️ **必须照样续链**：早期版本这里直接 `return Task::none()`，
                        // 于是"桌面静止"反而把唯一的循环掐断了 —— 之后再也无法刷新。
                        return self.bg_cycle();
                    }
                    self.bg_idle = 0;

                    // 原始像素留给折射 shader。
                    // 一次 `Arc::new` 之后，每帧 `draw()` 只 clone 引用计数。
                    let px = Arc::new(b.rgba);
                    self.bg_pixels =
                        Some((Arc::clone(&px), b.x, b.y, b.width, b.height, b.rw, b.rh));
                    // 记下"这张图对应窗口在哪" —— 拖动检测靠它（见 [`State::bg_win`]）。
                    //
                    // ⚠️ 抓屏本身要 40–95ms，期间窗口可能又挪了一点：那这一轮记的
                    // 位置会略微滞后，表现为"多跳过一轮"。下一轮位置对上了会立刻
                    // 补抓，是自愈的。
                    self.bg_win = crate::backdrop::window_rect();
                    // 版本号 +1 ⇒ shader 下一帧才会真的 `write_texture`
                    self.bg_version += 1;
                    // 告诉根容器"下面有东西了，你可以全透了"
                    theme::set_bg_ready(true);
                }
                // 抓完一轮，续下一次（链的唯一出口）
                return self.bg_cycle();
            }
            Message::HistoryScrolled(viewport) => {
                // 0.98 的余量：滚动条取整后很难精确等于 1.0
                self.at_bottom = viewport.relative_offset().y >= 0.98;
            }
            Message::AcrylicProbed(ok) => {
                // ⚠️ 探测**只能在窗口创建之后**做，而且要在它出现在
                // `EnumWindows` 里之后（`backdrop::probe` 里有 150ms 的等待）。
                if !self.acrylic_checked {
                    self.acrylic_checked = true;
                    theme::set_acrylic(ok);
                    if !ok {
                        eprintln!(
                            "[hipercalc-gui] 未启用 DWM 亚克力（远程桌面/旧系统不支持时正常）；                                   界面改用不透明底色 + 氛围光。设置里的'玻璃强度'仍可调。"
                        );
                    } else {
                        eprintln!("[hipercalc-gui] DWM 亚克力已启用");
                    }
                }
            }
            Message::WindowOpened => {
                // **恢复"上次退出时是最大化"**：`window::Settings` 只能给尺寸/位置，
                // 没有"以最大化启动"的字段，所以在窗口真正建好之后补一次。
                if self.win_max_pref && !self.maximized {
                    return win_task(move |id| iced::window::maximize::<Message>(id, true));
                }
                // ⚠️ **除此之外这里刻意什么都不做**。
                //
                // 上一版在这里调 `backdrop::enable` 并设 `acrylic_checked = true`，
                // 结果是：它先于 boot 里的 Task 触发，把 `acrylic_checked` 抢占了，
                // 于是 `AcrylicProbed` 到来时被挡掉 —— **而它的成功路径不打印任何东西**，
                // 用户看到的"控制台一行输出都没有"就是这么来的（我据此误判成
                // "Opened 事件没触发"，白查了一轮）。
                //
                // 现在探测只走 `backdrop::probe`（boot 里的 Task，成功失败都会报告，
                // 而且结果会显示在设置面板里）。
            }
        }
        Task::none()
    }

    /// 把当前设置写回 `~/.hipercalc_state`。
    ///
    /// 走的是**键级更新**（[`hipercalc::session::save_prefs`]）：只替换下面这几个键，
    /// 文件里的 `/let` 变量表、`/set` 颜色、以及 CLI 自己的键**原样保留** ——
    /// 否则 GUI 每存一次设置就会把命令行那边的变量全清掉。
    ///
    /// 失败只 `eprintln!`：设置存不下来（只读目录、磁盘满）不该让计算器不可用。
    /// 几何变化的**节流写盘**：立即写一次，若 2 秒内还有变化则挂一个"补写"任务。
    ///
    /// ⚠️ 只做"丢弃式节流"是不够的：一次拖动的**位置和尺寸是两个事件**，
    /// 位置那个先到、立刻写盘，尺寸那个紧随其后却被节流丢掉 —— 实测就是
    /// "位置记住了、大小没记住"。所以必须有**末尾补写**。
    fn save_geom_throttled(&mut self) -> Task<Message> {
        let now = std::time::Instant::now();
        let wait = match self.geom_saved_at {
            Some(t) => 2.0 - now.duration_since(t).as_secs_f32(),
            None => 0.0,
        };
        if wait <= 0.0 {
            self.geom_saved_at = Some(now);
            self.persist_prefs();
            return Task::none();
        }
        // 已经排了一个补写就不再排（否则一次拖动会排几十个）
        if self.geom_save_pending {
            return Task::none();
        }
        // 真正的"延时补写"交给订阅里的定时器（见 `subscription`）：
        // 这里只置标志，定时器到点发 [`Message::SaveGeomNow`]。
        self.geom_save_pending = true;
        Task::none()
    }

    fn persist_prefs(&self) {
        // 测试构造的 State 一律不写盘（否则跑一次测试就改掉用户的真实配置）
        if !self.persist {
            return;
        }
        let pairs: Vec<(&str, String)> = vec![
            (
                "display",
                match self.modes.display {
                    DisplayMode::MathIO => "mathio",
                    DisplayMode::LineIO => "lineio",
                }
                .to_string(),
            ),
            (
                "angle",
                match self.modes.angle {
                    AngleMode::Degree => "deg",
                    AngleMode::Radian => "rad",
                }
                .to_string(),
            ),
            (
                "calc",
                match self.modes.calc {
                    CalcMode::Deep => "deep",
                    CalcMode::Fast => "fast",
                }
                .to_string(),
            ),
            ("prec", self.modes.precision.to_string()),
            ("lang", self.modes.lang.code().to_string()),
            ("dark", if self.dark { "on" } else { "off" }.to_string()),
            ("glass", format!("{:.2}", self.acrylic_blur)),
            ("texture", self.texture.key().to_string()),
            (
                "capture",
                if self.allow_capture { "on" } else { "off" }.to_string(),
            ),
            ("ui_scale", format!("{:.2}", self.ui_scale)),
            ("font_scale", format!("{:.2}", self.font_scale)),
            // 窗口几何：没拿到过事件（w == 0）就不写，免得首启把"居中"覆盖成 (0,0)
            ("win_x", format!("{:.0}", self.win_geom.0)),
            ("win_y", format!("{:.0}", self.win_geom.1)),
            (
                "win_w",
                format!(
                    "{:.0}",
                    if self.win_geom.2 > 0.0 {
                        self.win_geom.2
                    } else {
                        0.0
                    }
                ),
            ),
            (
                "win_h",
                format!(
                    "{:.0}",
                    if self.win_geom.3 > 0.0 {
                        self.win_geom.3
                    } else {
                        0.0
                    }
                ),
            ),
            (
                "win_max",
                if self.win_max_pref { "on" } else { "off" }.to_string(),
            ),
            ("glass2_blur", format!("{:.2}", self.glass_blur)),
            ("glass2_tint", format!("{:.2}", self.glass_tint2)),
            ("glass_tint", format!("{:.2}", self.acrylic_tint)),
            // 设置面板自己的浓度（两种质感各一个）
            (
                "acrylic_dlg_tint",
                format!("{:.2}", self.acrylic_dialog_tint),
            ),
            ("glass2_dlg_tint", format!("{:.2}", self.glass_dialog_tint)),
            ("shadow", format!("{:.2}", self.shadow)),
            ("wrap", if self.wrap { "on" } else { "off" }.to_string()),
        ];
        if let Err(e) = hipercalc::session::save_prefs(&pairs) {
            eprintln!("[hipercalc-gui] 设置保存失败（不影响使用）：{e}");
        }
    }

    /// 点排版层里的某个可点目标（空槽位框、或某个记号）⇒ 光标送到那一位。
    ///
    /// `id` 是 [`expr::Doc::targets`] 的下标 —— **不是字符偏移**了。
    /// 两者必须与渲染时登记的 id 同源（`to_node` 与 `targets` 是同一个遍历），
    /// 否则"点哪去哪"会错位。
    /// 把光标放到第 `id` 个可点目标上。
    ///
    /// `right == true` 表示用户点的是那个字符的**右半段**：先落到它之前，再右移一格 ——
    /// 这样"点右半边 = 光标在该字符之后"（用户 2026-10-31 报的 bug：
    /// 之前只有"之前"一种落点，字符右半段点了没反应）。
    fn place_caret(&mut self, id: usize, right: bool) {
        self.input.place(id);
        if right {
            self.input.right();
        }
    }

    /// 清空输入
    fn clear_input(&mut self) {
        self.input.clear();
        self.input_hint = None;
    }

    // ── 自绘窗口栏（标题栏 / 缩放 / 三键）──────────────────────────────────

    /// 标题栏组件。
    ///
    /// 副标题走 `i18n`（中文"超高精度计算器"/英文"High-Precision Calculator"），
    /// 品牌名 `HiPerCalc` 是专有名词，不翻译。
    fn titlebar_area(&self) -> Element<'_, Message> {
        let bar = crate::chrome::titlebar(
            "HiPerCalc".to_string(),
            t("超高精度计算器"),
            self.maximized,
            Message::Chrome,
        );
        // ⚠️ **透传模式下标题栏要垫一层淡底**：Windows 上标题栏下面是抓来的桌面帧
        // （或氛围底），文字有承托；Linux/macOS 透传时下面是**裸壁纸**，
        // 白字压到浅色壁纸上直接看不见。这里补一层半透明主题底色。
        if theme::desktop_passthrough() {
            let c = theme::colors();
            container(bar)
                .width(Length::Fill)
                .style(move |_t| container::Style {
                    background: Some(Background::Color(c.backdrop.scale_alpha(0.72))),
                    ..container::Style::default()
                })
                .into()
        } else {
            bar
        }
    }

    /// 窗口栏的意图 → 系统窗口命令。
    ///
    /// ⚠️ 拖动 / 缩放都是**把控制权交给系统**（`drag` / `drag_resize` 会进入
    /// Windows 自己的移动/缩放循环），我们不跟踪 mousemove —— 那样既跟不牢，
    /// 也会和 DWM 的贴边吸附、Win+方向键冲突。
    fn chrome(&mut self, action: crate::chrome::ChromeMessage) -> Task<Message> {
        use crate::chrome::ChromeMessage as C;
        use iced::window;
        match action {
            C::Drag => win_task(window::drag),
            C::Resize(dir) => {
                // 最大化时不允许拖边（系统也不允许，这里直接忽略）
                if self.maximized {
                    return Task::none();
                }
                win_task(move |id| window::drag_resize(id, dir))
            }
            C::Minimize => win_task(|id| window::minimize::<Message>(id, true)),
            C::ToggleMax => {
                // 乐观更新：图标立刻切成"还原"，系统那边稍后由 `is_maximized` 校准
                let next = !self.maximized;
                self.maximized = next;
                win_task(move |id| window::maximize::<Message>(id, next))
            }
            C::Close => {
                // **退出前落盘**：窗口位置/大小/最大化只在关闭这一刻写 ——
                // 拖动窗口每秒几十个 Moved 事件，逐个写盘会把磁盘打满。
                self.win_max_pref = self.maximized;
                self.persist_prefs();
                win_task(window::close::<Message>)
            }
        }
    }

    /// 查一次"窗口是否最大化"，结果经 [`Message::WindowMaxState`] 回来。
    ///
    /// 为什么不在本地维护：双击标题栏、`Win+↑`、拖到屏幕顶端都会最大化，
    /// 本地记的位一定会漂 —— 只有问系统才准。
    fn sync_max_state() -> Task<Message> {
        win_task(|id| iced::window::is_maximized(id).map(Message::WindowMaxState))
    }

    /// 把一段"键面文本"插到光标处（键盘、函数面板、粘贴都走这里）
    ///
    /// 结构键（`sqrt()` / `∑()` / `(` / `^` …）由 [`expr::Doc::insert_key`]
    /// 变成真正的树节点，普通记号则按字符切开。
    fn insert(&mut self, text: &str) {
        self.input.insert_key(text);
        // 一有输入就把"没写完"的提示撤掉（用户已经在改了）
        self.input_hint = None;
    }

    /// 退格。⚠️ 只动**当前这一行**：光标在该行最前面时什么都不做 ——
    /// 绝不会顺手把外层结构（Σ/根号）删掉（用户拍板的防误删规则）。
    fn backspace(&mut self) {
        self.input.backspace();
        self.input_hint = None;
    }

    /// 向前删（`Delete` 键），同样不会碰外层结构
    fn delete_forward(&mut self) {
        self.input.delete();
    }

    /// 光标左右移动。
    ///
    /// ⚠️ 走的是**线性阅读序**（[`expr::Doc::linear`]），不是"字符偏移 ±1"：
    /// 摊平后每一项都是合法位置，所以**光标永远不会落在括号上消失**
    /// —— 这正是用户报的 `∑()` 里按 ← 光标不见了的根因。
    fn move_caret(&mut self, delta: isize) {
        if delta < 0 {
            self.input.left();
        } else {
            self.input.right();
        }
    }

    /// 跑**一轮**抓屏循环：抓一帧 → 睡一会儿 → 发 [`Message::BgTick`] 续下一轮。
    ///
    /// ## ⚠️ 这是全进程唯一的一条抓屏链
    ///
    /// 见 [`State::bg_running`] 的注释：老写法是"每条 `CaptureBg` 都自我续期"，
    /// 于是窗口每来一次 `Moved` / `Resized` 就永久多出一条链（实测启动即 3 条，
    /// 拖窗口后几十条），每条链还常驻占一条 runtime 线程睡觉 —— 线程池被睡满后
    /// 界面延迟一路涨到十几秒且**永不恢复**。
    ///
    /// 现在 `CaptureBg` 只置 `bg_dirty`，循环本体只有这里一个入口，
    /// 所以链的数量**恒等于 1**。
    fn bg_cycle(&mut self) -> Task<Message> {
        // 关掉质感 ⇒ 链到此为止（把 `bg_running` 放开，下次开质感时由
        // `SetTexture` 那条路径重新起链）
        if theme::texture() == theme::Texture::Off {
            if self.bg_pixels.is_some() {
                self.bg_pixels = None;
                self.bg_win = None;
                theme::set_bg_ready(false);
            }
            self.bg_running = false;
            // 让可能正在轮询的循环退出（否则那条 future 永远不返回，
            // 每条线程池线程就这么被占住了 —— 见 [`crate::dupl::stop`]）
            #[cfg(windows)]
            crate::dupl::stop();
            return Task::none();
        }
        self.bg_running = true;
        #[cfg(windows)]
        crate::dupl::resume();

        // ── ⚠️ 拖动中：跳过抓屏 ──────────────────────────────────────────
        //
        // 窗口原点与抓屏原点已经错开 ⇒ 说明窗口正在被拖动。此时背景**完全
        // 不需要重抓**：着色器按原点差平移采样（`backdrop::uv_map`），
        // 几何上就是精确的（抓屏区域还外扩了半个窗口的余量）。
        //
        // 老版本在这里每 500ms 重抓一次（抓屏 40–95ms 的 GDI 回读）：
        // 背景刷新率掉到 2fps，而且每次都是一次 CPU 尖峰 ——
        // 用户实测"拖动窗口时背景一卡一卡，目测小于 1 帧"。
        // 跳过之后拖动全程**零抓屏**，刷新跟着事件的 60Hz 走，CPU 也只剩重绘。
        //
        // ⚠️ **判定必须拿"抓屏那一刻的窗口矩形"来比**（见 [`State::bg_win`]）：
        // `bg_pixels` 里存的是**抓屏**原点与**降采样后**的宽高，
        // 拿它们去和 `window_rect()` 比是永远比不上的（抓屏原点比窗口原点
        // 小一个 `margin`，宽高又差一个降采样倍数）⇒ 上一版这条分支是死代码，
        // 拖动时照样每 500ms 重抓一次（实测拖动 CPU 1.10%，正是这 40–95ms 的
        // GDI 回读在 2Hz 上重复）。
        let dragging = std::mem::take(&mut self.bg_moved);
        if dragging
            && let Some((cx, cy, cw, ch)) = self.bg_win
            && let Some((wx, wy, ww, wh)) = crate::backdrop::window_rect()
        {
            let m = crate::backdrop::margin(ww, wh);
            let dx = (wx - cx).abs();
            let dy = (wy - cy).abs();
            // ① 尺寸没变（变了 ⇒ 抓屏区域对不上，必须重抓）；
            // ② 位移还在外扩余量内 ⇒ 偏移采样仍落在真实像素上（超出就是 clamp 拖影）
            if cw == ww && ch == wh && dx <= m && dy <= m {
                // 只走一遍"睡 → BgCaptured(None) → 续链"，不做 GDI 回读
                return Task::perform(
                    async move {
                        std::thread::sleep(std::time::Duration::from_millis(200));
                        None
                    },
                    Message::BgCaptured,
                );
            }
        }

        // ── 桌面复制可用：60Hz 轮询，只在"真的有新画面"时才回来 ──────────
        //
        // ⚠️ **轮询必须发生在这个 future 的内部**。若改成"每 16ms 唤醒一次
        // 并给 update 发消息"，iced 每 16ms 就会重建一次 view ——
        // 那才是真正的 CPU 杀手（5ms × 60 = 30% 单核）。
        // 循环内部只做两件事：问一句"有新帧吗"（0.1ms）、没有就睡 16ms。
        //
        // 抓不到（桌面静止）就继续循环 ⇒ 一条消息都不发 ⇒ 界面不重绘。
        #[cfg(windows)]
        if crate::dupl::available() {
            return Task::perform(
                async move {
                    loop {
                        if crate::dupl::stopped() {
                            return None;
                        }
                        // 实测每次 0.4ms（桌面静止时 0 次），所以这里**不需要限速**：
                        // 桌面 37Hz 变化 ⇒ 37 × 0.4ms ≈ 1.5% 单核 ≈ 0.09% 总 CPU。
                        let got = crate::backdrop::capture_window_blurred(blur_factor());
                        // ⚠️ **内容没变就继续轮询，一条消息都不发**。
                        // 实测 DWM 每秒给 ~20 个"新帧"，但内容 100% 与上一帧相同；
                        // 每发一条消息 iced 就要重建一次 view ⇒ 界面 20Hz 空转。
                        // 去重放在这里（runtime 线程）而不是 update 里，
                        // 连消息都不产生 —— 见 [`crate::dupl::dedup`]。
                        if let Some(got) = got
                            && crate::dupl::dedup(&got.rgba)
                        {
                            return Some(got);
                        }
                        std::thread::sleep(std::time::Duration::from_millis(16));
                    }
                },
                Message::BgCaptured,
            );
        }

        // ⚠️ **间隔是自适应的**（用户要求 CPU < 5%）。
        //
        // 实测：一次抓屏 40–95ms，其中 99% 花在 GDI 的 `StretchBlt` 上
        //（从屏幕 DC 回读桌面）。而"桌面一动不动"是绝大多数时候的常态，
        // 那时高频重抓纯属浪费 —— 连续 2 次内容没变就放慢到 **2s**；
        // 有外部事件（`Moved` / `Resized` / 换质感，即 `bg_dirty`）就回到 500ms。
        let idle = self.bg_idle >= 2 && !self.bg_dirty;
        let wait_ms: u64 = if idle { 2000 } else { 500 };
        self.bg_dirty = false;

        // 抓屏 + 模糊是阻塞的（GDI 回读 + 两趟 box blur），
        // 丢进 Task 让 runtime 在线程池里跑，UI 线程不等。
        //
        // ⚠️ **睡觉也放进同一个 Task**：以前"抓"和"睡"是两个并行的 Task，
        // 等于每一轮要占两条线程池线程；合成一条后每轮只占一条。
        Task::perform(
            async move {
                let r = crate::backdrop::capture_window_blurred(blur_factor());
                std::thread::sleep(std::time::Duration::from_millis(wait_ms));
                r
            },
            Message::BgCaptured,
        )
    }

    /// 外部（窗口移动 / 尺寸变化 / 换质感 / 启动）请求"尽快抓一帧"。
    ///
    /// ⚠️ 链已经在跑时**什么都不做**，只置 `bg_dirty` —— 这一条守卫是整个
    /// "单链"设计成立的地方。
    fn bg_request(&mut self) -> Task<Message> {
        self.bg_dirty = true;
        if self.bg_running {
            return Task::none();
        }
        self.bg_cycle()
    }

    /// 键盘事件的总入口。
    ///
    /// 取代原来"把事件丢给 `text_editor`"的做法 —— 现在每一次按键的去向都明确：
    /// 插入字符 / 删除 / 移动光标 / 求值 / 粘贴。
    fn handle_key(
        &mut self,
        key: iced::keyboard::Key,
        modifiers: iced::keyboard::Modifiers,
    ) -> Task<Message> {
        use iced::keyboard::key::{Key as K, Named};

        // Ctrl 组合键。⚠️ 语义与改动前**保持一致**：
        //   * `Enter`        → **求值**（Windows 计算器 / Desmos 的直觉）
        //   * `Ctrl + Enter` → **换行**（想要多行表达式时用）
        // 第一版我在这里写反过（Ctrl+Enter 也去求值），被老测试挡下来了。
        if modifiers.control() {
            match &key {
                K::Named(Named::Enter) => self.insert("\n"),
                K::Character(c) if c.as_ref().eq_ignore_ascii_case("v") => {
                    return iced::clipboard::read().map(Message::Pasted);
                }
                _ => {}
            }
            return Task::none();
        }

        match key {
            K::Named(Named::Enter) => return self.evaluate(),
            K::Named(Named::Backspace) => self.backspace(),
            K::Named(Named::Delete) => self.delete_forward(),
            K::Named(Named::ArrowLeft) => self.move_caret(-1),
            K::Named(Named::ArrowRight) => self.move_caret(1),
            K::Named(Named::Home) => self.input.home(),
            K::Named(Named::End) => self.input.end(),
            // 可打印字符直接插入 —— `+ - * / ( ) ^` 与字母数字一视同仁
            K::Character(text) => {
                let clean: String = text.chars().filter(|c| !c.is_control()).collect();
                if !clean.is_empty() {
                    self.insert(&clean);
                }
            }
            _ => {}
        }
        Task::none()
    }

    /// 输入框内容的**源文本**（求值、历史回显、测试都用它）。
    ///
    /// 结构化之后这里是**序列化**：树 → MathIO 文本（`∑` → `sum(...)`、
    /// 分数 → `a/b`、根号 → `sqrt(...)`）。求值侧完全不知道输入框是棵树，
    /// 所以"显示与计算解耦"这条边界仍然只有一处。
    fn input_text(&self) -> String {
        self.input.source()
    }

    /// 把**显示符号**还原成求值器认识的函数名。
    ///
    /// 输入栏里 `∑` / `∏` 是"符号本身"（用户要求：不要用函数名，否则光标在
    /// `sum` 中间移动时会把它拆成 `sumx`，排版立刻崩成文本）。
    /// 但求值器只认 `sum(f,k,a,b)` / `prod(f,k,a,b)`，所以在这条边界上做一次映射 ——
    /// **显示与计算彻底解耦**。
    fn normalize_for_eval(text: &str) -> String {
        text.replace('∑', "sum").replace('∏', "prod")
    }

    /// 求值：把请求**交给后台 worker**，立刻返回一个等待结果的 task。
    ///
    /// ## 多行怎么处理
    ///
    /// Enter 现在是求值、Ctrl+Enter 才是换行，所以输入框里可能有多行。规则与
    /// **交互式命令行一致**：按顺序逐行求值，只把**最后一行**的结果推进历史。
    ///
    /// 为什么不是"整个多行串当一条表达式"：那等于把换行当隐式乘法，用户敲两行
    /// 分开的算式会得到一个奇怪的乘积。为什么不是"每行都进历史"：用户是在
    /// 一个输入框里敲的，敲 3 行却在历史里炸出 3 条会被当成 bug。
    ///
    /// 副作用是 `A=3` 这种赋值行**在两行之间也能生效**（第 1 行给变量赋值，
    /// 第 2 行用它算结果），这正是命令行用户期待的行为。
    ///
    /// ## 为什么这里**不**碰求值器
    ///
    /// 这是本轮修复的核心：以前这里直接 `self.session.eval(..)`，跑在 iced 的事件
    /// 循环线程上 —— `100^100^10` 会把它按在那里几十秒，Windows 判定窗口失去响应
    /// 并在标题栏写上「(未响应)」。
    ///
    /// 现在这里只做三件事：把行拆出来、把请求推给 worker、返回一个
    /// `Task::perform`。真正的求值（以及它的计时）发生在别的线程，
    /// 结果通过 [`Message::Evaluated`] 回来。整个 `update` 恒定在微秒级返回。
    fn evaluate(&mut self) -> Task<Message> {
        // 上一次还在算就先不受理：否则 worker 队列会积压，用户连按几次 =
        // 排着算好几次同一个式子（而且每次都要等前面的跑完）
        if self.busy {
            return Task::none();
        }
        // ⚠️ **没写完就别丢给解析器**（用户反馈："错误提示还是输出的 sum 函数，
        // 没有渲染结构，而且错误提示请你适配 GUI 界面，不要用 CLI 的那一套"）。
        //
        // `∑(,,,)` 交给 CLI 的解析器只会得到"位置 4 处意外的字符: ','" ——
        // 带字符偏移、带 ASCII 引号、还把 `sum` 这种内部名字漏出来。
        // 界面上正确的说法是"还有空位没填"，就一句人话，也不占历史。
        if self.input.has_empty_slot() {
            self.input_hint = Some(t("还有空位没填").to_string());
            return Task::none();
        }
        let text = self.input_text();
        let lines: Vec<String> = text
            .lines()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .into_iter()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        if lines.is_empty() {
            return Task::none();
        }
        self.evaluate_lines(lines)
    }

    /// 求值**已经准备好的**文本行（命令也可以，例如 `/let A = 3`）。
    ///
    /// 从 `evaluate` 里拆出来是为了让"自定义函数与变量"页能复用同一条通道：
    /// 页面里的 `A = 3` 需要当作 `/let` 命令走（否则会被当成方程去解）。
    fn evaluate_lines(&mut self, lines: Vec<String>) -> Task<Message> {
        // 历史里显示的原文 = 用户敲的全部内容（多行保留换行，便于回看）
        let src = lines.join("\n");
        self.last_input = src.clone();
        // ⚠️ **求值走另一份文本**：`∑` / `∏` 要换成 `sum` / `prod`（见 `normalize_for_eval`）。
        // 历史里仍显示用户敲的原文，两边不互相污染。
        let eval_src = Self::normalize_for_eval(&src);
        let eval_lines: Vec<String> = lines.iter().map(|l| Self::normalize_for_eval(l)).collect();
        // 求值后清空输入框，符合计算器的直觉（现在就清 —— 结果回来时用户可能
        // 已经在敲下一句了，那时再清会把新打的内容一起抹掉）
        self.clear_input();
        self.busy = true;
        self.busy_since = Some(std::time::Instant::now());

        // 记下这次请求（测试要能"补等一次"，见 `await_pending_for_test`）
        self.pending = Some((eval_src.clone(), eval_lines.clone()));

        // ⚠️ **只构造 future、不 poll 它**。`evaluator.eval` 内部只做两件事：
        // 把请求推给 worker、返回一条由 `oneshot` 唤醒的 future。
        //
        // 交给 `Task::perform` 之后由 iced 的执行器去跑（默认 feature 是
        // `thread-pool`，即 `futures::executor::ThreadPool`）。
        //
        // ⚠️ 曾经的写法是 `Task::perform(wait(), Message::Evaluated)` ——
        // 注意那个 `wait()` 的括号：`Task::perform` 收的是 **future 本身**，
        // 不是"产生 future 的闭包"，所以 `wait()` 在**这一行**就被执行了。
        // 而当时的 `wait` 内部是自旋等待循环 ⇒ `update` 原地等求值跑完
        // ⇒ 大输入依然「未响应」。现在 `eval` 返回的 future 不含任何等待代码，
        // 谁也无法在构造阶段把线程占住（`evaluator` 里有一条测试守着这一点）。
        let fut = self.evaluator.eval(eval_src, eval_lines);
        Task::perform(fut, Message::Evaluated)
    }
}

fn is_precise(text: &str) -> bool {
    text.starts_with("= ") || text.starts_with('=')
}

/// 表达式回显是否按数学排版。
///
/// **恒为真**（除非这行是错误）—— 原因见 `history_entry` 的注释：回显是用户
/// 自己敲的表达式，天生就是数学式，**不随显示模式（MathIO / LineIO）变化**。
/// 抽成函数是为了能单测这条约定（用户要求："根号应当始终渲染"）。
/// 把实测耗时格式化成展示文本（**精确到 0.1 秒**）。
///
/// 用户 2026-10-23："把计算完的用时精确到 0.1s" —— 所以从"只报整秒"改成一位小数。
///
/// 两条规则：
/// - **向下取整到 0.1 秒**（`1.99s` → `1.9秒`），**不四舍五入** ——
///   宁可少报也不要多报：`1.99` 说成 `2.0` 会让用户以为比实际更慢；
/// - **不足 0.1 秒显示 `<0.1秒`**，而不是 `0.0秒` —— 后者会让人以为"没计时"。
///
/// ⚠️ CLI 仍是**整秒**口径（`Timing::elapsed_text`）：CLI 每行都打一条，
/// 一位小数会把输出挤得很吵；两边刻意不同，不要"顺手统一"。
///
/// 文案过 `i18n`（`用时：{0}秒` / `用时：<0.1秒` 两条词条在词典里）。
/// 复制到剪贴板时用的文本：去掉界面上的 `= ` / `≈ ` 前缀与首尾空白。
///
/// 前缀是"精确/近似"的视觉记号（见 `history_entry` 里结果行的说明），
/// 粘到别处只会碍事 —— 用户复制的是**结果本身**。
fn copy_payload(output: &str) -> String {
    output
        .trim()
        .trim_start_matches("= ")
        .trim_start_matches("≈ ")
        .trim()
        .to_string()
}

fn elapsed_text(d: std::time::Duration) -> String {
    // 直接用整数运算算 0.1 秒的个数，避免浮点格式化带来的舍入惊喜
    let tenths = d.as_millis() / 100;
    if tenths == 0 {
        return t("用时：<0.1秒");
    }
    t_fmt(
        "用时：{0}秒",
        &[&format!("{}.{}", tenths / 10, tenths % 10)],
    )
}

/// 订阅：窗口事件（亚克力探测）+ 键盘修饰键状态（区分 Enter / Ctrl+Enter）。
///
/// 用 `window::events()` 而不是别的钩子 —— iced 0.14 没有"窗口就绪回调"，
/// 事件流是唯一能在**窗口已经存在**之后拿到控制权的办法。
///
/// 键盘那条 [`iced::keyboard::listen`] 是为了 `Ctrl` 的按下/松开：
/// `text_editor` 自己会把 `Enter` 与 `Ctrl+Enter` 都当成"插入换行"，
/// 拿不到修饰键信息（见 `Message::Edit` 分支的注释），
/// 所以只能另开一路订阅跟踪 `ModifiersChanged`。
/// **帧时间探针**（只有 `HIPERCALC_FPS=1` 时才装）。
///
/// ## 为什么不能每帧发一条消息
///
/// iced 0.14 的规则是"**任意一条消息 ⇒ 全量 view + layout + draw**"
///（`iced_winit/lib.rs` 里 `if !messages.is_empty() || uis_stale`）。
/// 所以探针如果每帧 `Message::Frame(..)`，就成了**自我驱动的重绘循环** ——
/// 测到的是"探针造出来的帧"，不是真实帧。
///
/// 这里用 `listen_raw`：**只把时间戳记进静态变量、永远返回 `None`**，
/// 于是订阅本身零消息、零重建。统计每 120 帧往 stderr 打一行。
mod fps {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::time::Instant;

    static ON: AtomicBool = AtomicBool::new(false);
    static STARTED: AtomicBool = AtomicBool::new(false);
    /// 上一次 RedrawRequested 的时刻（纳秒，单调时钟）
    static LAST: AtomicU64 = AtomicU64::new(0);
    /// 本轮已收集的帧间隔（纳秒）与序号
    static N: AtomicU64 = AtomicU64::new(0);
    static SUM: AtomicU64 = AtomicU64::new(0);
    static MAX: AtomicU64 = AtomicU64::new(0);
    /// `view()` 的累计耗时（纳秒）与次数
    static VIEW_NS: AtomicU64 = AtomicU64::new(0);
    static VIEW_N: AtomicU64 = AtomicU64::new(0);

    fn now_ns() -> u64 {
        // 单调时钟即可，不要求和 `Instant::now()` 同一个基准
        static BASE: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        BASE.get_or_init(Instant::now).elapsed().as_nanos() as u64
    }

    pub fn enabled() -> bool {
        if !STARTED.swap(true, Ordering::Relaxed) {
            ON.store(
                std::env::var("HIPERCALC_FPS").ok().as_deref() == Some("1"),
                Ordering::Relaxed,
            );
        }
        ON.load(Ordering::Relaxed)
    }

    /// 记一帧（由 `listen_raw` 调用，**不产生消息**）
    pub fn on_frame() {
        if !enabled() {
            return;
        }
        let t = now_ns();
        let last = LAST.swap(t, Ordering::Relaxed);
        if last == 0 {
            return;
        }
        let dt = t.saturating_sub(last);
        // 过滤"从空闲切到活动"的第一个大间隔（那不是掉帧，是没在画）
        if dt > 200_000_000 {
            return;
        }
        SUM.fetch_add(dt, Ordering::Relaxed);
        if dt > MAX.load(Ordering::Relaxed) {
            MAX.store(dt, Ordering::Relaxed);
        }
        let n = N.fetch_add(1, Ordering::Relaxed) + 1;
        if n.is_multiple_of(120) {
            let avg = SUM.load(Ordering::Relaxed) as f64 / n as f64;
            let max = MAX.load(Ordering::Relaxed) as f64;
            let vn = VIEW_N.load(Ordering::Relaxed).max(1);
            let vavg = VIEW_NS.load(Ordering::Relaxed) as f64 / vn as f64;
            eprintln!(
                "[fps] frames={n} 间隔 avg={:.2}ms max={:.2}ms  view() avg={:.2}ms",
                avg / 1e6,
                max / 1e6,
                vavg / 1e6
            );
        }
    }

    /// 记一次 `view()` 的耗时
    pub fn on_view(ns: u64) {
        if !enabled() {
            return;
        }
        VIEW_NS.fetch_add(ns, Ordering::Relaxed);
        VIEW_N.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn subscription(state: &State) -> iced::Subscription<Message> {
    let window_events = iced::window::events().map(|(_id, ev)| match ev {
        iced::window::Event::Opened { .. } => Message::WindowOpened,
        // ⚠️ **只有尺寸变化才要重抓背景**（抓屏区域是跟着窗口尺寸算的）。
        //
        // 单纯**移动**不重抓：着色器按"当前窗口原点 − 抓屏原点"平移采样，
        // 几何上完全等价（`backdrop::uv_map`），而且**零 CPU** ——
        // 用户实测"拖动窗口时背景一卡一卡，目测小于 1 帧"就是每 500ms
        // 重抓一次（GDI 回读 40–95ms）造成的。
        // `WindowMoved` 唯一的作用就是触发一次重绘，让新偏移在下一帧生效
        //（iced 收到任意消息都会重建 view + 重绘 ⇒ 刷新率跟着事件的 60Hz 走）。
        iced::window::Event::Moved(pos) => Message::WindowMoved(pos),
        iced::window::Event::Resized(size) => Message::WindowResized(size),
        // 其余窗口事件本项目暂时不关心，映射成一个不会被处理的占位
        _ => Message::Noop,
    });

    // 键盘：**一个订阅同时处理**修饰键与按键。
    //
    // ⚠️ iced 0.14 没有 `keyboard::on_key_press`（那是别的版本的 API），
    // 只有 `keyboard::listen()` 这个"原始事件流"。拆成两个订阅也能跑，
    // 但每个事件会被广播两遍，不如合并成一处。
    // 几何"末尾补写"：只有在有待写内容时才启动定时器（否则纯属空转）
    let geom_save = if state.geom_save_pending {
        iced::time::every(std::time::Duration::from_millis(700)).map(|_| Message::SaveGeomNow)
    } else {
        iced::Subscription::none()
    };

    let ctrl = iced::keyboard::listen().map(|ev| match ev {
        iced::keyboard::Event::ModifiersChanged(m) => Message::CtrlChanged(m.control()),
        // 按键：自己处理（不再交给 `text_editor`）
        //
        // ⚠️ **不能用 `key`**：iced 传的是 `key_without_modifiers()`
        // （见 `iced_winit/src/conversion.rs` 里 `event.key_without_modifiers()`），
        // 所以 `Shift` + `=` 到这里是 `=`、`Shift` + `9` 是 `9` —— 用户实测
        // "无法打出需要 shift 的字符"，根因就在这里。
        //
        // 优先用 `text`（= `text_with_all_modifiers()`，已经把 Shift、键盘布局、
        // 输入法、死键都算进去了）；没有文本（Enter / Backspace / 方向键…）才退回
        // `modified_key`（= `logical_key`，同样带修饰键）。
        iced::keyboard::Event::KeyPressed {
            modified_key,
            text,
            modifiers,
            ..
        } => {
            let typed = text
                .as_deref()
                .filter(|t| !t.is_empty() && !t.chars().any(char::is_control));
            match typed {
                Some(t) => Message::KeyPressed(iced::keyboard::Key::Character(t.into()), modifiers),
                None => Message::KeyPressed(modified_key, modifiers),
            }
        }
        _ => Message::Noop,
    });

    // 帧时间探针：**永远返回 None**，所以不会制造任何重绘
    let fps = if fps::enabled() {
        iced::event::listen_raw(|event, _status, _window| {
            if let iced::Event::Window(iced::window::Event::RedrawRequested(_)) = event {
                fps::on_frame();
            }
            None
        })
    } else {
        iced::Subscription::none()
    };

    // 秒表：**只在计算中订阅**（空闲时一条都不发，不会造成空转重绘）。
    // 100ms 一跳 ⇒ "计算中 3.2s" 的小数位看着是连续在走的。
    let tick = if state.busy {
        iced::time::every(std::time::Duration::from_millis(100)).map(|_| Message::Tick)
    } else {
        iced::Subscription::none()
    };

    iced::Subscription::batch([window_events, ctrl, fps, tick, geom_save])
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
        // 文本 → 树（走与"粘贴"同一条解析路径），光标放到末尾
        self.input = expr::Doc::from_text(text);
    }

    /// **同步求值**（测试用）。
    ///
    /// 产品路径是异步的（[`State::evaluate`] 返回 `Task`，结果稍后由
    /// [`Message::Evaluated`] 送回来）。但集成测试（`tests/` 目录，尤其是快照）
    /// 里没有 iced 的事件循环 —— `Task` 交出去就没人跑，历史永远空着。
    ///
    /// 所以这里提供一条"等结果"的**同步**入口：输入 → 阻塞取结果 → 灌进历史。
    /// 走的是**同一个 worker、同一条通道**（[`Evaluator::eval_blocking`]），
    /// 所以快照画的确实是从真实求值路径来的内容，不是造出来的假数据。
    ///
    /// ⚠️ 唯一的差别是耗时会显示成真实值（测试里通常是 `<1秒`），
    /// 这让快照的右边栏也顺带获得了视觉覆盖。
    pub fn evaluate_blocking_for_test(&mut self) {
        let text = self.input_text();
        let lines: Vec<String> = text
            .lines()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .into_iter()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        if lines.is_empty() {
            return;
        }
        let src = lines.join("\n");
        self.last_input = src.clone();
        self.clear_input();
        let out = self.evaluator.eval_blocking(src, lines);
        let _ = self.update(Message::Evaluated(out));
    }

    /// 等**在飞的那一次**求值回来，并按正常路径写进历史（测试用）。
    ///
    /// 单测没法跑 iced 的 `Task`（`update(Message::Evaluate)` 只**构造**一条 future，
    /// 没人去 poll 它），所以那条路径上历史永远是空的。这个方法给出"补等一次"的能力：
    /// 把刚记下的请求**重放一次**（走 [`Evaluator::eval_blocking`]，同一个 worker、
    /// 同一条队列），再经 [`Message::Evaluated`] 写进历史 —— 落库路径与产品完全一致。
    ///
    /// ⚠️ 代价是同一个请求会被算两遍（第一遍的结果没人取）。
    /// 测试里用的都是毫秒级输入，所以这点浪费可以忽略；换来的是"等待逻辑
    /// 不可能再把测试线程挂住"（早期版本靠自旋读结果信箱，一旦 worker 出问题
    /// 就会把测试也一起卡死，只能靠超时杀进程）。
    ///
    /// 没有在飞的请求时什么也不做（幂等），可以放心多调。
    pub fn await_pending_for_test(&mut self) {
        let Some((src, lines)) = self.pending.take() else {
            return;
        };
        let out = self.evaluator.eval_blocking(src, lines);
        let _ = self.update(Message::Evaluated(out));
    }

    /// 设置"视口尺寸"（测试用：模拟窗口被拉宽/收窄）
    pub fn set_viewport_for_test(&mut self, w: f32, h: f32) {
        self.viewport = (w, h);
    }

    /// 设置面板一行的水平预算（测试用：验证它随窗口宽度变化）
    pub fn settings_row_budget_for_test(&self) -> f32 {
        settings::row_budget(self.viewport.0)
    }

    /// 渲染视图（测试用）
    pub fn view_for_test(&self) -> Element<'_, Message> {
        self.view()
    }

    /// 当前主题（测试用）
    pub fn theme_for_test(&self) -> iced::Theme {
        theme::theme(self)
    }

    /// 设置"窗口是否最大化"（测试用：让标题栏画成"还原"图标）
    pub fn set_maximized_for_test(&mut self, maximized: bool) {
        self.maximized = maximized;
    }

    /// 设置明暗（测试用）
    pub fn set_dark_for_test(&mut self, dark: bool) {
        self.dark = dark;
        theme::set_dark(dark);
        theme::set_font_scale(self.font_scale);
    }

    /// 设置显示模式（测试用）
    ///
    /// 走**完整路径**（UI 副本 + 发给 worker 的命令），而不是只改 `self.modes` ——
    /// 否则测试会通过、真实界面却还是按旧模式算（这正是要防的失真）。
    pub fn set_display_mode_for_test(&mut self, m: DisplayMode) {
        self.modes.display = m;
        self.evaluator.config(Command::SetDisplay(m));
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
mod tests;
