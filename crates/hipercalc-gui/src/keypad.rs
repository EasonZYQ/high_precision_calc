//! 虚拟键盘与函数面板的**数据层**：按键布局、函数目录、插入行为。
//!
//! 与 `app.rs` 分开的理由：这里是**纯数据 + 纯函数**，可以脱离 GUI 单测；
//! `app.rs` 只负责把这些数据摆成 widget、把点击转成消息。
//!
//! ## 键盘设计（照 Desmos 的布局）
//!
//! 布局模仿 Desmos 科学计算器的键盘，但按本项目的实际能力做了取舍。原文是
//! 两个**可切换的面板**：
//!
//! ```text
//! 主面板                                  符号/字母面板（点 `ABC` 进）
//! ┌────────────────────┬──────────┬────┐  ┌───────────────────────────────┐
//! │ x   y   a²  aᵇ     │ 7  8  9  │ƒ   │  │ q w e r t y u i o p           │
//! │ (   )   <   >      │ 4  5  6  │← → │  │ a s d f g h j k l θ           │
//! │ |a| ,   ≤   ≥      │ 1  2  3  │ ⌫  │  │   z x c v b n m               │
//! │ ABC 🔊  √   π      │ 0  .  =  │ ↵  │  │ 123 a_b !' [] {} ~% :          │
//! └────────────────────┴──────────┴────┘  └───────────────────────────────┘
//! ```
//!
//! ### 与 Desmos 的三处**有意偏离**
//!
//! Desmos 的键盘是为"画函数图像"设计的，我们不是，照抄会有几个键纯属噪音：
//!
//! 1. **`y` 键去掉了**：Desmos 用它画第二条曲线。本计算器只有 `x` 一个自变量，
//!    多发一个 `y` 只会让人以为能画两条线。位置换成本项目高频的 `e`（自然常数）。
//! 2. **`🔊` 换掉了**：那是 Desmos 的语音朗读。换成本项目高频的 `^`（幂）——
//!    高精度计算里 `2^500` 这类表达极其常用，比朗读键有用得多。
//! 3. **`<` `>` 换成了 `%` `!`**：不等号只在解不等式时用，且本项目的解不等式
//!    入口是 `solve` 而不是符号本身；`!`（阶乘）与 `%`（百分号）在计算器上使用频率
//!    高得多。**这一条要说明清楚**：`!` 是 Desmos 键盘上没有、我们特意加的。
//!
//! 保留的 Desmos 特征：`a²` / `aᵇ`（幂快捷）、`|a|`（绝对值）、`≤` `≥`、
//! `ABC` 面板切换、右侧独立的 `ƒ` / `←` `→` / `⌫` / `↵` 一列。
//!
//! ## 插入行为
//!
//! 按键文本分两类：
//! - **函数类**（`sin`、`sqrt`、`log`…）—— 插入后要**自动补一对括号并把光标停在括号内**，
//!   否则用户每次都得自己打 `(` 和 `)`。这类按键在 [`Key`] 里显式用 `insert` 字段描述。
//! - **普通类**（数字、运算符）—— 直接插入文本。

/// 按键的视觉分组（决定配色，见 `theme::KeyKind`）
pub use crate::theme::KeyKind;

/// 一个虚拟按键。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    /// 键面上显示的文字。
    ///
    /// 这里可以放**记号**（`π`、`√`、`≤`、`aᵇ`），它们由 `mathrender` 按数学字体排；
    /// 也可以放普通的字母数字。两者的字号在 `app::keypad_area` 里统一处理。
    pub label: &'static str,
    /// 点击后插入到输入框的**真实文本**。
    /// 为空表示"这个键不插文本"（如删除键、方向键、等号键），由 [`Key::action`] 决定行为。
    pub insert: &'static str,
    /// 插入后光标**额外左移**的格数。
    ///
    /// 这是"点一下 `sin` 就把光标放进括号里"的实现方式：
    /// 插入 `sin()`（5 个字符）后左移 1 格，光标就停在 `(` 与 `)` 之间。
    /// 用"插入完整文本 + 左移"而不是"插入 `sin(` 再等用户补右括号"，
    /// 是因为前者能立刻看到括号是配对的，用户不会漏掉。
    pub cursor_back: usize,
    /// 视觉分组
    pub kind: KeyKind,
    /// 行为（插文本之外的键）
    pub action: KeyAction,
}

