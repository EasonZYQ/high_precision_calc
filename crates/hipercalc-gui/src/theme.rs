//! HiPerCalc 的视觉主题与全部样式函数。
//!
//! 设计目标（用户指定）：**WinUI3 观感 + 亚克力质感 + 圆角 + 深色/浅色双模式 +
//! 尽量接近 Apple 的 liquid glass**。
//!
//! 实现策略是**纯 iced、零系统依赖**：所有"玻璃感"都由三层叠加出来 ——
//! 1. 半透明渐变底（`Background::Gradient`，自上而下 alpha 递减 = 玻璃厚度差异）
//! 2. 1px 高光描边（低 alpha 浅色边，模拟边缘折射）
//! 3. 柔和投影（`Shadow`，让面板"浮"在背景之上）
//!
//! 真正的**桌面模糊**（亚克力）必须靠 DWM，见 `backdrop.rs`；
//! `PanelAlpha` 就是两者的接口：DWM 成功时窗口背景接近全透、让桌面透上来，
//! 失败时窗口背景不透明、靠上面这三层自己营造质感。**绝不假装成功**。
//!
//! 配色取自 WinUI3 的深/浅色设计令牌（强调色 `#4CC2FF` / `#0F6CBD`），
//! 因为用户明确说"类似 WinUI3"，用系统色系最不容易跑偏。

use iced::theme::Palette;
use iced::widget::text_editor as text_edit;
use iced::widget::{button, container, rule, text, text_editor, text_input};
use iced::{Background, Border, Color, Shadow, Theme, Vector};

// 字体相关的入口在 `fonts` 模块；这里转发常用的几个，让调用处只 `use theme::`
// 就够了（界面上几乎每个函数都要写字体，少一层路径少一点噪音）。
pub use crate::fonts::{
    FAMILY_CJK, MathFont, contains_cjk, font_cjk, font_for, font_math, label, math_font,
    set_math_font,
};

// ─────────────────────────────────────────────────────────────────────────────
// 调色板
// ─────────────────────────────────────────────────────────────────────────────

/// 颜色字面量用 `rgb8` 写（`0x0F, 0x6C, 0xBD`），比 `Color::from_rgb8(15,108,189)` 短，
/// 又比十六进制数字好读。`const fn` 保证能用在 `const` 上下文里。
const fn rgb8(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
}

/// 带透明度的颜色（alpha 用 0..=1）
const fn rgba8(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a,
    }
}

/// HiPerCalc 的本体色板。字段刻意用语义名（`surface`/`stroke`）而不是 `gray1/gray2`，
/// 换配色时不用猜哪个灰是干嘛的。
#[derive(Debug, Clone, Copy)]
pub struct Colors {
    /// 窗口最底层背景（亚克力成功时 alpha 很低，让桌面透上来）
    pub backdrop: Color,
    /// 面板表面（卡片、工具条）
    pub surface: Color,
    /// 面板上的高光描边
    pub stroke: Color,
    /// 次级强调面（键盘按键底色）
    pub surface_alt: Color,
    /// 主文本
    pub text: Color,
    /// 次要文本（表达式回显、单位、提示）
    pub text_dim: Color,
    /// 强调色（主按钮、光标、焦点）
    pub accent: Color,
    /// 强调色上的文字
    pub on_accent: Color,
    /// 错误色
    pub danger: Color,
    /// 成功色（精确值 `=`）
    pub success: Color,
    /// 分数横线 / 根号顶线
    pub math_stroke: Color,
    /// 弹窗遮罩（函数面板背后的压暗层）。
    ///
    /// ⚠️ 必须**随主题**：深色下用半透明黑是对的，但浅色下照搬会把整个亮界面
    /// 压成灰的 —— 用户看到的现象正是"浅色模式打开函数面板，颜色一下子变暗了"。
    pub scrim: Color,
}

impl Colors {
    /// WinUI3 Dark 令牌
    pub const DARK: Self = Self {
        backdrop: rgba8(0x20, 0x20, 0x20, 1.0),
        surface: rgba8(0x2B, 0x2B, 0x2B, 0.86),
        stroke: rgba8(0xFF, 0xFF, 0xFF, 0.09),
        surface_alt: rgba8(0x3A, 0x3A, 0x3A, 0.72),
        text: rgba8(0xF2, 0xF2, 0xF2, 1.0),
        text_dim: rgba8(0xA8, 0xA8, 0xA8, 1.0),
        accent: rgb8(0x4C, 0xC2, 0xFF),
        on_accent: rgba8(0x00, 0x1A, 0x2B, 1.0),
        danger: rgb8(0xFF, 0x99, 0xA4),
        success: rgb8(0x6C, 0xCB, 0x5F),
        math_stroke: rgba8(0xE8, 0xE8, 0xE8, 0.68),
        // 深色：压暗一档，让弹窗（本身是浅一层的玻璃）浮出来
        scrim: rgba8(0x00, 0x00, 0x00, 0.42),
    };

