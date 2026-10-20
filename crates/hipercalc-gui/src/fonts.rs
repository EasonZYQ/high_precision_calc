//! 字体：打包、选择、族名解析。
//!
//! ## 为什么要单独一个模块
//!
//! 用户提了两条**具体**的字体要求：
//!
//! 1. "符号和字母使用 markdown 渲染出来的那种字体" —— 也就是数学/衬线字体，
//!    不是系统默认的无衬线体。`π` `√` `≤` 这些记号用无衬线体排出来又矮又瘦。
//! 2. "中文字体全部使用等线" —— 即 **DengXian**。
//!
//! 加上后来追加的"在设置里添加这些全部字体，供用户选择"，字体就成了有**状态**的东西：
//! 界面里要能切、切了立刻生效、还要能在测试里验证。集中在这里管理比散在 `app.rs` 清楚。
//!
//! ## 字体怎么进到渲染器里
//!
//! iced 的字体加载入口是 `iced::Settings::fonts`（一串**原始字节**）。iced 内部把字节
//! 交给 `fontdb` 解析，`fontdb` 会把**集合字体（`.ttc`）里的每一个 face 都单独注册**
//! （见 `fontdb::Database::load_font_source`：它先 `fonts_in_collection` 数出 face 数，
//! 再逐个 `parse_face_info`）。所以 `Cambria.ttc` 里的第二个 face **"Cambria Math"**
//! 能按族名直接取到 —— 这一点是实测验证过的（见 `examples/fontprobe.rs`），
//! 不是推测。
//!
//! ## 打包策略
//!
//! **字体文件进仓库**（`assets/fonts/`，用户明确要求）。代价是仓库大了约 19 MB，
//! 换来的是"任何机器上观感一致"：不依赖用户装没装某个系统字体。
//! 只有 `DengXian.ttf` 一个文件就有 16 MB —— 它是覆盖面最全的，值这个体积。
//!
//! ⚠️ **字体族名必须与文件里的 `name` 表完全一致**。写错不会报错，
//! 只会**静默退回默认字体** —— 这是最容易白忙一场的地方。所以
//! `tests/fonts.rs` 里有一条测试专门核对"我们声明的族名，真的能按名字取到"。

use iced::widget::text;
use iced::{Font, Theme};

// ─────────────────────────────────────────────────────────────────────────────
// 打包的字体字节
// ─────────────────────────────────────────────────────────────────────────────

/// 打包的字体文件（名字 → 字节）。顺序即设置面板里的显示顺序。
///
/// 用 `include_bytes!` 而不是运行时读文件：exe 自带字体，换目录/分发都不会丢。
macro_rules! bundled {
    ($($name:literal => $path:literal),* $(,)?) => {
        &[$(($name, include_bytes!($path) as &[u8])),*]
    };
}

/// 随程序分发的字体：`(设置面板里显示的名字, 字节)`
pub static BUNDLED: &[(&str, &[u8])] = bundled![
    "等线（中文）" => "../assets/fonts/DengXian.ttf",
    "Cambria Math" => "../assets/fonts/Cambria.ttc",
    "Times New Roman" => "../assets/fonts/TimesNewRoman.ttf",
    "Georgia" => "../assets/fonts/Georgia.ttf",
    "Constantia" => "../assets/fonts/Constantia.ttf",
    "Bookman Old Style" => "../assets/fonts/BookmanOldStyle.ttf",
    // ── 变量用的**斜体**字面（数学排版里变量是斜体、数字与函数名直立）──
    // ⚠️ 现有 5 套字体文件里**只有常规体**，没有 italic face，而 iced 不会合成斜体，
    // 所以必须单独打包。Constantia 没有斜体（系统里也没有），它会退回直立。
    "Cambria Italic" => "../assets/fonts/cambriai.ttf",
    "Times New Roman Italic" => "../assets/fonts/timesi.ttf",
    "Georgia Italic" => "../assets/fonts/georgiai.ttf",
    "Bookman Old Style Italic" => "../assets/fonts/bookosi.ttf",
];

/// 交给 `iced::Settings::fonts` 的全部字体字节。
///
/// GUI 的 `main.rs` 起事件循环时把这份列表传进去 —— **必须在窗口创建前**，
/// 之后渲染器才知道这些族名。
pub fn bundled_fonts() -> Vec<std::borrow::Cow<'static, [u8]>> {
    BUNDLED
        .iter()
        .map(|(_, bytes)| std::borrow::Cow::Borrowed(*bytes))
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// 数学字体（用户可切换）
// ─────────────────────────────────────────────────────────────────────────────

