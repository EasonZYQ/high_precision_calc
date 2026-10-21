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

use iced::widget::{
    Column, Row, Space, Stack, button, column, container, row, rule, scrollable, slider, stack,
    text, text_input,
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
use crate::mathrender;
use crate::theme::{self, MathFont};

/// **键面上那些"不是数学记号"的 UI 图标**：数学字体里没有它们的字形，
/// 硬套会渲染成豆腐块（`⌫` 实测如此）。这些键保持默认字体。
const UI_ICON_KEYS: &[&str] = &["⌫"];

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
    /// 这一条算完花了多久（worker 侧的实测值，见 [`crate::evaluator`]）。
    ///
    /// 界面把它格式化成 `用时：N秒` 贴在结果行**最右侧**（用户要求）。
    /// 存 `Duration` 而不是格式化后的字符串：语言可能在之后被切换，
    /// 而"用时"这两个字是**界面文案**、要跟着语言走 ——
    /// 所以格式化推迟到 `history_entry` 里做。
    elapsed: std::time::Duration,
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
    2f32.powf(4.0 * a).round().clamp(1.0, 16.0) as u32
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
pub fn window_title(_state: &State) -> String {
    t("HiPerCalc — 超高精度计算器")
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
    /// 点了排版层里的**空参数框**：把光标挪到源文本的第 n 个字符处
    PlaceCaret(usize),
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
    /// 窗口**被移动**了（只要求重绘，不要求重抓 —— 背景靠 uv 偏移跟手，
    /// 见 [`State::bg_cycle`] 里"拖动中跳过抓屏"的说明）。
    WindowMoved,
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
        theme::set_glass(prefs.glass);
        theme::set_glass_tint(prefs.glass_tint);
        theme::set_texture(theme::Texture::from_key(&prefs.texture));
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
            dark: prefs.dark,
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
            last_input: String::new(),
            panel: Panel::default(),
            ctrl_held: false,
            busy: false,
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
            Message::PlaceCaret(at) => self.place_caret(at),
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
                self.pending = None;
                let precise = is_precise(&out.text);
                self.history.push(Entry {
                    input: out.input,
                    output: out.text,
                    error: out.is_error,
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
                    return Task::none();
                }
                // ⚠️ **链已经在跑就只打个标记，绝不再起第二条**。
                //
                // 这是"延迟越来越高、且永远不恢复"的根因（详见 [`State::bg_running`]
                // 的注释）：实测启动即有 3 条链并发抓屏，拖一下窗口就是几十条。
                return self.bg_request();
            }
            // 链的内部续期：无条件再跑一轮（唯一的循环入口）
            Message::BgTick => return self.bg_cycle(),
            // 窗口移动：只重绘（view 里现算 uv 偏移），不重抓屏。
            //
            // 但**要留下"还在动"的痕迹**：`bg_moved` 让 `bg_cycle` 知道本轮可以
            // 跳过抓屏；`bg_dirty` 让"停下来之后的那一轮"立刻补抓一帧
            //（否则要等 `bg_idle` 把间隔放慢到 2s，拖完会有明显的一段旧背景）。
            Message::WindowMoved => {
                self.bg_moved = true;
                self.bg_dirty = true;
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
                // ⚠️ **这里刻意什么都不做**。
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
    fn place_caret(&mut self, id: usize) {
        self.input.place(id);
    }

    /// 清空输入
    fn clear_input(&mut self) {
        self.input.clear();
        self.input_hint = None;
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

    /// 视图
    pub fn view(&self) -> Element<'_, Message> {
        // 把最新背景同步给玻璃着色器（`liquid` 的管线在版本号变化时才重传
        // 纹理）。这里每帧调用是刻意的：质感切到"关闭"时 `bg_pixels` 会被
        // update 清成 None，只有从 view 这条每帧必经的路同步，着色器才能
        // 可靠地知道"现在没有背景可采样"。
        //
        // 同时算出**窗口→背景图**的 uv 映射：窗口挪动时原点差跟着变，
        // 着色器按它平移采样 —— 拖动全程零抓屏、零 CPU（用户要求 60fps）。
        //
        // ⚠️ `uv_map` 收的是**抓屏区域的物理尺寸**（元组后两位），
        // 纹理像素尺寸（中间两位）只喂 `write_texture` —— 混用会整体错位
        //（change_log81 的根因，见 [`BgPixels`] 的文档）。
        let (bw, bh) = self
            .bg_pixels
            .as_ref()
            .map_or((0, 0), |(_, _, _, w, h, _, _)| (*w, *h));
        let bg_map = self
            .bg_pixels
            .as_ref()
            .and_then(|(_, x, y, _, _, rw, rh)| crate::backdrop::uv_map((*x, *y, *rw, *rh)))
            .unwrap_or(([1.0, 1.0], [0.0, 0.0]));
        liquid::sync_backdrop(
            self.bg_pixels.as_ref().map(|(p, ..)| p),
            bw,
            bh,
            self.bg_version,
            bg_map,
        );

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
                        // ⚠️ **全透明**（用户反馈："打开函数面板会变暗"）。
                        //
                        // 之前这里是一层压暗遮罩（先是写死的 `BLACK 0.42`，后来改成
                        // 随主题的 `scrim`）。但函数面板几乎占满窗口，遮罩除了把
                        // 露出的一圈压暗之外没有任何作用 —— 而它唯一的**功能**是
                        // "点空白处关闭"，那不需要可见。
                        //
                        // 所以底色直接给全透明：点击照样能收到，界面一点不变暗。
                        background: Some(Background::Color(Color::TRANSPARENT)),
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

            // ⚠️ **必须把 `body` 垫在最底下**（用户："我的意思不是遮住整个界面，
            // 是设置这个小框在最顶层，不是全部遮住啊"）。
            //
            // 上一版写的是 `stack![backdrop, panel]` —— 浮层**替代**了 `body`，
            // 于是打开设置时工具条/历史/输入区/键盘**全都不画了**，只剩一块空白，
            // 看起来就是把整个界面盖住。我当时只改了高度，没发现根因在这里。
            stack![body, backdrop, panel].into()
        } else if self.show_settings {
            // ── 设置浮层（用户要求）────────────────────────────────────────
            //
            // 两条要求：
            // 1. **盖在历史计算之上**，而不是把历史推下去 —— 推进去会让历史区被压得
            //    很矮（旧快照里最后一条的分母被滚出可视区，看起来像排版坏了）；
            // 2. **点面板外的任意位置就关闭**（不必再点一次齿轮）。
            //
            // 做法与函数弹窗同一套：`stack!` + 一层接收点击的遮罩。
            let backdrop = iced::widget::mouse_area(
                container(Space::new().width(Length::Fill).height(Length::Fill)).style(|_t| {
                    container::Style {
                        // ⚠️ **全透明**（用户要求："不要遮盖全部界面，显示在哪就遮哪里"）。
                        //
                        // 这一层的职责只有一个：接住"点面板外任意处"这个点击，
                        // 让设置关掉。界面本身一点不该被压暗 —— 它显示在历史区那块，
                        // 视觉上也就该只影响那块。
                        background: Some(Background::Color(Color::TRANSPARENT)),
                        ..container::Style::default()
                    }
                }),
            )
            .on_press(Message::CloseSettings);

            // ⚠️ **高度必须是 Shrink**（用户反馈："我的意思不是遮住整个界面，
            // 是设置这个小框在最顶层，不是全部遮住啊"）。
            // 上一版写的是 `height(Length::Fill)`，于是 `settings_panel()` 那层
            // **带背景色**的容器被拉伸到整屏高 —— 面板下面拖出一大片同色空白，
            // 看起来就是把整个界面盖住了。
            let panel = container(self.settings_panel())
                .width(Length::Fill)
                .height(Length::Shrink)
                .padding([56.0, 22.0]);

            // ⚠️ **必须把 `body` 垫在最底下**（用户："我的意思不是遮住整个界面，
            // 是设置这个小框在最顶层，不是全部遮住啊"）。
            //
            // 上一版写的是 `stack![backdrop, panel]` —— 浮层**替代**了 `body`，
            // 于是打开设置时工具条/历史/输入区/键盘**全都不画了**，只剩一块空白，
            // 看起来就是把整个界面盖住。我当时只改了高度，没发现根因在这里。
            stack![body, backdrop, panel].into()
        } else {
            // ⚠️ **单独包一层 `stack!`，哪怕只有一个孩子**。
            //
            // 用户实测："历史计算较多时，打开设置就会拉到最早的历史"。
            // 根因：两个浮层分支返回的是 `stack![body, ..]`，而这里直接返回 `body`
            // —— 于是 `body` 在 widget 树里的**位置不同**（一个在 stack 的
            // `children[0]`，一个就是根），iced 按位置匹配状态 ⇒ 滚动位置被判为
            // "新容器"而重置到顶部。
            //
            // 统一成"body 永远是 stack 的第 0 个孩子"，状态就能跨开合保留下来。
            stack![body].into()
        };

        // 根容器负责"透出桌面"：亚克力成功时背景全透，失败时不透明。
        // 这一步**必须有** —— 窗口开了 transparent，若根容器不给背景，
        // 没成功启用亚克力时就会露出一片黑/花屏。
        // 最底层：**自绘模糊**的背景图（抓屏 + 模糊，见 `crate::backdrop`）。
        // 它必须垫在 `content` **之下** —— 上面的面板是半透明的，
        // 透过它们看到的就是这张图。抓不到时退回氛围底色。
        // ⚠️ **玻璃关掉就不叠这层**（用户实测："关闭后并没有完全消失，
        // 而是还有残留的透明"）。否则背景图会一直透出来，面板再实也挡不住。
        // ⚠️ 只看 `bg_pixels`，**不再要求 `self.bg` 一定存在**：
        // 玻璃模式走 shader，根本用不到 `image::Handle`（见 `BgCaptured`），
        // 那里刻意把它留成 `None` 来省掉一次图集上传。
        let layered: Element<'_, Message> = match &self.bg_pixels {
            Some((px, _cx, _cy, w, h, _, _)) if theme::texture() != theme::Texture::Off => {
                // 两种质感看到的是**同一张**模糊背景，但画法不同：
                // - 亚克力：普通贴图（磨砂玻璃，不做形变）
                // - 玻璃：走**折射 shader** —— 边缘被"压弯"，像一块凸透镜
                // 扭曲强度：玻璃要"边缘被压弯"（0.85），亚克力是磨砂、**不做形变**（0.0）。
                let strength = if theme::texture() == theme::Texture::Glass {
                    0.85
                } else {
                    0.0
                };
                // 窗口→背景图的 uv 映射（拖动时随原点差变化，零抓屏跟手）
                let layer: Element<'_, Message> =
                    iced::widget::shader(crate::refraction::Refraction {
                        pixels: Arc::clone(px),
                        width: *w,
                        height: *h,
                        strength,
                        version: self.bg_version,
                        uv_scale: bg_map.0,
                        uv_off: bg_map.1,
                    })
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into();
                stack![layer, content].into()
            }
            _ => content,
        };

        container(layered)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::root)
            .into()
    }

    // ── ① 工具条 ────────────────────────────────────────────────────────────

    /// 给一块**面板内容**垫上液态玻璃（只在玻璃质感下；其余模式原样返回）。
    ///
    /// ⚠️ **玻璃必须垫在 container 外面**（而不是塞进 container 里面）：
    /// 面板的 padding 是 container 的，垫在里面就只能盖住 padding 以内的区域，
    /// 面板最外圈会露出一条没盖到的直角 —— 而 **折射的精华全在最外 30px 那一圈**，
    /// 盖不到就等于没做（用户要求："做好四周的折射"）。
    ///
    /// 垫在外面时要 `push_under`：让 container（base）决定整块的尺寸，
    /// 玻璃是 `Fill`，绘制顺序在 container **下面** ——
    /// 于是观感是"折射过的背景 → 面板底色 → 面板内容"。
    fn glass_under<'a>(content: Element<'a, Message>, g: liquid::Glass) -> Element<'a, Message> {
        if theme::liquid_on() {
            Stack::new()
                .push(content)
                .push_under(liquid::layer(g))
                .into()
        } else {
            content
        }
    }

    fn toolbar(&self) -> Element<'_, Message> {
        // ⚠️ 从 UI 侧副本读，而不是问 `Session` —— 求值器在别的线程上（见 `State::modes`）
        let disp = self.modes.display;
        let ang = self.modes.angle;
        let deep = self.modes.calc == CalcMode::Deep;

        // 工具条上的文字都是"拉丁/数学"标签（MathIO、RAD…），走数学字体；
        // 中文标签（快速/死算/清空）走等线。`theme::label` 自动按内容挑。
        //
        // ⚠️ **padding 必须给内容、不给 button**：
        // 玻璃层是 `.width/height(Fill)`，它撑的是 **button 的内容盒** ——
        // 而内容盒 = button 边界减去 padding。把 padding 留在 button 上，
        // 玻璃就会比按键窄一圈（四周露出没盖到的直角）。
        // 所以 button 自己 `.padding(0)`，padding 由里面的 `container` 给。
        // ⚠️ **下面这个必须是 `fn`，不能写成闭包**：闭包参数的生命周期会被推断成
        // **不变的**，而 `Button<'_, Message>` 对生命周期也是不变的 ——
        // 于是"传入 `Element<'_>` → 包进 `button(..)` → 返回"会报
        // `lifetime may not live long enough`。写成 `fn` 就能显式标出 `'a`。
        //
        // ⚠️ **padding 在这里给，不给 button**：液态玻璃是 `.width/height(Fill)` 的
        // 着色器层，在 `Stack` 里它必须排在**文字之后**才能由文字决定尺寸（见下）。
        // 而文字外还得留一圈内边距 —— 所以 padding 包在文字外面、玻璃垫在整块下面，
        // 玻璃才盖得到 chip 的全部面积（含内边距）。
        fn chip<'a>(content: Element<'a, Message>, on: bool, msg: Message) -> Element<'a, Message> {
            let padded: Element<'a, Message> = container(content).padding([5, 12]).into();
            // ⚠️ **玻璃层必须 `push_under`，不能写成 `stack![glass, 文字]`**：
            // `Stack` 的 `base_layer` 默认是 **0**，尺寸取自**第一个**子元素 ——
            // 而玻璃是 `Fill`，放进第 0 位就等于"把 chip 撑满可用空间"，
            // 实际效果是工具条上的选项被拉成 180px 高的大板子（第一版快照的惨状）。
            // `push_under` 把玻璃插到第 0 位并把 `base_layer` 加 1 ⇒
            // 尺寸由文字决定、玻璃铺满文字那一圈、绘制顺序在文字**下面**。
            let body: Element<'a, Message> = if theme::liquid_on() {
                Stack::new()
                    .push(padded)
                    .push_under(liquid::layer(theme::chip_glass(
                        on,
                        theme::CHIP_RADIUS_GLASS,
                    )))
                    .into()
            } else {
                padded
            };
            button(body)
                .padding(0)
                .style(move |t, s| theme::mode_chip(t, s, on))
                .on_press(msg)
                .into()
        }
        // `seg` 收 `&'static str`（字面量，无需翻译的那些：MathIO / RAD / DEG）；
        // `seg_t` 收**已翻译的 `String`** —— 界面里会跟随语言变化的那几个标签用它。
        let seg = |label: &'static str, on: bool, msg: Message| {
            chip(theme::label(label).size(14).into(), on, msg)
        };
        let seg_t = |label: String, on: bool, msg: Message| {
            chip(theme::label(&label).size(14).into(), on, msg)
        };

        // 图标按钮：固定最小宽度，否则 `☾` / `⚙` 这类单字符按钮会缩成很小的方块，
        // 点在它旁边一点就落空（第一版快照里它们明显偏小偏挤）
        let icon = |label: &'static str, on: bool, msg: Message| {
            chip(
                text(label).size(15).font(theme::font_math()).into(),
                on,
                msg,
            )
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
        // 设置 + 清空。
        //
        // ⚠️ 明暗切换按钮**不在这里**了 —— 用户要求"深色浅色的按钮放到设置里"。
        // 主题属于"偶尔改一次"的设置，和 MathIO/RAD/快速 这些"算之前随手切"的
        // 模式不是一类东西，混在工具条上会让一排 chip 越长越难扫。
        r = r.push(icon("⚙", self.show_settings, Message::ToggleSettings));
        // 清空历史：这条**必须**翻译（工具条上最显眼的中文之一）。
        // 注意键用的是"清空历史"而不是"清空" —— 后者在 CLI 里可能被当成别的片段。
        r = r.push(seg_t(t("清空历史"), false, Message::ClearHistory));

        Self::glass_under(
            container(r)
                .padding([8, 12])
                .width(Length::Fill)
                .style(theme::panel)
                .into(),
            theme::panel_glass(),
        )
    }

    // ── ② 结果区 ────────────────────────────────────────────────────────────

    fn results(&self) -> Element<'_, Message> {
        let math = self.modes.display == DisplayMode::MathIO;

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

        // 「计算中…」提示（用户要求："让程序始终响应"）。
        //
        // 它不只是装饰：大运算要跑几十秒，用户需要**看得见**"还在算"，否则会以为
        // 程序又卡死了 —— 而那正是本次要修掉的观感。放在历史最下面（也就是
        // 视线落点，因为 `anchor_bottom` 会把视图钉在底部）。
        if self.busy {
            col = col.push(
                container(theme::label(t("计算中…")).size(13).style(theme::hint))
                    .width(Length::Fill)
                    .padding([4, 9]),
            );
        }

        // `anchor_bottom`：历史是"越往下越新"，滚到底部才符合计算器的直觉。
        // 内容超出可视区时首条会被滚出视野 —— 这是**正常行为**（不是裁切 bug），
        // 加内边距只是为了首条不与面板边框贴死。
        //
        // 滚动方向跟着"长结果"设置走（用户要求："增加左右拖动或者换行，供用户选择"）：
        // - 自动换行：只开垂直滚动。这一档要求内容折行，而折行由 `mathrender`
        //   的 `Wrapping::Glyph` 负责 —— 光开垂直滚动是不够的（`Wrapping::Word`
        //   不会在 100 位整数中间断行，照样横向溢出）。
        // - 横向滚动：两个方向都开。内容的宽度约束变成"无限"，于是不会折行，
        //   超出的部分靠拖动看。
        let direction = if self.wrap {
            scrollable::Direction::Vertical(scrollable::Scrollbar::default())
        } else {
            scrollable::Direction::Both {
                vertical: scrollable::Scrollbar::default(),
                horizontal: scrollable::Scrollbar::default(),
            }
        };
        let inner = scrollable(col.padding([10, 10]))
            .id(HISTORY_ID)
            .anchor_bottom()
            .height(Length::Fill)
            .direction(direction)
            // 记住"用户是不是在底部"（见 `State::at_bottom`）
            .on_scroll(Message::HistoryScrolled);

        Self::glass_under(
            container(inner)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(6)
                .style(theme::panel_card)
                .into(),
            theme::panel_glass(),
        )
    }

    /// 一条历史：上面是表达式回显，下面是（可排版的）结果
    fn history_entry<'a>(&'a self, e: &'a Entry, math: bool) -> Element<'a, Message> {
        // 回显也走 `mathrender`：用户敲的 `sqrt(2)/3` 在回显里同样应该显示成
        // 根号 + 分数，而不是一行原始 ASCII —— 与输入框里看到的样子保持一致。
        //
        // ⚠️ **回显永远按数学排版，与显示模式无关**（用户要求："根号应当始终渲染"）。
        // 曾经的 bug：这里传的是 `math`，于是切到 LineIO 后回显也退化成纯文本，
        // `sqrt(2)/3` 原样显示成 ASCII —— 但**回显是用户自己敲的表达式**，
        // 它天然就是数学式，跟"结果用十进制还是分数显示"是两码事。
        // 显示模式只该影响**结果**的呈现（MathIO 排版 / LineIO 小数）。
        //
        // ⚠️ **错误行也要排版**：回显的是**用户敲的表达式**，它天然是数学式。
        // 以前错误行退回纯文本，于是 `∑` 结构的输入在历史里显示成
        // `sum(k^2,k,1,)`（用户实测："错误提示还是输出的 sum 函数，没有渲染结构"）。
        // 报错文案本身仍然是散文（见下面 `result` 那一段的说明），两者互不影响。
        let echo_text: Element<'a, Message> = mathrender::render(
            &e.input,
            true,
            mathrender::Style2 {
                size: 15.0,
                color: Some(theme::colors().text_dim),
                wrap: self.wrap,
            },
            // 历史是只读的：占位框不可点
            None,
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
                // ⚠️ **统一用数学字体**（用户要求：LineIO 与 MathIO 字体要一致）。
                // 之前 LineIO 走等宽字体（`!math`），观感上与 MathIO 判若两套；
                // 等宽的好处只是"小数位对齐"，代价是整块结果区看起来像另一款软件。
                wrap: self.wrap,
            },
            // 历史是只读的：结果里的占位框不可点
            None,
        );

        // ── 耗时贴结果行**最右侧**（用户要求："计时放在运算结果的最右侧"）──
        //
        // 用 `Space(Length::Fill)` 把结果推到左边、耗时压到右边 ——
        // 这是 iced 里唯一的"两端对齐"写法（没有 `justify_content: space-between`）。
        //
        // ⚠️ 结果元素必须能伸能缩：`mathrender` 的根是 `Row`，宽度默认
        // `Length::Shrink`，所以它只占自己需要的宽度，`Space` 吃掉剩下的全部余量，
        // 耗时自然落到最右端。若结果被撑成 `Fill`，`Space` 就没有余量可分，
        // 耗时会紧贴在结果后面而不是靠右。
        let timing = elapsed_text(e.elapsed);
        let result_line = row![
            result,
            Space::new().width(Length::Fill).height(Length::Shrink),
            theme::label(timing).size(12).style(theme::hint),
        ]
        .align_y(Alignment::Center);

        container(column![echo, result_line].spacing(5))
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
            theme::label(t_fmt("当前 {0} 位", &[&self.modes.precision.to_string()]))
                .size(13)
                .style(theme::hint),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        // ② 语言（用户要求："设置里添加语言选择"）。
        // 语言名用**各语言自己的写法**（"简体中文" / "繁體中文" / "English"），
        // 而不是翻译成当前语言 —— 语言选单里这样才是惯例：看不懂当前语言的人
        // 也能找到自己的那一项。
        let cur_lang = self.modes.lang;
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

        // ④ 主题（用户要求："深色浅色的按钮放到设置里" —— 原来在工具条上）。
        let theme_row = row![
            self.setting_label(t("主题"), SETTING_LABEL_W),
            self.chip(t("深色"), self.dark, Message::ToggleDark),
            self.chip(t("浅色"), !self.dark, Message::ToggleDark),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        // ⑤ 质感（**三选一，互斥**）—— 用户规格：
        //   第一行 亚克力：背景模糊 + 半透明面板（原来那套）
        //   第二行 玻璃：iOS liquid glass（折射/高光是后续，先复用同一套背景模糊）
        //   第三行 关闭：界面完全不透明，连抓屏都停掉
        // 每个模式有**自己**的「模糊强度」与「底色浓度」，互不影响。
        let tex = self.texture;
        let pct = |v: f32| t_fmt("{0}%", &[&(v * 100.0).round().to_string()]);

        let tex_acrylic_row = row![
            self.setting_label(t("亚克力质感"), SETTING_LABEL_W),
            // 只有**选中**的那一行写"已开启"，其余写"未启用" ——
            // 否则三行都写"已开启"（只靠底色区分），一眼看去像三个都开着。
            self.chip(
                if tex == theme::Texture::Acrylic {
                    t("已开启")
                } else {
                    t("未启用")
                },
                tex == theme::Texture::Acrylic,
                Message::SetTexture(theme::Texture::Acrylic),
            ),
            theme::label(t("模糊强度")).size(12).style(theme::hint),
            slider(0.0..=1.0, self.acrylic_blur, Message::SetAcrylicBlur)
                .step(0.01_f32)
                .on_release(Message::CommitGlass)
                .width(Length::Fixed(104.0)),
            theme::label(pct(self.acrylic_blur))
                .size(13)
                .style(theme::hint),
            Space::new().width(Pixels(8.0)),
            theme::label(t("底色浓度")).size(12).style(theme::hint),
            slider(0.0..=1.0, self.acrylic_tint, Message::SetAcrylicTint)
                .step(0.01_f32)
                .on_release(Message::CommitGlass)
                .width(Length::Fixed(104.0)),
            theme::label(pct(self.acrylic_tint))
                .size(13)
                .style(theme::hint),
            Space::new().width(Pixels(8.0)),
            // 设置页自己的浓度（用户要求：两种质感各配一个）
            theme::label(t("设置页浓度")).size(12).style(theme::hint),
            slider(
                0.0..=1.0,
                self.acrylic_dialog_tint,
                Message::SetAcrylicDialogTint
            )
            .step(0.01_f32)
            .on_release(Message::CommitGlass)
            .width(Length::Fixed(96.0)),
            theme::label(pct(self.acrylic_dialog_tint))
                .size(13)
                .style(theme::hint),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        let tex_glass_row = row![
            self.setting_label(t("玻璃质感"), SETTING_LABEL_W),
            // 只有**选中**的那一行写"已开启"，其余写"未启用" ——
            // 否则三行都写"已开启"（只靠底色区分），一眼看去像三个都开着。
            self.chip(
                if tex == theme::Texture::Glass {
                    t("已开启")
                } else {
                    t("未启用")
                },
                tex == theme::Texture::Glass,
                Message::SetTexture(theme::Texture::Glass),
            ),
            theme::label(t("模糊强度")).size(12).style(theme::hint),
            slider(0.0..=1.0, self.glass_blur, Message::SetGlassBlur)
                .step(0.01_f32)
                .on_release(Message::CommitGlass)
                .width(Length::Fixed(104.0)),
            theme::label(pct(self.glass_blur))
                .size(13)
                .style(theme::hint),
            Space::new().width(Pixels(8.0)),
            theme::label(t("底色浓度")).size(12).style(theme::hint),
            slider(0.0..=1.0, self.glass_tint2, Message::SetGlassTint2)
                .step(0.01_f32)
                .on_release(Message::CommitGlass)
                .width(Length::Fixed(104.0)),
            theme::label(pct(self.glass_tint2))
                .size(13)
                .style(theme::hint),
            Space::new().width(Pixels(8.0)),
            theme::label(t("设置页浓度")).size(12).style(theme::hint),
            slider(
                0.0..=1.0,
                self.glass_dialog_tint,
                Message::SetGlassDialogTint
            )
            .step(0.01_f32)
            .on_release(Message::CommitGlass)
            .width(Length::Fixed(96.0)),
            theme::label(pct(self.glass_dialog_tint))
                .size(13)
                .style(theme::hint),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        let tex_off_row = row![
            self.setting_label(t("关闭"), SETTING_LABEL_W),
            self.chip(
                t("已关闭"),
                tex == theme::Texture::Off,
                Message::SetTexture(theme::Texture::Off),
            ),
            theme::label(t("界面完全不透明：不抓屏、也不模糊"))
                .size(12)
                .style(theme::hint),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        // 三行作为**一组**：内部不再插分隔线（它们是一个三选一的整体）
        let texture_block = column![tex_acrylic_row, tex_glass_row, tex_off_row].spacing(6);

        // ⑥ 面板投影（用户要求："阴影的透明调节加到设置里"）。
        // 单独占一行：玻璃那行已经有两根滑条，再塞一根就太挤了。
        let shadow_row = row![
            self.setting_label(t("面板投影"), SETTING_LABEL_W),
            slider(0.0..=1.0, self.shadow, Message::SetShadow)
                .step(0.01_f32)
                .on_release(Message::CommitGlass)
                .width(Length::Fixed(104.0)),
            theme::label(pct(self.shadow)).size(13).style(theme::hint),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        // ⑦ 桌面模糊状态（**只读**）。
        //
        // ⚠️ 结果必须显示在界面上：探测报告原本只 eprintln 到控制台，而用户是按
        // `cargo run` 跑、不会盯着控制台 —— "到底生效没有"就只能靠猜。
        // 这类"环境相关的能力"就该显示出来，而不是藏在没人看的 stderr 里。
        let acrylic_row = row![
            self.setting_label(t("桌面模糊"), SETTING_LABEL_W),
            theme::label(if theme::acrylic() {
                t("已启用")
            } else {
                t("系统未提供")
            })
            .size(13)
            .style(theme::hint),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        // ⑦' **允许被截图**（用户 2026-10-21 要求"必须能截图"，但这两件事在
        // Windows 上不能同时成立 —— 见 `GuiPrefs::capture` 的说明）。
        let capture_row = row![
            self.setting_label(t("允许被截图"), SETTING_LABEL_W),
            self.chip(
                t("允许截图"),
                self.allow_capture,
                Message::SetCaptureOk(true)
            ),
            self.chip(
                t("不允许截图"),
                !self.allow_capture,
                Message::SetCaptureOk(false),
            ),
            theme::label(t("关掉后背景是真实桌面，但截图/录屏拍不到窗口"))
                .size(12)
                .style(theme::hint),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        // ⑧ 长结果的处理方式（用户要求："增加左右拖动或者换行，放在设置里供用户选择"）。
        let wrap_row = row![
            self.setting_label(t("长结果"), SETTING_LABEL_W),
            self.chip(t("自动换行"), self.wrap, Message::SetWrap(true)),
            self.chip(t("横向滚动"), !self.wrap, Message::SetWrap(false)),
            theme::label(t("结果太长看不清时怎么办"))
                .size(12)
                .style(theme::hint),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        container(
            column![
                row![
                    // ⚠️ 标题用"设置面板"而不是光"设置"：词典是**子串最长优先**
                    // 匹配，而 CLI 的输出里满地都是"设置"（"已恢复默认设置…"），
                    // 一个两字短键迟早会在某句话里被误替换。
                    theme::label(t("设置面板")).size(16),
                    Space::new().width(Length::Fill),
                    // 提示"点别处就关" —— 用户明确要求了这个交互，
                    // 但没必要让每个人自己去试出来
                    theme::label(t("点面板外任意处关闭"))
                        .size(12)
                        .style(theme::hint),
                ]
                .align_y(Alignment::Center),
                rule::horizontal(1.0).style(theme::divider),
                precision_row,
                rule::horizontal(1.0).style(theme::divider),
                lang_row,
                rule::horizontal(1.0).style(theme::divider),
                font_row,
                rule::horizontal(1.0).style(theme::divider),
                theme_row,
                rule::horizontal(1.0).style(theme::divider),
                texture_block,
                rule::horizontal(1.0).style(theme::divider),
                shadow_row,
                rule::horizontal(1.0).style(theme::divider),
                acrylic_row,
                rule::horizontal(1.0).style(theme::divider),
                capture_row,
                rule::horizontal(1.0).style(theme::divider),
                wrap_row,
            ]
            .spacing(8),
        )
        .width(Length::Fill)
        .padding([12, 14])
        // ⚠️ 用 `dialog`（**接近不透明**的浮层样式）而不是 `panel`：
        // 玻璃面板的 alpha 只有 0.5 上下，下面的历史文字会透上来，
        // 与设置项混在一起 —— 用户实测反馈"设置与计算结果重叠"。
        // `dialog` 保留高光边与大投影（浮起来的感觉），但底色是实心的。
        .style(theme::dialog)
        .into()
    }

    /// 设置面板里的一个"选中态芯片"（主题、行布局这类二选一/开关用它）
    ///
    /// 收 `String`（已翻译）而不是 `&str`：`theme::mode_chip` 需要 `Move` 闭包
    /// 捕获 `selected`，而按钮文案是运行时翻译出来的。
    fn chip(&self, label: String, selected: bool, msg: Message) -> Element<'_, Message> {
        // ⚠️ 结构与工具条的 `chip` 同一套（见 `toolbar` 里的长注释）：
        // padding 给内层 container（玻璃层要盖到 padding），玻璃用 `push_under`
        // 垫在文字**下面**且让文字当 base 层决定尺寸 —— 写成 `stack![玻璃, 文字]`
        // 会让 `Fill` 的玻璃成为 base 把 chip 撑满可用空间。
        let padded = container(theme::label(label).size(13)).padding([5, 12]);
        let body: Element<'_, Message> = if theme::liquid_on() {
            Stack::new()
                .push(padded)
                .push_under(liquid::layer(theme::chip_glass(
                    selected,
                    theme::CHIP_RADIUS_GLASS,
                )))
                .into()
        } else {
            padded.into()
        };
        button(body)
            .padding(0)
            .style(move |t, s| theme::mode_chip(t, s, selected))
            .on_press(msg)
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
        let src = self.input_text();

        // ── ② 用户真正**看得见**的数学排版 ─────────────────────────────────
        //
        // ⚠️ 这一层**就是输入栏本身**，不再是"预览"：结构化之后树既能排版、
        // 又能精确说出"每个可点目标 ⇒ 光标落在哪一位"，所以不需要
        // "文本编辑器 + 预览"那套两层结构了。
        let shown: Element<'_, Message> = if src.is_empty() {
            // ⚠️ **空输入也要画光标**（用户反馈："没有输入时点击输入栏不会出现
            // 光标"）。以前空着只画提示语，光标是 `Node` 树的一部分、树是空的，
            // 于是"看不见光标"—— 用户根本不知道可以开始打字。
            row![
                mathrender::caret(19.0),
                theme::label(t("输入表达式，例如 sqrt(2)/3、x^2-4=0、int(x^2,x,0,1)"))
                    .size(17)
                    .style(theme::hint),
            ]
            .spacing(3)
            .align_y(Alignment::Center)
            .into()
        } else {
            // 树 → 渲染 IR。光标作为一根**真实参与布局的竖线**插进去，
            // 位置由 iced 算 —— 不需要猜任何字体的字形宽度。
            //
            // 第二个返回值是"可点目标表"，`Node::Hit` / `Node::Empty` 里的
            // `at` 就是它的下标 ⇒ 点它就把光标送到那个位置。
            let (node, _targets) = self.input.to_node();
            mathrender::build(
                &node,
                mathrender::Style2 {
                    size: 19.0,
                    color: Some(theme::colors().text),
                    wrap: true,
                },
                // 点空槽位框 / 点某个记号 ⇒ 光标落在那里（用户要求）
                Some(&|id| Message::PlaceCaret(id)),
            )
        };

        // `stack!` 把两层叠起来，高度取两者较大的 —— 所以分数/根号把排版撑高时
        // 输入区会跟着长高，而不是被 `INPUT_HEIGHT` 裁掉。
        //
        // ⚠️ **顺序很关键：后写的在上面**。必须把**排版层放在 editor 之上**。
        //
        // 反过来的话，透明的 editor 会把所有鼠标点击都吃掉 —— 它内部的定位是按
        // **纯文本字形**算的（`√2` 的宽度和 `sqrt(2)` 差得远），点出来的位置必然偏。
        // 用户的原话是"完全无法选中修改，一选中就变成了文本字母"。
        // 排版层在上之后，点击由我们自己的 `Node::At` 处理（位置精确到源字符偏移），
        // editor 只负责它真正不可替代的那件事：键盘、输入法、粘贴、框选。
        //
        // 外层的 `mouse_area` 是**兜底**：点在排版层的空白处（没有 `At` 覆盖的地方）
        // 时不移动光标，但至少要把焦点还给 editor，否则用户接着打字会没反应。
        // 排版层直接就是输入栏
        let editor_slot = container(shown)
            .width(Length::FillPortion(10))
            .padding([12, 14])
            .align_x(Alignment::Start);

        // `=` 与 `C` 给**固定宽高**：两者不等宽（56 / 46）是有意的，让 `=` 更醒目。
        //
        // ⚠️ **高度必须显式给**。第一版只给了宽度，`Row` 的交叉轴分配让按钮被压成
        // 一条 22px 高的细线（截图上只看得到色块，看不到文字）——
        // 而 editor 那边有 `Height::Fixed(56)` 撑着，落差全由按钮承担了。
        // 实测：宽度对、高度塌，是要单独看像素才能发现的一类问题。
        //
        // 计算中时给 `=` 去掉 `on_press`（等价于禁用）：不然用户会一直点，
        // 而 `evaluate` 里那条 `busy` 守卫会把后续点击全部静默丢掉 ——
        // 用户看到的是"点了没反应"。按钮画成不可按，意图就清楚了。
        // 同时把字形换成 `…`，与历史区那条「计算中…」互相印证。
        let go_glyph = if self.busy { "…" } else { "=" };
        let mut go = button(text(go_glyph).size(22).font(theme::font_math()))
            .width(Length::Fixed(56.0))
            .height(Length::Fixed(INPUT_HEIGHT))
            .style(|t, s| theme::key(t, KeyKind::Equals, s));
        if !self.busy {
            go = go.on_press(Message::Evaluate);
        }

        let clear = button(text("C").size(18).font(theme::font_math()))
            .width(Length::Fixed(46.0))
            .height(Length::Fixed(INPUT_HEIGHT))
            .style(|t, s| theme::key(t, KeyKind::Control, s))
            .on_press(Message::ClearInput);

        let row1 = row![editor_slot, clear, go]
            .spacing(8)
            .align_y(Alignment::Center);

        // "式子没写完"之类的**行内提示**（不占历史、不染红整块）
        let hint: Element<'_, Message> = match &self.input_hint {
            Some(h) => container(theme::label(h.as_str()).size(13).style(theme::danger_text))
                .padding([2, 14])
                .into(),
            None => Space::new().height(Length::Shrink).into(),
        };

        // ⚠️ **点在空白处 = 光标到末尾**。
        //
        // 字符级点击目标只能表达"落在某个记号**之前**"，最后一个记号**之后**
        // 那个位置没有任何目标可点 —— 用户实测："光标在 4|5，无论鼠标点哪里
        // 都无法移到 5 后面"。所以整块输入栏再包一层 `mouse_area` 兜底：
        // 内层目标会先捕获事件（`mouse_area` 只在 `!is_event_captured` 时才动作），
        // 只有落在空白处才轮到这里。
        let body: Element<'_, Message> = iced::widget::mouse_area(
            container(column![row1, hint])
                .width(Length::Fill)
                .padding(8)
                .style(theme::panel),
        )
        .on_press(Message::PlaceCaretEnd)
        .into();

        // 输入区只剩一行：排版层本身就是"输入栏"（用户要求），
        // 不再另起一条"预览"—— 那是旧方案（编辑器可见 + 预览在下）的产物。
        Self::glass_under(body, theme::panel_glass())
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
                    .font(theme::font_math_italic()),
                text(sup.to_string())
                    .size(KEY_FACE_SIZE * 0.68)
                    .font(theme::font_math()),
            ]
            .spacing(1)
            .align_y(Alignment::Start)
            .into();
        }

        // ⚠️ **所有键面统一用数学字体**（用户反馈："虚拟键盘和计算栏的字母和符号
        // 并没有使用数学字体，而是等线"）。
        //
        // 以前只给 `label_is_math` 认得的符号（π √ ≤ …）设数学字体，字母数字
        // （`x` `7` `ABC`）**不设** ⇒ 落到 iced 默认字体上，与旁边的 `π` 不是一套。
        //
        // ⚠️ **例外**：`⌫`（U+232B）在数学字体里**没有字形**，硬套会变成豆腐块
        //（实测）。这类"UI 图标符号"不是数学记号，保持默认字体即可。
        //
        // ⚠️ **键面与输入栏必须同一条规则**（用户反馈："输入栏的字母是斜体，
        // 但是虚拟键盘不是，请将虚拟键盘和输入栏字体同步"）：
        // 输入栏里单个字母是**变量**（斜体，见 `mathrender::is_variable`），
        // 所以键面上的 `x` / `e` / `a` 也必须斜体，否则同一个字母在两处是两种样子。
        // 多字母（`ABC`）与数字仍然直立。
        let _ = kind;
        let face = text(label.to_string()).size(KEY_FACE_SIZE);
        let face = if UI_ICON_KEYS.contains(&label) {
            face
        } else if mathrender::looks_like_variable(label) {
            face.font(theme::font_math_italic())
        } else {
            face.font(theme::font_math())
        };
        face.into()
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
                // 键面四向居中（容器宽高都填满）—— 见下面 `.center_x/.center_y` 的注释
                let face = container(face)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill);
                // 玻璃质感下，键面底下垫一层**液态玻璃**（见 `crate::liquid`）。
                // 它只管外观；尺寸与 hover 由着色器自己从 bounds / cursor 取。
                let inner: Element<'_, Message> = if theme::liquid_on() {
                    stack![
                        liquid::layer(theme::key_glass(k.kind, theme::KEY_RADIUS_GLASS)),
                        face
                    ]
                    .into()
                } else {
                    face.into()
                };
                r = r.push(
                    // ⚠️ **`button` 的内容默认贴在左上角**，不是居中。
                    // `iced_widget::button` 的 `layout` 走 `layout::padded`，
                    // 而 `padded` 把内容 `move_to((padding.left, padding.top))` ——
                    // 完全没有垂直居中的步骤（button 也没有 `center_y` / `vertical_alignment`
                    // 这类 API，翻遍 `button.rs` 只有 `width` / `height` / `padding`）。
                    //
                    // 所以只看宽度的 `container(face).width(Fill).center_x(Fill)` 会让键面
                    // **偏上**：容器撑满宽度但高度只有文字高，button 把它钉在顶部。
                    // 修法：容器**宽高都填满**并四向居中，内容才会落在键面正中心。
                    button(inner)
                        .width(w)
                        .height(Length::Fixed(KEY_HEIGHT))
                        // ⚠️ **padding 必须是 0**：玻璃层撑的是 button 的**内容盒**
                        //（= 边界 - padding），留 padding 玻璃就盖不到按键边缘。
                        // 键面本来就是居中的，不需要靠 padding 定位。
                        .padding(0)
                        .style(move |t, s| theme::key(t, k.kind, s))
                        .on_press(Message::Key(ri, ci)),
                );
            }
            rows = rows.push(r);
        }

        Self::glass_under(
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
            .into(),
            theme::panel_glass(),
        )
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
        // 与设置面板同理：浮层必须压得住下面的内容（见 `theme::dialog`）
        .style(theme::dialog);

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