    /// WinUI3 Light 令牌
    pub const LIGHT: Self = Self {
        backdrop: rgba8(0xF3, 0xF3, 0xF3, 1.0),
        surface: rgba8(0xFF, 0xFF, 0xFF, 0.78),
        stroke: rgba8(0x00, 0x00, 0x00, 0.07),
        surface_alt: rgba8(0xFF, 0xFF, 0xFF, 0.60),
        text: rgba8(0x1A, 0x1A, 0x1A, 1.0),
        text_dim: rgba8(0x5D, 0x5D, 0x5D, 1.0),
        accent: rgb8(0x0F, 0x6C, 0xBD),
        on_accent: rgba8(0xFF, 0xFF, 0xFF, 1.0),
        danger: rgb8(0xC4, 0x2B, 0x1C),
        success: rgb8(0x0F, 0x7B, 0x0F),
        math_stroke: rgba8(0x20, 0x20, 0x20, 0.72),
        // 浅色：**不能**照搬深色那层的半透明黑。亮底上叠黑会整体压灰，
        // 用户看到的就是"切浅色后打函数面板，颜色变暗了"。
        // 这里用更淡的中性灰，只轻轻压一层、把弹窗托起来。
        scrim: rgba8(0x33, 0x33, 0x33, 0.16),
    };

    /// 按明暗取色板
    pub fn of(dark: bool) -> Self {
        if dark { Self::DARK } else { Self::LIGHT }
    }

    /// 背景要不要透出桌面（亚克力成功时）
    pub fn backdrop_for(self, acrylic: bool) -> Color {
        if acrylic {
            self.backdrop.scale_alpha(0.0)
        } else {
            self.backdrop
        }
    }
}