/// 可选的**数学/符号字体**。
///
/// 这些是"符号和字母"用的字体 —— 键面上的 `π` `√` `≤`、输入框里的根号、
/// 结果区的数学排版都走这里。名字必须与字体文件里的族名完全一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MathFont {
    /// Cambria Math —— Word 公式编辑器用的那套，符号字形最标准（默认）
    #[default]
    CambriaMath,
    /// Times New Roman —— 经典衬线，字母数字最好看
    TimesNewRoman,
    /// Georgia —— 屏幕阅读友好，字形更厚重
    Georgia,
    /// Constantia —— 略窄，长表达式更省空间
    Constantia,
    /// Bookman Old Style —— 笔画粗、辨识度高
    BookmanOldStyle,
}

impl MathFont {
    /// 全部可选值（设置面板按这个顺序排）
    pub const ALL: [MathFont; 5] = [
        MathFont::CambriaMath,
        MathFont::TimesNewRoman,
        MathFont::Georgia,
        MathFont::Constantia,
        MathFont::BookmanOldStyle,
    ];

    /// **字体族名**（必须与字体文件的 `name` 表一致）
    pub fn family(self) -> &'static str {
        match self {
            MathFont::CambriaMath => "Cambria Math",
            MathFont::TimesNewRoman => "Times New Roman",
            MathFont::Georgia => "Georgia",
            MathFont::Constantia => "Constantia",
            MathFont::BookmanOldStyle => "Bookman Old Style",
        }
    }

    /// 设置面板里显示的名字（与 [`Self::family`] 一致，方便用户对上号）
    pub fn display_name(self) -> &'static str {
        self.family()
    }

    /// iced 的 `Font` 值
    pub fn font(self) -> Font {
        Font::with_name(self.family())
    }

    /// **变量斜体**用的族名。
    ///
    /// ⚠️ `Cambria Math` 这个族本身**没有斜体字面**（它是 OpenType 数学字体，
    /// 斜体由 MATH 表提供，字体文件里没有独立的 italic face）。所以变量斜体
    /// 必须落到它的**正文字体** `Cambria` 的 Italic 上 —— 这也是 Word 的做法。
    /// Constantia 在系统里就没有斜体，会退回直立（可接受的降级）。
    pub fn italic_family(self) -> &'static str {
        match self {
            MathFont::CambriaMath => "Cambria",
            other => other.family(),
        }
    }

    /// 从下标还原（设置面板的按钮索引 → 字体）
    pub fn from_index(i: usize) -> Option<Self> {
        Self::ALL.get(i).copied()
    }
}

/// 当前选中的数学字体（进程级开关）。
///
/// 与明暗、亚克力同样走 `AtomicU8`：样式/渲染函数拿不到 `State`，
/// 而字体选择要能影响**任意一处**文字 —— 包括 `mathrender` 里那些深层递归调用。
static MATH_FONT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// 设置数学字体
pub fn set_math_font(f: MathFont) {
    let idx = MathFont::ALL.iter().position(|x| *x == f).unwrap_or(0) as u8;
    MATH_FONT.store(idx, std::sync::atomic::Ordering::Relaxed);
}

/// 当前数学字体
pub fn math_font() -> MathFont {
    MathFont::from_index(MATH_FONT.load(std::sync::atomic::Ordering::Relaxed) as usize)
        .unwrap_or_default()
}

/// 当前数学字体的 iced `Font`（最常用的入口）
pub fn font_math() -> Font {
    math_font().font()
}

