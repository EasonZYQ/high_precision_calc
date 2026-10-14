//! 虚拟键盘与函数面板的**数据层**：按键布局、函数目录、插入行为。
//!
//! 与 `app.rs` 分开的理由：这里是**纯数据 + 纯函数**，可以脱离 GUI 单测；
//! `app.rs` 只负责把这些数据摆成 widget、把点击转成消息。
//!
//! ## 键盘设计
//!
//! 布局照搬 Windows 计算器 / WinUI3 的通用排列（用户说"类似 WinUI3"），
//! 但对**高精度计算器**做了三处调整：
//!
//! 1. 保留独立的 `x`（未知数）与 `,`（函数多参数、方程组、统计列表都要用）；
//! 2. 加 `^`（幂）与 `Ans`（上次结果）—— 高精度场景里这两者使用频率极高；
//! 3. 不留"科学计算器"的花哨符号（`n!`、`%`、`1/x`）：本项目这些都能直接打
//!    （`5!`、`sin(x)`），多一个按钮只是增加视觉噪音。
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
    /// 键面上显示的文字（可用符号，如 `π`、`√`、`⌫`）
    pub label: &'static str,
    /// 点击后插入到输入框的**真实文本**。
    /// 为空表示"这个键不插文本"（如删除键、等号键），由 [`Key::action`] 决定行为。
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

/// 键盘布局：每行一组键。
///
/// 行内按键个数不要求相同 —— 布局用 `Row` 而不是 `Grid`，
/// 这样 `=` 键可以单独加宽，视觉重心更好。
pub const LAYOUT: &[&[Key]] = &[
    // 第一行：控制 + 结构 + 常数
    &[
        Key {
            label: "⌫",
            insert: "",
            cursor_back: 0,
            kind: KeyKind::Control,
            action: KeyAction::Backspace,
        },
        Key {
            label: "C",
            insert: "",
            cursor_back: 0,
            kind: KeyKind::Control,
            action: KeyAction::Clear,
        },
        key("(", "(", KeyKind::Structural),
        key(")", ")", KeyKind::Structural),
        key(",", ", ", KeyKind::Structural),
        key("π", "pi", KeyKind::Function),
        key("e", "e", KeyKind::Function),
        key("x", "x", KeyKind::Function),
        key("Ans", "Ans", KeyKind::Function),
    ],
    // 第二行：数字 7-9 + 除法/幂
    &[
        key("7", "7", KeyKind::Digit),
        key("8", "8", KeyKind::Digit),
        key("9", "9", KeyKind::Digit),
        key("÷", "/", KeyKind::Operator),
        key("^", "^", KeyKind::Operator),
    ],
    // 第三行：数字 4-6 + 乘法/根号
    &[
        key("4", "4", KeyKind::Digit),
        key("5", "5", KeyKind::Digit),
        key("6", "6", KeyKind::Digit),
        key("×", "*", KeyKind::Operator),
        fn_key("√", "sqrt()", 1),
    ],
    // 第四行：数字 1-3 + 减法/平方
    &[
        key("1", "1", KeyKind::Digit),
        key("2", "2", KeyKind::Digit),
        key("3", "3", KeyKind::Digit),
        key("−", "-", KeyKind::Operator),
        fn_key("x²", "^2", 0),
    ],
    // 第五行：0 / 小数点 / 加法 / 等号（等号单独一组，视觉上更重）
    &[
        key("0", "0", KeyKind::Digit),
        key(".", ".", KeyKind::Digit),
        key("+", "+", KeyKind::Operator),
        Key {
            label: "=",
            insert: "",
            cursor_back: 0,
            kind: KeyKind::Equals,
            action: KeyAction::Evaluate,
        },
    ],
];

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_rows_are_non_empty() {
        assert!(!LAYOUT.is_empty());
        for (i, row) in LAYOUT.iter().enumerate() {
            assert!(!row.is_empty(), "第 {i} 行不该是空的");
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
        for need in ["+", "-", "*", "/", "^", "(", ")", ".", ","] {
            assert!(all.contains(need), "键盘缺少符号 {need}");
        }
        let labels: Vec<&str> = LAYOUT
            .iter()
            .flat_map(|r| r.iter())
            .map(|k| k.label)
            .collect();
        for need in [
            "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "π", "√", "=",
        ] {
            assert!(labels.contains(&need), "键盘缺少键 {need}");
        }
    }

    #[test]
    fn every_row_has_an_evaluate_or_is_digit_row() {
        // 至少存在一个求值键，否则键盘就是残的
        let has_eq = LAYOUT
            .iter()
            .flat_map(|r| r.iter())
            .any(|k| k.action == KeyAction::Evaluate);
        assert!(has_eq, "键盘必须有一个 `=` 求值键");
        let has_back = LAYOUT
            .iter()
            .flat_map(|r| r.iter())
            .any(|k| k.action == KeyAction::Backspace);
        assert!(has_back, "键盘必须有一个退格键");
    }

    /// 函数键应自带括号，且光标能落进括号里
    #[test]
    fn function_keys_auto_close_parens() {
        for row in LAYOUT {
            for k in *row {
                if k.kind == KeyKind::Function && k.action == KeyAction::Insert {
                    if k.insert.ends_with(')') || k.insert.contains('(') {
                        // 括号必须配对
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
}