/// 亚克力是否可用。**由 `backdrop::enable()` 的探测结果决定**，不是配置项。
///
/// 存成进程级静态量而不是塞进 `State`：样式函数签名是 `Fn(&Theme) -> Style`，
/// 拿不到 State。用 `AtomicBool` 与 `calc_mode` 的做法保持一致。
static ACRYLIC: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 记录亚克力探测结果
pub fn set_acrylic(on: bool) {
    ACRYLIC.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// 亚克力是否已启用
pub fn acrylic() -> bool {
    ACRYLIC.load(std::sync::atomic::Ordering::Relaxed)
}

/// 当前是否深色模式（由 `State` 写入，供样式函数读取）
static DARK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// 设置明暗（切主题时调用）
pub fn set_dark(on: bool) {
    DARK.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// 当前是否深色
pub fn dark() -> bool {
    DARK.load(std::sync::atomic::Ordering::Relaxed)
}

/// 玻璃强度，用**百分数**存（0 = 关闭，100 = 最强）。
///
/// 用整数存是为了避开 `AtomicF32` 这种不存在的类型 —— 换算一次而已，
/// 精度到 1% 对"通透程度"这种主观指标完全够用。
///
/// 与 [`DARK`]、[`ACRYLIC`] 同一套理由：样式函数的签名是 `Fn(&Theme) -> Style`，
/// 拿不到 `State`，所以这类"全局观感开关"只能走进程级静态量。
static GLASS_PERCENT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(60);

/// 玻璃**底色的浓度**（0 = 完全透明，100 = 完全不透明）。
///
/// 与 [`GLASS_PERCENT`] 的分工（用户要求两者都要）：
/// - **强度**是"整体玻璃效果有多强"—— 它同时影响底色透明度、描边高光、投影幅度；
/// - **底色**只拧一个旋钮：面板底色本身有多实。想要"更飘"就调低，想要"更稳、
///   字更好认"就调高。
///
/// 默认 70 而不是 100：配合 [`ambient_background`] 的氛围底，70% 就能看出
/// "面板是块玻璃"，而 100% 会让所有面板看起来还是不透明的色块。
static GLASS_TINT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(70);

/// 设置玻璃强度（0.0 = 关闭，走不透明纯色；1.0 = 最强）
pub fn set_glass(amount: f32) {
    let pct = (amount.clamp(0.0, 1.0) * 100.0).round() as u8;
    GLASS_PERCENT.store(pct, std::sync::atomic::Ordering::Relaxed);
}

/// 当前玻璃强度（0.0 – 1.0）
pub fn glass_amount() -> f32 {
    GLASS_PERCENT.load(std::sync::atomic::Ordering::Relaxed) as f32 / 100.0
}

/// 投影（阴影）强度，同样按百分数存（0 = 完全没有阴影）。
///
/// 用户要求把它做成设置项："阴影的透明调节加到设置里"。
/// 0 会让界面变得非常"平"（面板浮不起来），但对喜欢极简的人是合法的选择。
static SHADOW_PERCENT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(100);

/// 设置投影强度（0.0 = 无投影，1.0 = 最强）
pub fn set_shadow(amount: f32) {
    let pct = (amount.clamp(0.0, 1.0) * 100.0).round() as u8;
    SHADOW_PERCENT.store(pct, std::sync::atomic::Ordering::Relaxed);
}

/// 当前投影强度（0.0 – 1.0）
pub fn shadow_amount() -> f32 {
    SHADOW_PERCENT.load(std::sync::atomic::Ordering::Relaxed) as f32 / 100.0
}

/// 设置玻璃底色浓度（0.0 = 完全透明，1.0 = 完全不透明）
pub fn set_glass_tint(tint: f32) {
    let pct = (tint.clamp(0.0, 1.0) * 100.0).round() as u8;
    GLASS_TINT.store(pct, std::sync::atomic::Ordering::Relaxed);
}

/// 当前玻璃底色浓度（0.0 – 1.0）
pub fn glass_tint() -> f32 {
    GLASS_TINT.load(std::sync::atomic::Ordering::Relaxed) as f32 / 100.0
}

/// 当前色板
pub fn colors() -> Colors {
    Colors::of(dark())
}

/// 构造 iced 主题（供 `iced::application(...).theme(..)` 使用）
pub fn theme(_state: &super::app::State) -> Theme {
    let dark = _state.dark;
    let c = Colors::of(dark);
    let palette = if dark {
        Palette {
            background: c.backdrop,
            text: c.text,
            primary: c.accent,
            success: c.success,
            warning: rgb8(0xF7, 0x63, 0x0C),
            danger: c.danger,
        }
    } else {
        Palette {
            background: c.backdrop,
            text: c.text,
            primary: c.accent,
            success: c.success,
            warning: rgb8(0x9D, 0x5D, 0x00),
            danger: c.danger,
        }
    };
    Theme::custom(
        if dark {
            "HiPerCalc Dark"
        } else {
            "HiPerCalc Light"
        },
        palette,
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// 圆角定额（统一在这里定，避免每个面板各写一个数字）
// ─────────────────────────────────────────────────────────────────────────────

/// 窗口级大面板
pub const RADIUS_PANEL: f32 = 12.0;
/// 卡片 / 输入框
pub const RADIUS_CARD: f32 = 8.0;
/// 按钮 / 按键
pub const RADIUS_BUTTON: f32 = 6.0;

// ─────────────────────────────────────────────────────────────────────────────
// 样式函数
// ─────────────────────────────────────────────────────────────────────────────

/// 玻璃面板：半透明渐变 + 1px 高光边 + 柔和阴影。
///
/// 这是"liquid glass"的核心。三个参数分别控制：
/// - `lift`：投影强度（0 = 贴底，1 = 浮得最高）——用于区分主面板与次级面板
/// - `tint`：背景 tint 的 alpha 倍数（键盘区比结果区更透，视觉分层）
fn glass(c: &Colors, lift: f32, tint: f32) -> container::Style {
    let g = glass_amount();
    // 底色浓度 = 各面板自己的相对浓度 × 全局旋钮（用户在设置里调，见 [`GLASS_TINT`]）。
    // 玻璃越强 → 底色越透（让氛围底 / DWM 模糊透上来）；0 时是完全不透明的纯色。
    //
    // ⚠️ **深色下不做透明度衰减**（k = 0）：深色的面底色（#2B2B2B）本来就只比背景
    //（#202020）亮十来级，一旦变透就直接融进背景 —— 用户看到的不是"玻璃"，
    // 而是"整个界面糊成一片黑"（快照里实测过：面板全看不见了）。
    // 所以深色下"强度"只作用于高光描边与投影 —— 那两样才撑得起玻璃感。
    // 浅色下才真正调透明度（白面板透出氛围底的色彩）。
    //
    // 系数 0.72 而不是 0.55：用户实测反馈"开到 100% 也看不出差别" ——
    // 0.55 时 `(1 - 0.55g)` 在 g∈[0.6, 1] 只从 0.67 变到 0.45，肉眼分不出。
    let k = if dark() { 0.0 } else { 0.72 };
    let alpha = tint * glass_tint() * (1.0 - k * g);
    // 玻璃越强 → 投影越明显。"玻璃厚度"这个观感来自三件事，缺一不可：
    // 半透明底色（上面）、1px 高光描边（下面）、柔和投影（这里）。
    let lift = lift * (0.35 + 1.15 * g);
    let base = c.surface;
    container::Style {
        background: Some(gradient_background([
            (0.0, base.scale_alpha(base.a * alpha)),
            (1.0, base.scale_alpha(base.a * alpha * 0.62)),
        ])),
        text_color: Some(c.text),
        border: Border {
            // 描边是"玻璃边缘那道高光"。深色主题下 `stroke` 本来就是浅色（白 0.09），
            // 提亮即可；**浅色主题下 `stroke` 是黑 0.07**，直接用会变成一条分隔线
            //（那是"卡片边框"的观感，不是玻璃）—— 所以浅色下换成**白色**高光。
            color: if dark() {
                c.stroke.scale_alpha(1.0 + 1.2 * g)
            } else {
                Color::WHITE.scale_alpha(0.30 + 0.45 * g)
            },
            width: 1.0,
            radius: RADIUS_PANEL.into(),
        },
        shadow: {
            // 用户的"投影强度"旋钮：0 时连偏移一起归零（只把 alpha 调零会留下
            // 一片看不见但仍占布局的空白，观感上不如彻底去掉干净）
            let sh = shadow_amount();
            Shadow {
                color: Color::BLACK.scale_alpha(0.30 * lift * sh),
                offset: Vector::new(0.0, 3.0 * lift * sh),
                blur_radius: 18.0 * lift * sh,
            }
        },
        snap: false,
    }
}

/// 两个色按 `t` 线性插值（`t = 0` 取 `base`，`t = 1` 取 `over`）。
///
/// `Color` 没有内置的 `lerp`，而这里只需要 RGB 三个通道 —— 手算比拉一个依赖划算。
/// 结果**强制不透明**：调用方（[`ambient_background`]）要的正是"实色"。
fn mix(base: Color, over: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color {
        r: base.r + (over.r - base.r) * t,
        g: base.g + (over.g - base.g) * t,
        b: base.b + (over.b - base.b) * t,
        a: 1.0,
    }
}

/// 窗口的**氛围底色**：一层不透明的斜向柔光（左上偏亮 → 右下回到底色）。
///
/// ## 这不是装饰，是"玻璃能成立"的前提
///
/// 半透明面板透过去的若是同一个纯色，观感就是一堆不透明的灰块 —— 跟玻璃毫无关系。
/// 用户实测反馈过"只有窗口顶部一栏像玻璃，整个界面都不是"，根因就在这里
///（亚克力在某些环境下不可用，窗口底色退化成纯色）。
/// 有了这层带方向的柔光，面板各处透出的色才不同，玻璃的"厚度"才立得住。
///
/// ## 为什么两个停靠点都必须不透明
///
/// 窗口开了 `transparent: true`。这层若带 alpha，亚克力失败时会直接透出
/// 桌面甚至黑色 ⇒ 花屏。所以这里用 [`mix`] 调出实色，而不是靠 alpha 叠加。
fn ambient_background(c: &Colors) -> Background {
    // 把强调色掺进底色当"光"。比例刻意很小（0.12 / 0.02）：再大就不像系统界面了
    let light = mix(c.backdrop, c.accent, if dark() { 0.14 } else { 0.10 });
    let shade = mix(c.backdrop, c.accent, 0.02);
    // 45° —— 斜向比纯竖向更有"光从某个方向来"的感觉
    let mut linear = iced::gradient::Linear::new(iced::Radians(std::f32::consts::FRAC_PI_4));
    linear = linear.add_stop(0.0, light);
    linear = linear.add_stop(1.0, shade);
    Background::Gradient(iced::Gradient::Linear(linear))
}

/// 造一个自上而下的线性渐变背景。
///
/// `Background::Gradient` 要的是 `Gradient`（不是 `Linear`）—— `Linear` 是
/// `Gradient` 的一个字段。另外停靠点必须**按位置递增**，否则 iced 内部插值会错乱。
fn gradient_background(stops: [(f32, Color); 2]) -> Background {
    let mut linear = iced::gradient::Linear::new(iced::Radians(std::f32::consts::PI));
    for (offset, color) in stops {
        linear = linear.add_stop(offset, color);
    }
    Background::Gradient(iced::Gradient::Linear(linear))
}

/// 主面板（工具条 / 结果区 / 输入区）
pub fn panel(_theme: &Theme) -> container::Style {
    glass(&colors(), 1.0, 1.0)
}

/// 输入框下方那条**实时数学预览**的底衬。
///
/// 刻意比主面板更暗/更淡：预览是"正在打的东西"的辅助回显，
/// 不该和真正的结果区抢注意力。所以只给一个极淡的底色 + 细边框。
pub fn preview_strip(_theme: &Theme) -> container::Style {
    let c = colors();
    container::Style {
        background: Some(Background::Color(
            c.surface_alt.scale_alpha(c.surface_alt.a * 0.34),
        )),
        text_color: Some(c.text),
        border: Border {
            color: c.stroke.scale_alpha(c.stroke.a * 0.7),
            width: 1.0,
            radius: RADIUS_BUTTON.into(),
        },
        ..container::Style::default()
    }
}

/// 结果区的卡片。
///
/// 走与工具条同一套玻璃语言（用户要求："liquidglass 不仅仅窗口栏有，
/// 整个计算器都应该有"），只是 `lift` 给得小一点 —— 它托在工具条下面，
/// 层次该低一档，否则整屏的投影会互相打架。
pub fn panel_card(_theme: &Theme) -> container::Style {
    glass(&colors(), 0.7, 0.78)
}

/// 次级面板（键盘底座，浮得低一点，避免和主面板抢视线）
pub fn panel_low(_theme: &Theme) -> container::Style {
    glass(&colors(), 0.45, 0.72)
}

/// 窗口根容器：亚克力成功时**完全透明**（让 DWM 的模糊透上来 —— 那才是真正的
/// liquid glass），否则铺一层[氛围底色][ambient_background]。
pub fn root(_theme: &Theme) -> container::Style {
    let c = colors();
    container::Style {
        background: Some(if acrylic() {
            Background::Color(c.backdrop_for(true))
        } else {
            ambient_background(&c)
        }),
        text_color: Some(c.text),
        ..container::Style::default()
    }
}

/// 工具条上的模式按钮。
///
/// 四态都必须显式给色 —— iced 的默认按钮样式在自定义主题下不好看，
/// 而且 hover/press 的反馈必须**看得见**，否则用户不知道点中了没有。
pub fn mode_chip(_theme: &Theme, status: button::Status, selected: bool) -> button::Style {
    let c = colors();
    let (bg, fg, border) = match (selected, status) {
        (true, button::Status::Hovered) => (c.accent.scale_alpha(0.92), c.on_accent, c.accent),
        (true, button::Status::Pressed) => (c.accent.scale_alpha(0.78), c.on_accent, c.accent),
        (true, _) => (c.accent, c.on_accent, c.accent),
        (false, button::Status::Hovered) => (
            c.surface_alt.scale_alpha(1.35),
            c.text,
            c.stroke.scale_alpha(1.8),
        ),
        (false, button::Status::Pressed) => (c.surface_alt.scale_alpha(0.65), c.text, c.stroke),
        (false, _) => (c.surface_alt.scale_alpha(0.0), c.text_dim, c.stroke),
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: fg,
        border: Border {
            color: border,
            width: 1.0,
            radius: RADIUS_BUTTON.into(),
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

/// **"隐形"编辑器**：`text_editor` 的每一处可见部分都透明化。
///
/// ## 为什么需要它（用户要求："不要输入栏，预览就是输入栏"）
///
/// `text_editor` 只能显示纯文本（`sqrt(2)` 就是那 7 个字符），它**没有**
/// "在这里画一根号"的扩展点（唯一开放的 `Highlighter` 只有 `color` / `font`）。
/// 但它是**唯一**能接管键盘、中文输入法、Ctrl+V、鼠标框选的东西 ——
/// 自己从零实现这些要跟系统输入法对接，做不出来。
///
/// 所以做法是**分层**：`text_editor` 藏在下面继续收输入，但一个字都不显示
///（`background` / `placeholder` / `value` / `selection` 全透明）；
/// 用户看到的是它上面那一层 `mathrender` 排出来的数学式。
///
/// ⚠️ **光标也会一起消失**：iced 画光标用的就是 `style.value` 的颜色
///（见 `iced_widget` 的 `text_editor::draw`，`Selection::Caret` 分支）。
/// 所以光标得由排版层自己画（`mathrender::Node::Caret` + [`caret`]）。
pub fn invisible_editor(_theme: &Theme, _status: text_edit::Status) -> text_editor::Style {
    text_editor::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border::default(),
        placeholder: Color::TRANSPARENT,
        value: Color::TRANSPARENT,
        selection: Color::TRANSPARENT,
    }
}

/// **输入光标**（排版层自绘的那根竖线）。
///
/// 输入区改成"预览即输入栏"后，背后的 `text_editor` 被透明化，它自己的光标
/// 也跟着看不见了（iced 的光标用的就是 `style.value` 的颜色）。
/// 所以光标由 `mathrender` 在解析时插一个 `Node::Caret`，样式在这里定。
pub fn caret(_theme: &Theme) -> container::Style {
    let c = colors();
    container::Style {
        background: Some(Background::Color(c.accent)),
        ..container::Style::default()
    }
}

/// **空参数占位框**（`sqrt()` / `1/` / `sum(,k,1,10)` 里那些还没输入的位置）。
///
/// Desmos 用的是一个灰色小方块。刻意**不做成虚线框**（虽然也有编辑器这么做）：
/// 实心方块在小字号下更清楚，而且它表示的是"这里缺一个东西"，
/// 不是"这里可以画一条线"。
pub fn placeholder(_theme: &Theme) -> container::Style {
    let c = colors();
    container::Style {
        // 半透明的"阴影块"：比正文弱、但比背景显眼，一眼能看出"这里还缺东西"。
        //
        // ⚠️ alpha 不能太低：0.38 那版在快照里**几乎看不见**（方块只有 9px，
        // 再淡就等于没有），而"这里缺个东西"正是这个占位框存在的唯一理由。
        background: Some(Background::Color(c.text_dim.scale_alpha(0.62))),
        border: Border {
            color: c.text_dim.scale_alpha(0.95),
            width: 1.0,
            radius: 3.0.into(),
        },
        shadow: Shadow {
            color: Color::BLACK.scale_alpha(0.10),
            offset: Vector::new(0.0, 1.0),
            blur_radius: 2.0,
        },
        ..container::Style::default()
    }
}

/// 占位框外面那层"光板"按钮（不要任何视觉，样式全在里面的方块上）
pub fn bare_button(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: None,
        text_color: Color::WHITE,
        border: Border::default(),
        shadow: Shadow::default(),
        snap: false,
    }
}

/// **浮层面板**的样式（设置面板 / 函数面板）。
///
/// ## 为什么不能直接用 `panel`（玻璃）
///
/// 用户实测反馈："设置与计算结果重叠，设置应该遮住计算结果"。
/// 玻璃面板的底色 alpha 只有 0.5 上下，**下面的历史文字会透上来**，
/// 与设置项的字混在一起 —— 读起来非常糟。
///
/// 浮层和普通面板的要求不一样：普通面板是"界面的一部分"（透一点好看），
/// 浮层是"压在界面之上的对话"（**必须压得住**）。
/// 所以这里保留玻璃的**高光边 + 大投影**（厚度感、浮起来的感觉都在），
/// 但底色用**接近不透明**的面板色。
pub fn dialog(_theme: &Theme) -> container::Style {
    let c = colors();
    let g = glass_amount();
    let sh = shadow_amount();
    // 把面板色的 alpha 拉到 1：`surface` 的 RGB 本来就是"面板该有的颜色"，
    // 只是它在普通面板里被当作半透明底用。这里要的是实心。
    let solid = Color {
        a: 1.0,
        ..c.surface
    };
    container::Style {
        // 只留一点点渐变，免得整块面板看起来是死板的纯色
        background: Some(gradient_background([(0.0, solid), (1.0, solid)])),
        text_color: Some(c.text),
        border: Border {
            color: if dark() {
                c.stroke.scale_alpha(1.0 + 1.2 * g)
            } else {
                Color::WHITE.scale_alpha(0.85)
            },
            width: 1.0,
            radius: RADIUS_PANEL.into(),
        },
        shadow: Shadow {
            // 投影比普通面板重一档：它是浮在最上面的那一层
            color: Color::BLACK.scale_alpha(0.42 * sh),
            offset: Vector::new(0.0, 8.0 * sh),
            blur_radius: 32.0 * sh,
        },
        snap: false,
    }
}

/// 键盘按键的视觉分组 —— 不同分组用不同底色，用户扫一眼就能定位。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    /// 数字与小数点
    Digit,
    /// 运算符
    Operator,
    /// 括号、逗号
    Structural,
    /// 函数名、常数符号
    Function,
    /// 退格 / 清空
    Control,
    /// 等号（求值）
    Equals,
}

/// 按键样式：分组定底色，四态定亮度。
pub fn key(_theme: &Theme, kind: KeyKind, status: button::Status) -> button::Style {
    let c = colors();
    let base = match kind {
        KeyKind::Digit => c.surface_alt,
        KeyKind::Operator => c.accent.scale_alpha(0.16),
        KeyKind::Structural => c.surface_alt.scale_alpha(0.55),
        KeyKind::Function => c.accent.scale_alpha(0.10),
        KeyKind::Control => c.danger.scale_alpha(0.14),
        KeyKind::Equals => c.accent.scale_alpha(0.72),
    };
    let fg = match kind {
        KeyKind::Equals => c.on_accent,
        KeyKind::Operator | KeyKind::Function => c.accent,
        KeyKind::Control => c.danger,
        _ => c.text,
    };
    // 按下变暗、悬停变亮。
    // ⚠️ `iced::Color` **只有 `scale_alpha`**，没有 `scale_rgb`（第一版写错过）——
    // 所以亮度变化用 alpha 表达：浅色主题下 alpha 提高即"更实"，深色下效果同理。
    // 各分组用同一个系数，保证手感一致。
    let bg = match status {
        button::Status::Hovered => base.scale_alpha(base.a * 1.35),
        button::Status::Pressed => base.scale_alpha(base.a * 0.62),
        button::Status::Disabled => base.scale_alpha(base.a * 0.30),
        _ => base,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: if status == button::Status::Disabled {
            fg.scale_alpha(0.45)
        } else {
            fg
        },
        border: Border {
            color: if status == button::Status::Hovered {
                c.stroke.scale_alpha(2.2)
            } else {
                c.stroke
            },
            width: 1.0,
            radius: RADIUS_CARD.into(),
        },
        shadow: Shadow {
            color: Color::BLACK.scale_alpha(0.20),
            offset: Vector::new(0.0, 1.0),
            blur_radius: 3.0,
        },
        snap: false,
    }
}

/// 结果区的行样式（历史条目）
pub fn history_row(_theme: &Theme, error: bool) -> container::Style {
    let c = colors();
    container::Style {
        background: Some(Background::Color(if error {
            c.danger.scale_alpha(0.08)
        } else {
            Color::TRANSPARENT
        })),
        text_color: Some(if error { c.danger } else { c.text }),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: RADIUS_CARD.into(),
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

/// 主输入框（玻璃卡片）
pub fn input(_theme: &Theme, status: text_edit::Status) -> text_editor::Style {
    let c = colors();
    let focused = matches!(status, text_edit::Status::Focused { .. });
    text_editor::Style {
        background: Background::Color(c.surface_alt.scale_alpha(0.42)),
        border: Border {
            color: if focused {
                c.accent.scale_alpha(0.85)
            } else {
                c.stroke.scale_alpha(1.6)
            },
            width: if focused { 1.5 } else { 1.0 },
            radius: RADIUS_CARD.into(),
        },
        placeholder: c.text_dim.scale_alpha(0.75),
        value: c.text,
        selection: c.accent.scale_alpha(0.32),
    }
}

/// 顶部搜索/次要输入框（精度等数值设置）
pub fn field(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let c = colors();
    let focused = matches!(status, text_input::Status::Focused { .. });
    text_input::Style {
        background: Background::Color(c.surface_alt.scale_alpha(0.36)),
        border: Border {
            color: if focused {
                c.accent
            } else {
                c.stroke.scale_alpha(1.6)
            },
            width: 1.0,
            radius: RADIUS_BUTTON.into(),
        },
        icon: c.text_dim,
        placeholder: c.text_dim.scale_alpha(0.7),
        value: c.text,
        selection: c.accent.scale_alpha(0.32),
    }
}

/// 分数横线 / 根号顶线
pub fn math_rule(_theme: &Theme) -> rule::Style {
    let c = colors();
    rule::Style {
        color: c.math_stroke,
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: false,
    }
}

/// 弱分割线（面板之间）
pub fn divider(_theme: &Theme) -> rule::Style {
    let c = colors();
    rule::Style {
        color: c.stroke.scale_alpha(1.5),
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: false,
    }
}

/// 历史里的"表达式回显"文本样式（次要色）
pub fn echo(_theme: &Theme) -> text::Style {
    text::Style {
        color: Some(colors().text_dim),
    }
}

// 结果文字的颜色**不在这里**定义：数学排版（`mathrender::Style2`）需要按语义
// （错误 / 精确 / 近似）着色，而样式函数签名是 `Fn(&Theme) -> Style`、拿不到 State。
// 与其造两个只能二选一的 `text::Style` 函数，不如让调用方直接从 `colors()` 取色 ——
// 少一层间接，也不会出现"两处配色各改一半"的漂移。

/// 提示 / 占位文本
pub fn hint(_theme: &Theme) -> text::Style {
    text::Style {
        color: Some(colors().text_dim),
    }
}

/// 强调文本（小节标题）
pub fn accent_text(_theme: &Theme) -> text::Style {
    text::Style {
        color: Some(colors().accent),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palettes_have_expected_contrast_direction() {
        // 深色：文本比背景亮；浅色：文本比背景暗。写反了界面会看不清。
        let d = Colors::DARK;
        let l = Colors::LIGHT;
        assert!(d.text.r > d.backdrop.r, "深色主题的文本应比背景亮");
        assert!(l.text.r < l.backdrop.r, "浅色主题的文本应比背景暗");
    }

    /// ⚠️ **浅色主题下函数弹窗的遮罩不能把界面压灰**。
    ///
    /// 这是用户报过的现象："浅色模式时打开函数面板颜色会变暗" —— 根因是遮罩
    /// 写死成 `BLACK 0.42`。现在它随主题走，这条测试钉住"浅色的遮罩明显更淡"。
    #[test]
    fn scrim_is_lighter_in_light_theme() {
        let d = Colors::DARK.scrim;
        let l = Colors::LIGHT.scrim;
        assert!(
            l.a < d.a,
            "浅色主题的遮罩必须更淡（深色 {:.2} vs 浅色 {:.2}）",
            d.a,
            l.a
        );
        assert!(l.a <= 0.25, "浅色遮罩的 alpha 应很低，否则会把亮界面压灰");
    }

    /// 玻璃强度：0.0 = 关闭，1.0 = 最强；值能原样读回（1% 量化），越界夹得住
    #[test]
    fn glass_amount_round_trips() {
        let before = glass_amount();
        for v in [0.0_f32, 0.33, 1.0] {
            set_glass(v);
            assert!(
                (glass_amount() - v).abs() < 0.011,
                "设 {v} 后应读回接近它的值（1% 量化），实际 {}",
                glass_amount()
            );
        }
        set_glass(-1.0);
        assert_eq!(glass_amount(), 0.0, "下界应夹到 0");
        set_glass(9.0);
        assert_eq!(glass_amount(), 1.0, "上界应夹到 1");
        set_glass(before);
    }

    /// 玻璃强度**真的**作用在面板上（不是个只存不用的开关）。
    ///
    /// ⚠️ 用**浅色**主题来验"透明度"这一路：深色下强度刻意不改透明度
    ///（面板会融进背景，见 `glass()` 里的长注释），所以拿深色去断言
    /// "背景变了"必然失败 —— 那正是第一版写错的地方。
    #[test]
    fn glass_strength_changes_panel_alpha() {
        let before = glass_amount();
        let was_dark = dark();
        // ⚠️ 样式函数读的是**进程级**的 `dark()`，不是传进去的那个 `Theme` ——
        // 想在测试里验"浅色那一档"，必须先把全局开关切过去（第一版就栽在这里：
        // 传 `Theme::Light` 却拿到深色色板，断言于是"看起来莫名其妙"地失败）。
        set_dark(false);
        set_glass(0.0);
        let solid = format!("{:?}", panel(&Theme::Light).background);
        set_glass(1.0);
        let clear = format!("{:?}", panel(&Theme::Light).background);
        assert_ne!(
            solid, clear,
            "浅色下玻璃强度必须真的改变面板底色（否则设置没接上样式）"
        );
        // 深色下改的是**描边与投影**，同样必须变
        set_dark(true);
        set_glass(0.0);
        let solid = format!("{:?}", panel(&Theme::Dark).border);
        set_glass(1.0);
        let clear = format!("{:?}", panel(&Theme::Dark).border);
        assert_ne!(solid, clear, "深色下玻璃强度必须改变高光描边");
        set_dark(was_dark);
        set_glass(before);
    }

    #[test]
    fn acrylic_switches_backdrop_transparency() {
        let c = Colors::DARK;
        assert_eq!(c.backdrop_for(false).a, 1.0, "无亚克力时底色必须不透明");
        assert_eq!(
            c.backdrop_for(true).a,
            0.0,
            "有亚克力时底色必须全透，让桌面透上来"
        );
    }

    #[test]
    fn theme_names_differ_by_mode() {
        set_dark(true);
        assert_eq!(colors().text, Colors::DARK.text);
        set_dark(false);
        assert_eq!(colors().text, Colors::LIGHT.text);
        set_dark(true);
    }
}