/// 按键的行为类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    /// 按 `insert` / `cursor_back` 插入文本
    Insert,
    /// 退格
    Backspace,
    /// 清空整行
    Clear,
    /// 求值
    Evaluate,
    /// 光标左移一格（Desmos 的 `←`）
    CursorLeft,
    /// 光标右移一格（Desmos 的 `→`）
    CursorRight,
    /// 切到主面板（数字/符号）
    ShowMain,
    /// 切到字母面板（Desmos 的 `ABC`）
    ShowAbc,
    /// 插入上标 `^`（Desmos 的 `aᵇ`），随后光标停在指数位置
    Superscript,
    /// 打开函数面板（Desmos 键盘右侧那个 `functions` 键）
    ToggleFuncs,
}

/// 键盘面板
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Panel {
    /// 主面板：符号 + 数字（默认）
    #[default]
    Main,
    /// 字母面板（Desmos 的 `ABC`）
    Abc,
}

/// 构造一个"插入文本"的键
const fn key(label: &'static str, insert: &'static str, kind: KeyKind) -> Key {
    Key {
        label,
        insert,
        cursor_back: 0,
        kind,
        action: KeyAction::Insert,
    }
}

/// 构造一个"插入后光标回退"的键（用于函数：补全括号并把光标放到括号内）
const fn fn_key(label: &'static str, insert: &'static str, back: usize) -> Key {
    Key {
        label,
        insert,
        cursor_back: back,
        kind: KeyKind::Function,
        action: KeyAction::Insert,
    }
}

/// 构造一个"只做事、不插文本"的键
const fn act(label: &'static str, kind: KeyKind, action: KeyAction) -> Key {
    Key {
        label,
        insert: "",
        cursor_back: 0,
        kind,
        action,
    }
}

/// **主面板**布局：左侧符号区 + 中间数字区 + 右侧功能列。
///
/// 用 `&[&[Key]]` 而不是二维数组，是为了让每行长度可以不同 ——
/// 渲染时用 `Row`，`=` 与 `↵` 能单独加宽，视觉重心更稳。
pub const LAYOUT: &[&[Key]] = &[
    // ── 第 1 行：符号 / 数字7-9 / ƒ ────────────────────────────────────────
    &[
        key("x", "x", KeyKind::Function),
        key("e", "e", KeyKind::Function),
        fn_key("a²", "^2", 0),
        act("aᵇ", KeyKind::Function, KeyAction::Superscript),
        key("7", "7", KeyKind::Digit),
        key("8", "8", KeyKind::Digit),
        key("9", "9", KeyKind::Digit),
        key("÷", "/", KeyKind::Operator),
        act("ƒ", KeyKind::Function, KeyAction::ToggleFuncs),
    ],
    // ── 第 2 行：括号 / 数字4-6 / 光标 ────────────────────────────────────
    &[
        key("(", "(", KeyKind::Structural),
        key(")", ")", KeyKind::Structural),
        key("%", "%", KeyKind::Operator),
        key("!", "!", KeyKind::Operator),
        key("4", "4", KeyKind::Digit),
        key("5", "5", KeyKind::Digit),
        key("6", "6", KeyKind::Digit),
        key("×", "*", KeyKind::Operator),
        act("←", KeyKind::Control, KeyAction::CursorLeft),
    ],
    // ── 第 3 行：绝对值 / 数字1-3 / 右移 + 退格 ───────────────────────────
    &[
        key("|a|", "abs()", KeyKind::Function),
        key(",", ", ", KeyKind::Structural),
        key("≤", "<=", KeyKind::Operator),
        key("≥", ">=", KeyKind::Operator),
        key("1", "1", KeyKind::Digit),
        key("2", "2", KeyKind::Digit),
        key("3", "3", KeyKind::Digit),
        key("−", "-", KeyKind::Operator),
        act("→", KeyKind::Control, KeyAction::CursorRight),
    ],
    // ── 第 4 行：ABC / 数制 / 根号 / π / 数字0 / 等号 / 回车 ──────────────
    //
    // 这一行是主面板的"底边"，`↵` 在右下角、用强调色 —— 与 Desmos 一致。
    &[
        act("ABC", KeyKind::Control, KeyAction::ShowAbc),
        key("√", "sqrt()", KeyKind::Function),
        key("π", "pi", KeyKind::Function),
        key("^", "^", KeyKind::Operator),
        key("0", "0", KeyKind::Digit),
        key(".", ".", KeyKind::Digit),
        key("+", "+", KeyKind::Operator),
        Key {
            label: "⌫",
            insert: "",
            cursor_back: 0,
            kind: KeyKind::Control,
            action: KeyAction::Backspace,
        },
        Key {
            label: "↵",
            insert: "",
            cursor_back: 0,
            kind: KeyKind::Equals,
            action: KeyAction::Evaluate,
        },
    ],
];

