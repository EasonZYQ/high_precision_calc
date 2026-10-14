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
use iced::widget::{button, container, rule, text, text_editor, text_input};
use iced::{Background, Border, Color, Shadow, Theme, Vector};

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
    Theme::custom(if dark { "HiPerCalc Dark" } else { "HiPerCalc Light" }, palette)
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
    let base = c.surface;
    container::Style {
        background: Some(Background::Gradient(
            iced::gradient::Linear::new(iced::Radians(std::f32::consts::PI))
                .add_stop(0.0, base.scale_alpha(base.a * tint))
                .add_stop(1.0, base.scale_alpha(base.a * tint * 0.62)),
        )),
        text_color: Some(c.text),
        border: Border {
            color: c.stroke,
            width: 1.0,
            radius: RADIUS_PANEL.into(),
        },
        shadow: Shadow {
            color: Color::BLACK.scale_alpha(0.30 * lift),
            offset: Vector::new(0.0, 3.0 * lift),
            blur_radius: 18.0 * lift,
        },
        snap: false,
    }
}

/// 主面板（工具条 / 结果区 / 输入区）
pub fn panel(_theme: &Theme) -> container::Style {
    glass(&colors(), 1.0, 1.0)
}

/// 次级面板（键盘底座，浮得低一点，避免和主面板抢视线）
pub fn panel_low(_theme: &Theme) -> container::Style {
    glass(&colors(), 0.45, 0.72)
}

/// 窗口根容器：亚克力成功时**完全透明**（让 DWM 的模糊透上来），
/// 否则填上不透明底色。
pub fn root(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(colors().backdrop_for(acrylic()))),
        text_color: Some(colors().text),
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
    // 按下变暗、悬停变亮：亮度变化量固定，保证各分组手感一致
    let bg = match status {
        button::Status::Hovered => base.scale_alpha(1.42).scale_rgb(1.10),
        button::Status::Pressed => base.scale_alpha(0.80).scale_rgb(0.86),
        button::Status::Disabled => base.scale_alpha(base.a * 0.35),
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
pub fn input(_theme: &Theme, status: text_editor::Status) -> text_editor::Style {
    let c = colors();
    let focused = matches!(status, text_editor::Status::Focused { .. });
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
            color: if focused { c.accent } else { c.stroke.scale_alpha(1.6) },
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

/// 结果文本样式
pub fn result_text(_theme: &Theme) -> text::Style {
    text::Style {
        color: Some(colors().text),
    }
}

/// 结果区是否精确（`=` 用成功色、`≈` 用普通色）
pub fn result_text_precise(_theme: &Theme, precise: bool) -> text::Style {
    let c = colors();
    text::Style {
        color: Some(if precise { c.success } else { c.text }),
    }
}

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

    #[test]
    fn acrylic_switches_backdrop_transparency() {
        let c = Colors::DARK;
        assert_eq!(c.backdrop_for(false).a, 1.0, "无亚克力时底色必须不透明");
        assert_eq!(c.backdrop_for(true).a, 0.0, "有亚克力时底色必须全透，让桌面透上来");
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