/// **变量用的斜体**字体（数学排版规范：变量斜体、数字与函数名直立）。
///
/// iced 不会给没有 italic face 的字体合成倾斜，所以这里换的是**族**：
/// 见 [`MathFont::italic_family`]。找不到斜体时 fontdb 会退回同族常规体
/// （观感退化成直立，不会报错、也不会缺字）。
pub fn font_math_italic() -> Font {
    Font {
        style: iced::font::Style::Italic,
        ..Font::with_name(math_font().italic_family())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 中文字体（固定为等线）
// ─────────────────────────────────────────────────────────────────────────────

/// 中文字体族名 —— 用户要求"中文字体全部使用等线"，这是个**定值**，
/// 不做成可选项（与数学字体不同）。
pub const FAMILY_CJK: &str = "DengXian";

/// 等线的 iced `Font`。
///
/// 用于所有**含中文**的文本。iced 不做"按字符自动换字体"（没有 font fallback 链的
/// 配置入口），所以我们必须显式指定：一段文字用哪个字体，要么全是数学字体、
/// 要么全是等线。界面上的中文说明与数学式因此是分开的两种 `text`。
pub fn font_cjk() -> Font {
    Font::with_name(FAMILY_CJK)
}

/// 给一段文本挑字体：**含中日韩字符就用等线，否则用数学字体**。
///
/// 这是界面上最省事的用法 —— 大部分文字要么是纯中文标签、要么是纯数学式，
/// 自动挑一下就够了，不必在每个 `text(..)` 处手工指定。
pub fn font_for(s: &str) -> Font {
    if contains_cjk(s) {
        // 中文标签里若**同时**含数学符号（如"平方根 √"），等线本身也带
        // 常用数学符号字形，不必拆开排 —— 拆了反而基线对不齐。
        font_cjk()
    } else {
        font_math()
    }
}

/// 文本里是否含中日韩字符
pub fn contains_cjk(s: &str) -> bool {
    s.chars().any(|c| {
        matches!(c as u32,
            0x3000..=0x303F   // CJK 标点
            | 0x3040..=0x30FF // 假名
            | 0x3400..=0x4DBF // 扩展 A
            | 0x4E00..=0x9FFF // 基本区
            | 0xF900..=0xFAFF // 兼容表意
            | 0xFF00..=0xFFEF // 全角
        )
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// 便捷构造
// ─────────────────────────────────────────────────────────────────────────────

/// 造一段"自动挑字体"的文本 widget。
///
/// 等价于 `text(s).font(theme::font_for(s))`，但在调用处短得多 ——
/// 界面上有几十处文字，每处都写一遍 `font_for` 太啰嗦。
///
/// ⚠️ 返回类型要**三个参数都给全**（`Text<'a, Theme, Renderer>`），且生命周期
/// 与输入绑定。两种写法都试过、都编译不过：
///
/// - 泛型消息参数 `M` —— `Text` 里根本没有消息参数，`button(..)` 处推不出来；
/// - `Text<'static, ..>` —— 太严，借用的 `String` 会被拒（`borrowed data escapes`）。
///
/// 正确抽象是"借用输入、无关消息类型"：`Text` 自带它自己的 `'a`，用同一个 `'a` 即可。
pub fn label<'a>(s: impl Into<String>) -> iced::widget::Text<'a, Theme, iced::Renderer> {
    let s = s.into();
    let f = font_for(&s);
    text(s).font(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_math_fonts_have_distinct_families() {
        let names: Vec<&str> = MathFont::ALL.iter().map(|f| f.family()).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(names.len(), sorted.len(), "数学字体族名有重复：{names:?}");
        for n in names {
            assert!(!n.is_empty());
        }
    }

    #[test]
    fn math_font_roundtrips_through_index() {
        for (i, f) in MathFont::ALL.iter().enumerate() {
            assert_eq!(MathFont::from_index(i), Some(*f));
        }
        assert_eq!(MathFont::from_index(99), None);
    }

    #[test]
    fn math_font_setting_is_global_and_defaults_to_cambria() {
        let old = math_font();
        set_math_font(MathFont::Georgia);
        assert_eq!(math_font(), MathFont::Georgia);
        assert_eq!(font_math().family, iced::font::Family::Name("Georgia"));
        set_math_font(old); // 还原，别污染别的测试
    }

    #[test]
    fn cjk_detection() {
        assert!(contains_cjk("工作精度"));
        assert!(contains_cjk("平方根 √"));
        assert!(contains_cjk("，"));
        assert!(!contains_cjk("sqrt(2)"));
        assert!(!contains_cjk("Cambria Math"));
        assert!(!contains_cjk(""));
    }

    /// 中文挑等线、非中文挑数学字体
    ///
    /// ⚠️ 不能断言 `label(..).font` —— iced 的 `Text` **没有**读回字体的公开方法
    /// （只有 `.font(..)` 这个 setter）。所以这里直接测**选字体的那个函数**，
    /// 它才是真正的逻辑；`label` 只是它的一层薄封装。
    #[test]
    fn font_selection_follows_content() {
        assert_eq!(font_for("精度"), font_cjk());
        assert_eq!(
            font_for("精度").family,
            iced::font::Family::Name(FAMILY_CJK)
        );
        assert_eq!(
            font_for("sqrt"),
            math_font().font(),
            "非中文应走当前选中的数学字体"
        );
        // 换一个数学字体，font_for 要跟着变（证明它读的是**当前**选择）
        let old = math_font();
        set_math_font(MathFont::Constantia);
        assert_eq!(
            font_for("sqrt").family,
            iced::font::Family::Name("Constantia")
        );
        set_math_font(old);
    }

    /// 打包的字体列表不能是空的，且每条都有字节
    #[test]
    fn bundled_fonts_are_non_empty() {
        assert!(!BUNDLED.is_empty());
        for (name, bytes) in BUNDLED {
            assert!(!name.is_empty());
            assert!(bytes.len() > 1000, "{name} 的字节数太小，像是占位");
        }
        assert_eq!(bundled_fonts().len(), BUNDLED.len());
    }

    /// 中文字体必须在打包列表里 —— 否则界面上的中文会退回默认字体
    #[test]
    fn cjk_font_is_bundled() {
        assert!(
            BUNDLED.iter().any(|(n, _)| n.contains("等线")),
            "打包字体里必须有等线：{:?}",
            BUNDLED.iter().map(|(n, _)| *n).collect::<Vec<_>>()
        );
    }
}