/// **字母面板**布局（Desmos 的 `ABC` 面板）。
///
/// 字母键插的是**变量名**：`x` 之外的字母在项目里就是普通未知数/变量。
/// 最后一行右侧几个符号是 Desmos 有的（`[]` `{}` `:` 等），本项目里也都能用
/// （`[]` 是列表、`{}` 是集合/方程组分隔）。
pub const LAYOUT_ABC: &[&[Key]] = &[
    &[
        key("q", "q", KeyKind::Digit),
        key("w", "w", KeyKind::Digit),
        key("e", "e", KeyKind::Digit),
        key("r", "r", KeyKind::Digit),
        key("t", "t", KeyKind::Digit),
        key("y", "y", KeyKind::Digit),
        key("u", "u", KeyKind::Digit),
        key("i", "i", KeyKind::Digit),
        key("o", "o", KeyKind::Digit),
        key("p", "p", KeyKind::Digit),
    ],
    &[
        key("a", "a", KeyKind::Digit),
        key("s", "s", KeyKind::Digit),
        key("d", "d", KeyKind::Digit),
        key("f", "f", KeyKind::Digit),
        key("g", "g", KeyKind::Digit),
        key("h", "h", KeyKind::Digit),
        key("j", "j", KeyKind::Digit),
        key("k", "k", KeyKind::Digit),
        key("l", "l", KeyKind::Digit),
        key("θ", "theta", KeyKind::Function),
    ],
    &[
        act("↑", KeyKind::Control, KeyAction::Superscript),
        key("z", "z", KeyKind::Digit),
        key("x", "x", KeyKind::Digit),
        key("c", "c", KeyKind::Digit),
        key("v", "v", KeyKind::Digit),
        key("b", "b", KeyKind::Digit),
        key("n", "n", KeyKind::Digit),
        key("m", "m", KeyKind::Digit),
        Key {
            label: "⌫",
            insert: "",
            cursor_back: 0,
            kind: KeyKind::Control,
            action: KeyAction::Backspace,
        },
    ],
    &[
        act("123", KeyKind::Control, KeyAction::ShowMain),
        key("aᵇ", "^", KeyKind::Operator),
        key("!", "!", KeyKind::Operator),
        key("[", "[", KeyKind::Structural),
        key("]", "]", KeyKind::Structural),
        key("{", "{", KeyKind::Structural),
        key("}", "}", KeyKind::Structural),
        key(":", ":", KeyKind::Structural),
        Key {
            label: "↵",
            insert: "",
            cursor_back: 0,
            kind: KeyKind::Equals,
            action: KeyAction::Evaluate,
        },
    ],
];

/// 取某个面板的布局
pub fn layout_for(panel: Panel) -> &'static [&'static [Key]] {
    match panel {
        Panel::Main => LAYOUT,
        Panel::Abc => LAYOUT_ABC,
    }
}

/// 函数面板的一类（弹窗里按类别分节）
pub struct FnGroup {
    pub title: &'static str,
    pub items: &'static [FnItem],
}

/// 函数目录中的一项
pub struct FnItem {
    /// 插入的文本（含括号，光标会自动落进括号内）
    pub insert: &'static str,
    /// 键面显示（`sqrt` 显示成 `√` 更直观）
    pub label: &'static str,
    /// 简短说明（中文，鼠标悬停/列表里显示）
    pub about: &'static str,
}

const fn f(insert: &'static str, label: &'static str, about: &'static str) -> FnItem {
    FnItem {
        insert,
        label,
        about,
    }
}