/// 表达式回显是否按数学排版。
///
/// **恒为真**（除非这行是错误）—— 原因见 `history_entry` 的注释：回显是用户
/// 自己敲的表达式，天生就是数学式，**不随显示模式（MathIO / LineIO）变化**。
/// 抽成函数是为了能单测这条约定（用户要求："根号应当始终渲染"）。
/// 把实测耗时格式化成展示文本，**口径与 CLI 完全一致**。
///
/// CLI 的 `Timing::elapsed_text`（`crates/hipercalc/src/lib.rs`）是：
///
/// ```text
/// 整秒；不足 1 秒显示 `用时：<1秒`；关闭计时显示时返回空串
/// ```
///
/// 这里照抄它的两条规则：
/// - **只报整秒**、向下取整（`1.99 秒` 显示 `用时：1秒`）——
///   用户要的是量级（`100^100^10` 是"几十秒"还是"零点几秒"），不是毫秒精度；
/// - **不足 1 秒显示 `<1秒`**，而不是 `0秒` —— 后者会让人以为"没计时"。
///
/// 文案过 `i18n`（`用时：{0}秒` / `用时：<1秒` 两条词条已在词典里），
/// 所以切到 English 会变成 `Time: 3s`。
fn elapsed_text(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    if secs < 1 {
        t("用时：<1秒")
    } else {
        t_fmt("用时：{0}秒", &[&secs.to_string()])
    }
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
pub fn subscription(_state: &State) -> iced::Subscription<Message> {
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
        iced::window::Event::Moved(_) => Message::WindowMoved,
        iced::window::Event::Resized(_) => Message::CaptureBg,
        // 其余窗口事件本项目暂时不关心，映射成一个不会被处理的占位
        _ => Message::Noop,
    });

    // 键盘：**一个订阅同时处理**修饰键与按键。
    //
    // ⚠️ iced 0.14 没有 `keyboard::on_key_press`（那是别的版本的 API），
    // 只有 `keyboard::listen()` 这个"原始事件流"。拆成两个订阅也能跑，
    // 但每个事件会被广播两遍，不如合并成一处。
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

    iced::Subscription::batch([window_events, ctrl])
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
            // 位置包装：拍平后就是里面那棵子树
            Node::At { inner, .. } | Node::Hit { inner, .. } => flatten_node(inner),
            // 占位框 / 光标 / 参数逗号都不携带源文本，拍平后是空的
            Node::Empty { .. } | Node::EmptyFocused { .. } | Node::Caret | Node::Comma { .. } => {
                String::new()
            }
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
        assert_eq!(s.input_text(), "sum(,,,)", "输入框内容不该被动过");

        // 一打字提示就撤掉
        s.insert("k");
        assert!(s.input_hint.is_none(), "开始修改后提示应消失");
    }

    #[test]
    fn elapsed_text_matches_cli_convention() {
        use std::time::Duration;
        let _ = State::default().update(Message::SetLang(UiLang::ZhCn));

        // 不足 1 秒 → `<1秒`（**不是** `0秒`：后者会让人以为没计时）
        for d in [
            Duration::ZERO,
            Duration::from_millis(1),
            Duration::from_millis(999),
            Duration::from_micros(1),
        ] {
            let s = elapsed_text(d);
            assert!(
                s.contains("<1"),
                "{d:?} 应显示 <1秒，实际 {s:?}（对齐 CLI 的 elapsed_text）"
            );
        }

        // 整秒向下取整：1.99 秒 = `用时：1秒`（不是 2 秒）
        assert!(elapsed_text(Duration::from_millis(1000)).contains('1'));
        assert!(elapsed_text(Duration::from_millis(1999)).contains('1'));
        assert_eq!(
            elapsed_text(Duration::from_millis(1999)),
            elapsed_text(Duration::from_millis(1000)),
            "1.99 秒与 1.00 秒都应取整成 1 秒（向下取整，不四舍五入）"
        );
        assert!(elapsed_text(Duration::from_secs(2)).contains('2'));
        assert!(elapsed_text(Duration::from_secs(125)).contains("125"));
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
}