/// 函数目录。**这是 GUI 端唯一的函数清单**，与 `parser::FUNCTIONS` 对应；
/// 只收录常用的一批（全量函数有上百个，弹窗里列全反而不便查找）。
pub const FN_GROUPS: &[FnGroup] = &[
    FnGroup {
        title: "三角",
        items: &[
            f("sin()", "sin", "正弦"),
            f("cos()", "cos", "余弦"),
            f("tan()", "tan", "正切"),
            f("asin()", "asin", "反正弦"),
            f("acos()", "acos", "反余弦"),
            f("atan()", "atan", "反正切"),
            f("sinh()", "sinh", "双曲正弦"),
            f("cosh()", "cosh", "双曲余弦"),
            f("tanh()", "tanh", "双曲正切"),
        ],
    },
    FnGroup {
        title: "对数与指数",
        items: &[
            f("log()", "log", "对数 log(base, x)"),
            f("ln()", "ln", "自然对数"),
            f("exp()", "exp", "指数 e^x"),
            f("sqrt()", "√", "平方根"),
            f("cbrt()", "∛", "立方根"),
            f("abs()", "|x|", "绝对值"),
        ],
    },
    FnGroup {
        title: "微积分",
        items: &[
            f("diff(,x)", "diff", "求导 diff(f, x)"),
            f("int(,x,0,1)", "int", "定积分 int(f,x,a,b)"),
            f("lim(,x,0)", "lim", "极限 lim(f, x, a)"),
            f("sum(,k,1,10)", "sum", "求和 sum(f,k,a,b)"),
            f("prod(,k,1,10)", "prod", "求积 prod(f,k,a,b)"),
        ],
    },
    FnGroup {
        title: "方程与多项式",
        items: &[
            f("fac()", "fac", "因式分解"),
            f("primefac()", "primefac", "素因数分解"),
            f("gcd(,)", "gcd", "最大公约数"),
            f("lcm(,)", "lcm", "最小公倍数"),
        ],
    },
    FnGroup {
        title: "统计",
        items: &[
            f("mean([])", "mean", "平均值 mean([1,2])"),
            f("median([])", "median", "中位数"),
            f("stddev([])", "stddev", "标准差 σ"),
            f("stddev_s([])", "stddev_s", "样本标准差 s"),
            f("var([])", "var", "方差"),
        ],
    },
    FnGroup {
        title: "数论与组合",
        items: &[
            f("comb(,)", "comb", "组合数 C(n,k)"),
            f("perm(,)", "perm", "排列数 P(n,k)"),
            f("fib()", "fib", "斐波那契数"),
            f("zeta()", "zeta", "黎曼 ζ 函数"),
        ],
    },
    FnGroup {
        title: "矩阵与几何",
        items: &[
            f("det([[]])", "det", "行列式 det([[1,2]])"),
            f("inv([[]])", "inv", "逆矩阵"),
            f("transpose([[]])", "transpose", "转置"),
            f("triangle()", "triangle", "解三角形"),
        ],
    },
];

/// 键盘里 `insert` 文本中**需要光标回退**的键，回退量 = 已补的右括号数。
///
/// 独立函数而不是写死字段：函数面板（[`FN_GROUPS`]）里的插入串也走同一套规则，
/// 两边共用才能保证"点键盘的 √" 和 "点函数面板的 √" 行为完全一致。
pub fn cursor_back_for(insert: &str) -> usize {
    insert.chars().rev().take_while(|c| *c == ')').count()
}

/// 把 `insert` 文本插到 `text` 的 `cursor` 位置。
///
/// `cursor` 是**字符**下标（不是字节）—— `π`、`√` 都是多字节字符，
/// 用字节下标会把文本切坏。返回 `(新文本, 新光标位置)`。
pub fn splice(text: &str, cursor: usize, insert: &str) -> (String, usize) {
    let chars: Vec<char> = text.chars().collect();
    let at = cursor.min(chars.len());
    let mut out: String = chars[..at].iter().collect();
    out.push_str(insert);
    out.extend(chars[at..].iter());
    let back = cursor_back_for(insert);
    let new_cursor = at + insert.chars().count() - back.min(insert.chars().count());
    (out, new_cursor)
}

/// 键面文字是否应当按**数学字体**（而非普通字体）排版。
///
/// Desmos 的键面是"数学记号"的观感：`a²` 的上标、`≤`、`π`、`√` 都是衬线数学字体。
/// 我们的做法是——含数学符号的键面交给 [`crate::mathrender`] 的字体栈渲染。
pub fn label_is_math(label: &str) -> bool {
    label.chars().any(|c| {
        matches!(
            c,
            'π' | '√'
                | '∛'
                | '∞'
                | '≈'
                | '≤'
                | '≥'
                | '≠'
                | '∑'
                | '∫'
                | '·'
                | '×'
                | '÷'
                | 'θ'
                | 'φ'
                | '²'
                | 'ᵇ'
                | '³'
                | '⌫'
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_rows_are_non_empty() {
        for (name, layout) in [("主", LAYOUT), ("字母", LAYOUT_ABC)] {
            assert!(!layout.is_empty(), "{name} 面板不该是空的");
            for (i, row) in layout.iter().enumerate() {
                assert!(!row.is_empty(), "{name} 面板第 {i} 行不该是空的");
            }
        }
    }

    /// 键盘必须带齐用户要求的几类符号
    #[test]
    fn layout_covers_required_symbols() {
        let all: String = LAYOUT
            .iter()
            .flat_map(|r| r.iter())
            .map(|k| k.insert)
            .collect::<Vec<_>>()
            .join("");
        for need in ["+", "-", "*", "/", "^", "(", ")", ".", ",", "!", "%"] {
            assert!(all.contains(need), "键盘缺少符号 {need}");
        }
        let labels: Vec<&str> = LAYOUT
            .iter()
            .flat_map(|r| r.iter())
            .map(|k| k.label)
            .collect();
        for need in [
            "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "π", "√", "↵", "ABC",
        ] {
            assert!(labels.contains(&need), "键盘缺少键 {need}");
        }
    }

    /// 数字 0-9 **每一个**都必须在主面板上能点到 —— 这是计算器的底线。
    ///
    /// 单独写一条：布局重排时最容易漏的就是"某个数字键被顶掉了"，
    /// 而上面那条只检查个别几个。
    #[test]
    fn all_ten_digits_are_present_on_main_panel() {
        for d in '0'..='9' {
            let s = d.to_string();
            let found = LAYOUT
                .iter()
                .flat_map(|r| r.iter())
                .any(|k| k.label == s && k.action == KeyAction::Insert);
            assert!(found, "主面板缺少数字键 {d}");
        }
    }

    #[test]
    fn every_panel_has_evaluate_and_backspace() {
        for (name, layout) in [("主", LAYOUT), ("字母", LAYOUT_ABC)] {
            let flat: Vec<&Key> = layout.iter().flat_map(|r| r.iter()).collect();
            assert!(
                flat.iter().any(|k| k.action == KeyAction::Evaluate),
                "{name} 面板必须有一个求值键"
            );
            assert!(
                flat.iter().any(|k| k.action == KeyAction::Backspace),
                "{name} 面板必须有一个退格键"
            );
        }
    }

    /// 两个面板必须能**互相切回去**，否则进字母面板就出不来了。
    #[test]
    fn panels_can_switch_both_ways() {
        let main_flat: Vec<&Key> = LAYOUT.iter().flat_map(|r| r.iter()).collect();
        let abc_flat: Vec<&Key> = LAYOUT_ABC.iter().flat_map(|r| r.iter()).collect();
        assert!(
            main_flat.iter().any(|k| k.action == KeyAction::ShowAbc),
            "主面板必须有进字母面板的键"
        );
        assert!(
            abc_flat.iter().any(|k| k.action == KeyAction::ShowMain),
            "字母面板必须有回主面板的键"
        );
    }

    /// 函数键应自带括号，且光标能落进括号里
    #[test]
    fn function_keys_auto_close_parens() {
        for layout in [LAYOUT, LAYOUT_ABC] {
            for row in layout {
                for k in *row {
                    if k.kind == KeyKind::Function
                        && k.action == KeyAction::Insert
                        && (k.insert.ends_with(')') || k.insert.contains('('))
                    {
                        let open = k.insert.chars().filter(|c| *c == '(').count();
                        let close = k.insert.chars().filter(|c| *c == ')').count();
                        assert_eq!(open, close, "键 {} 的括号不配对：{}", k.label, k.insert);
                    }
                }
            }
        }
    }

    /// **核心行为**：点 `sin` 后光标应停在括号内
    #[test]
    fn insert_puts_cursor_inside_parens() {
        let (t, c) = splice("", 0, "sqrt()");
        assert_eq!(t, "sqrt()");
        // `sqrt()` 共 6 字符，回退 1 ⇒ 光标在 5，即 `(` 与 `)` 之间
        assert_eq!(c, 5, "光标应停在括号内，实际 {c}");
        let chars: Vec<char> = t.chars().collect();
        assert_eq!(chars[c - 1], '(');
        assert_eq!(chars[c], ')');
    }

    #[test]
    fn cursor_back_matches_trailing_parens() {
        assert_eq!(cursor_back_for("sin()"), 1);
        assert_eq!(cursor_back_for("sum(,k,1,10)"), 1);
        // `det([[]])` 结尾是 `])` —— 连续的右括号只有 **1** 个（`]` 不是 `)`），
        // 所以只回退 1 格。这里曾把断言写成 3，是把 `]` 也算进去了。
        assert_eq!(cursor_back_for("det([[]])"), 1);
        // 真正的连续右括号：`f(())` 结尾两个都是 `)`
        assert_eq!(cursor_back_for("f(())"), 2);
        assert_eq!(cursor_back_for("7"), 0);
        assert_eq!(cursor_back_for("x"), 0);
    }

    /// 插入到**中间**（不是末尾）必须正确 —— 这是虚拟键盘最容易写错的地方
    #[test]
    fn splice_inserts_at_cursor_not_at_end() {
        // 文本 "1+2"，光标在 1（`+` 前）→ 插入 `9` 应得 "19+2"
        let (t, c) = splice("1+2", 1, "9");
        assert_eq!(t, "19+2");
        assert_eq!(c, 2);
    }

    /// 多字节字符（π、√）必须按字符下标处理，不能按字节
    #[test]
    fn splice_handles_multibyte_chars() {
        // "π+1"，光标在 1 → 插入 `2` 应得 "π2+1"
        let (t, c) = splice("π+1", 1, "2");
        assert_eq!(t, "π2+1");
        assert_eq!(c, 2);

        // 插入 π 本身
        let (t, c) = splice("1+", 2, "π");
        assert_eq!(t, "1+π");
        assert_eq!(c, 3);
    }

    #[test]
    fn splice_clamps_out_of_range_cursor() {
        // 光标超出长度时夹到末尾（界面同步有延迟时可能发生）
        let (t, c) = splice("12", 99, "3");
        assert_eq!(t, "123");
        assert_eq!(c, 3);
    }

    #[test]
    fn fn_groups_are_non_empty_and_parens_balanced() {
        assert!(!FN_GROUPS.is_empty());
        for g in FN_GROUPS {
            assert!(!g.title.is_empty());
            assert!(!g.items.is_empty(), "函数分组 {} 是空的", g.title);
            for it in g.items {
                let open = it.insert.chars().filter(|c| *c == '(').count();
                let close = it.insert.chars().filter(|c| *c == ')').count();
                assert_eq!(
                    open, close,
                    "{} 的插入串括号不配对：{}",
                    it.label, it.insert
                );
                assert!(!it.about.is_empty(), "{} 缺少说明", it.label);
            }
        }
    }

    /// 函数面板里每一项插入后，光标都应落在**最内层括号**内
    #[test]
    fn fn_panel_insert_lands_inside_innermost_parens() {
        for g in FN_GROUPS {
            for it in g.items {
                let (t, c) = splice("", 0, it.insert);
                let chars: Vec<char> = t.chars().collect();
                assert!(c <= chars.len());
                // 光标处（或其右侧）应能找到一个右括号，说明确实在括号内
                assert!(
                    chars[c..].contains(&')'),
                    "{} 插入后光标不在括号内：{:?} @ {c}",
                    it.label,
                    t
                );
            }
        }
    }

    /// 键面里的数学记号要被识别出来（决定用哪套字体渲染）
    #[test]
    fn math_labels_are_recognized() {
        for l in ["π", "√", "≤", "≥", "a²", "aᵇ", "×", "÷", "θ", "⌫"] {
            assert!(label_is_math(l), "{l} 应被认作数学记号");
        }
        // 普通字母数字不该走数学字体（否则按键上的 `7` 会和别处不一样）
        for l in ["7", "x", "q", "ABC", "123"] {
            assert!(!label_is_math(l), "{l} 不该被认作数学记号");
        }
    }

    /// 布局里每个键的 `insert` 都必须能被解析器接受（否则点了会立刻报错）
    ///
    /// ⚠️ 键盘上的键分成**三类**，每类要补的上下文不同，得分开判 —— 一刀切地
    /// 拼个前后缀会把本来就完整的键拼坏（第一版用 `1{insert}1`，
    /// 把 `abs()` 拼成 `1abs()1`）。
    ///
    /// | 类别 | 例子 | 判定 | 补什么 |
    /// |---|---|---|---|
    /// | 函数调用模板 | `abs()`、`sqrt()`、`sin()` | 光标会落在括号内，用户接着打参数 | 往最内层括号里塞个 `1` |
    /// | 后缀片段 | `^2`、`!`、`%` | 要左邻当底数 | 前缀 `1` → `1^2` |
    /// | 中缀片段 | `/`、`+`、`=` | 右边缺操作数 | 两边都补 → `1/1` |
    ///
    /// ⚠️ 最后这一类是**最容易误判的**：`abs()` 裸着试会报
    /// `位置 4 处意外的字符: ')'` —— 看着像语法错，其实是解析器把
    /// "**参数为空**"这句话也说成了"意外的字符"。而 `abs()` 恰恰是这个键
    /// **故意**插入的形态：光标落在括号里，用户接下来就打字。所以要
    /// 把参数补上再判（`abs(1)`）。
    ///
    /// 判法是**挨个试上下文，任意一个能过就算过**。这样就不用维护
    /// "哪些键是什么类"的表 —— 表会随布局改动而过期。
    ///
    /// 仍然豁免"未定义变量"—— 空会话里 `x` 当然没定义，那是**正确**行为。
    #[test]
    fn every_key_insert_is_parseable_or_empty() {
        // 只把**真正的语法错**当失败。"未定义变量"/"未定义函数"是正确行为：
        // 空会话里当然没有 `x`，也没注册 `foobar`。
        //
        // ⚠️ `')'` 那条要单独放行：它是"参数为空"的措辞，而空参正是函数键的
        // 预期插入形态。见上面注释里的表格。
        let is_syntax_error =
            |t: &str| (t.contains("意外的字符") && !t.contains("')'")) || t.contains("未预期的");

        /// 把 `1` 塞进**最内层**的 `()`（`abs()` → `abs(1)`、`sqrt(sqrt())` →
        /// `sqrt(sqrt(1))`）。没有 `()` 就原样返回。
        fn fill_innermost(s: &str) -> String {
            match s.rfind("()") {
                Some(i) => format!("{}1{}", &s[..i + 1], &s[i + 1..]),
                None => s.to_string(),
            }
        }

        for layout in [LAYOUT, LAYOUT_ABC] {
            for row in layout {
                for k in *row {
                    // 控制类键（清空 / 退格 / 面板切换…）本来就不插文本
                    if k.action != KeyAction::Insert || k.insert.is_empty() {
                        continue;
                    }
                    // 四种候选上下文，**任意一种能解析就算通过**：
                    // ① 裸插（完整式子）② 括号里补参数（函数模板）
                    // ③ 前补底数（后缀片段）④ 前后都补（中缀片段）
                    let candidates = [
                        k.insert.to_string(),
                        fill_innermost(k.insert),
                        format!("1{}", k.insert),
                        format!("1{}1", k.insert),
                    ];
                    let ok = candidates.iter().any(|c| {
                        let mut s = hipercalc::Session::new();
                        !is_syntax_error(&s.eval(c).text)
                    });
                    assert!(
                        ok,
                        "键 {} 的插入串 `{}` 在四种上下文（`{}` / `{}` / `1{}` / `1{}1`）下都解析不了",
                        k.label,
                        k.insert,
                        k.insert,
                        fill_innermost(k.insert),
                        k.insert,
                        k.insert
                    );
                }
            }
        }
    }
}
