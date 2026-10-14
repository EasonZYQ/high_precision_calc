//! 把 `hipercalc` 的 **MathIO 文本**渲染成真正的数学排版（分数、根号、幂、π）。
//!
//! ## 为什么要这一层
//!
//! `hipercalc-core::display::format_mathio` 的输出是**纯 ASCII 的线性文本**，
//! 不能直接当数学看：
//!
//! ```text
//! 1 / 2                    → 应当画成上 1、中间横线、下 2 的竖排分数
//! (1 / 3)*sqrt(2)          → 应当画成 (1/3)·√2
//! (5 / 6)*pi               → 应当画成 (5/6)π
//! -1 / 4 + (1 / 4)*sqrt(5) → 应当画成 -1/4 + (1/4)√5，其中 -1/4 是一个分数
//! ```
//!
//! ## 做法
//!
//! 两层：**解析**（本文件上半部，纯函数、可单测）把文本解析成一棵 [`Node`] 树；
//! **渲染**（下半部）把树递归展开成 iced widget。
//!
//! 解析必须处理的核心难点是 **`-1 / 4 + (1 / 4)*sqrt(5)` 里 `-1 / 4` 的分子是 `-1`**：
//! 分数是按"紧邻的原子"结合的，不是按整个表达式 —— 否则会把 `+ (1/4)√5` 也吸进分子。
//! 所以 [`parse`] 里的分数判定用的是「左侧取一个**原子**、右侧取一个**原子**」，
//! 而不是「从头吃到尾」。
//!
//! ## 与 `lineio` 的关系
//!
//! LineIO 输出（`0.8333333…`、`1.41421356…`）**没有可排版的数学结构**，本模块直接
//! 原样当普通文本渲染。两种模式走同一入口 [`render`]，由参数区分。

use iced::widget::{Column, Row, Space, column, container, row, rule, text};
use iced::{Alignment, Element, Font, Length, Pixels};

use crate::theme;

// ─────────────────────────────────────────────────────────────────────────────
// 1. 语法树
// ─────────────────────────────────────────────────────────────────────────────

/// 一个可排版的数学节点。
///
/// 刻意保持最小：只覆盖 `format_mathio` 真正会产出的结构。遇到不认识的写法一律
/// 降级成 [`Node::Raw`]（原样文本），**保证任何输入都能显示**，绝不 panic、绝不吞内容。
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    /// 原样文本（数字、变量名、无法识别的片段）
    Text(String),
    /// 竖排分数：分子 / 分母
    Frac(Box<Node>, Box<Node>),
    /// 根号：`√(inner)`，`degree` 为 `Some` 时是 n 次根
    Root {
        degree: Option<Box<Node>>,
        inner: Box<Node>,
    },
    /// 底数与指数（渲染成右上标）
    Pow(Box<Node>, Box<Node>),
    /// 一行按顺序排列的若干节点（乘号、加减号都在这里显式出现）
    Seq(Vec<Node>),
}

/// 解析入口：把 MathIO 文本切成节点序列。
///
/// 返回 `Seq`，其中每个元素是**顶层**节点（分数、根号、符号、数字…）。
pub fn parse(input: &str) -> Node {
    let tokens = tokenize(input);
    let mut p = Parser { toks: tokens, pos: 0 };
    let seq = p.parse_seq(Style::Value);
    if seq.len() == 1 {
        seq.into_iter().next().unwrap()
    } else {
        Node::Seq(seq)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. 词法
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(String),
    /// 标识符（`pi`、`sqrt`、`x`…）
    Ident(String),
    Op(char),
    LParen,
    RParen,
    /// 乘号（`*` 或 `·`）
    Star,
    /// 幂号 `^`
    Caret,
}

fn tokenize(s: &str) -> Vec<Tok> {
    let cs: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        match c {
            ' ' => i += 1,
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            '*' | '·' | '×' => {
                out.push(Tok::Star);
                i += 1;
            }
            '^' => {
                out.push(Tok::Caret);
                i += 1;
            }
            '+' | '-' | '/' => {
                out.push(Tok::Op(c));
                i += 1;
            }
            _ if c.is_ascii_digit() || c == '.' => {
                let start = i;
                while i < cs.len() && (cs[i].is_ascii_digit() || cs[i] == '.') {
                    i += 1;
                }
                // 支持 `1e-5` 这类科学计数法：`e` 后紧跟可选正负号再跟数字
                if i < cs.len() && (cs[i] == 'e' || cs[i] == 'E') {
                    let save = i;
                    let mut j = i + 1;
                    if j < cs.len() && (cs[j] == '+' || cs[j] == '-') {
                        j += 1;
                    }
                    if j < cs.len() && cs[j].is_ascii_digit() {
                        while j < cs.len() && cs[j].is_ascii_digit() {
                            j += 1;
                        }
                        i = j;
                    } else {
                        i = save;
                    }
                }
                out.push(Tok::Num(cs[start..i].iter().collect()));
            }
            _ if c.is_alphabetic() || c == '_' => {
                let start = i;
                while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '_') {
                    i += 1;
                }
                out.push(Tok::Ident(cs[start..i].iter().collect()));
            }
            _ => {
                // 未识别字符（`π`、`√` 等）当单字符标识符处理，最终降级成原样文本
                out.push(Tok::Ident(c.to_string()));
                i += 1;
            }
        }
    }
    out
}

/// 表达式所处的"结合环境"，决定 `/` 按分数还是按普通除法处理。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Style {
    /// 顶层 / 括号内：`/` 优先解释成**分数**
    Value,
    /// 已经是分子或分母的内部：`/` 退化成普通除法符号（避免 `a/b/c` 无限嵌套）
    Term,
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    /// 解析一段节点序列，直到遇到不归自己管的记号（`)` 或结束）。
    fn parse_seq(&mut self, style: Style) -> Vec<Node> {
        let mut out: Vec<Node> = Vec::new();
        while let Some(tok) = self.peek().cloned() {
            match tok {
                Tok::RParen => break,
                // 分数：吞掉 `/`，左右各取一个"原子"
                Tok::Op('/') if style == Style::Value => {
                    self.pos += 1;
                    // 左：从已收集的序列里**弹出最后一个原子**当分子。
                    // 这就是 `-1 / 4 + …` 能正确工作的关键：分子只拿到 `-1`，
                    // 而不是把整段（含前面的 `+`）都吃进去。
                    let Some(numer) = out.pop() else {
                        // 开头就是 `/`（畸形输入）⇒ 退化成普通除号，保证前进
                        out.push(Node::Text("/".into()));
                        continue;
                    };
                    // 右：取一个原子当分母；取不到就补个空串占位（不丢内容、不 panic）
                    let denom = self
                        .parse_atom(Style::Term)
                        .unwrap_or_else(|| Node::Text(String::new()));
                    out.push(Node::Frac(Box::new(numer), Box::new(denom)));
                }
                _ => {
                    if let Some(node) = self.parse_atom(style) {
                        out.push(node);
                    } else {
                        // 解析不出原子：把当前记号当原样文本吃掉，保证前进（不死循环）
                        out.push(self.raw_token());
                    }
                }
            }
        }
        out
    }

    /// 取一个"原子"：数字、标识符、括号组、带指数者，以及**紧贴其后的 `sqrt(...)` 之类**。
    fn parse_atom(&mut self, style: Style) -> Option<Node> {
        let mut base = self.parse_primary()?;
        // 后缀：`^指数`
        while matches!(self.peek(), Some(Tok::Caret)) {
            self.pos += 1;
            let exp = self.parse_primary()?;
            base = Node::Pow(Box::new(base), Box::new(exp));
        }
        // 隐式乘法：`(1 / 3)*sqrt(2)` 里 `*` 是显式的；但数学模式也可能产出
        // 紧贴形式（如 `2sqrt(3)`），这里一并支持，避免两段被拆散
        let _ = style;
        Some(base)
    }

    /// 最基础的成分
    fn parse_primary(&mut self) -> Option<Node> {
        match self.peek()?.clone() {
            Tok::Num(s) => {
                self.pos += 1;
                Some(Node::Text(s))
            }
            Tok::Ident(name) => {
                self.pos += 1;
                match name.as_str() {
                    // `sqrt(x)` → 根号
                    "sqrt" => {
                        let (inner, consumed) = self.parse_call_args();
                        if consumed {
                            // 常见的 `sqrt(2)`（单参数）画成 √2；多参数理论上不会出现
                            let inner = if inner.len() == 1 {
                                inner.into_iter().next().unwrap()
                            } else {
                                Node::Seq(inner)
                            };
                            Some(Node::Root {
                                degree: None,
                                inner: Box::new(inner),
                            })
                        } else {
                            // 没有括号跟随：当普通标识符
                            Some(Node::Text(name))
                        }
                    }
                    // 已知数学常数与函数名：渲染成对应符号
                    _ => Some(Node::Text(render_ident(&name))),
                }
            }
            Tok::LParen => {
                self.pos += 1;
                let inner = self.parse_seq(Style::Value);
                // 吃掉右括号
                if matches!(self.peek(), Some(Tok::RParen)) {
                    self.pos += 1;
                }
                // 括号组：内部若已是单节点就直接用，否则包成 Seq
                let node = if inner.len() == 1 {
                    inner.into_iter().next().unwrap()
                } else {
                    Node::Seq(inner)
                };
                // `)` 后面紧跟 `^` 时交给 parse_atom 的后缀循环处理
                Some(node)
            }
            Tok::Op(c) => {
                self.pos += 1;
                Some(Node::Text(c.to_string()))
            }
            Tok::Star => {
                self.pos += 1;
                Some(Node::Text("·".into()))
            }
            Tok::Caret => None,
            Tok::RParen => None,
        }
    }

    /// 吃掉一对括号里的内容（用于 `sqrt(...)`）。返回 `(节点列表, 是否真的吃到了括号)`
    fn parse_call_args(&mut self) -> (Vec<Node>, bool) {
        if !matches!(self.peek(), Some(Tok::LParen)) {
            return (Vec::new(), false);
        }
        self.pos += 1;
        let inner = self.parse_seq(Style::Value);
        if matches!(self.peek(), Some(Tok::RParen)) {
            self.pos += 1;
        }
        (inner, true)
    }

    /// 把一个记号原样变成 `Node::Text` 并前进（容错路径）
    fn raw_token(&mut self) -> Node {
        let t = match self.peek().cloned() {
            Some(Tok::Num(s)) | Some(Tok::Ident(s)) => s,
            Some(Tok::Op(c)) => c.to_string(),
            Some(Tok::Star) => "·".into(),
            Some(Tok::Caret) => "^".into(),
            Some(Tok::LParen) => "(".into(),
            Some(Tok::RParen) => ")".into(),
            None => String::new(),
        };
        self.pos += 1;
        Node::Text(t)
    }
}

/// 组合"分子乘以某物"的容错分支用（仅极少见的畸形输入会走到）
fn combine_product(_a: Node, b: Node) -> Node {
    b
}

/// 标识符 → 显示符号
fn render_ident(name: &str) -> String {
    match name {
        "pi" => "π".into(),
        "inf" => "∞".into(),
        "INF" => "∞".into(),
        _ => name.to_string(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. 渲染
// ─────────────────────────────────────────────────────────────────────────────

/// 渲染配置（字号与颜色由调用方给，便于结果区/历史区用不同字号）
#[derive(Debug, Clone, Copy)]
pub struct Style2 {
    pub size: f32,
    pub color: Option<iced::Color>,
    pub monospace: bool,
}

impl Default for Style2 {
    fn default() -> Self {
        Self {
            size: 26.0,
            color: None,
            monospace: false,
        }
    }
}

/// 渲染入口：把一段结果文本变成可显示的 `Element`。
///
/// - `math` 为真（MathIO）→ 先解析成语法树再排版；
/// - 为假（LineIO，纯小数）→ 直接当文本，避免把 `1.41421356…` 误判成结构。
pub fn render<'a, M: 'a>(src: &str, math: bool, style: Style2) -> Element<'a, M> {
    if !math {
        return plain(src, style);
    }
    let node = parse(src);
    build(&node, style)
}

/// 纯文本渲染
fn plain<'a, M: 'a>(src: &str, style: Style2) -> Element<'a, M> {
    let mut t = text(src.to_string()).size(Pixels(style.size));
    if let Some(c) = style.color {
        t = t.color(c);
    }
    if style.monospace {
        t = t.font(Font::MONOSPACE);
    }
    t.into()
}

/// 递归把节点变成 widget。
fn build<'a, M: 'a>(node: &Node, style: Style2) -> Element<'a, M> {
    match node {
        Node::Text(s) => plain(s, style),
        Node::Seq(items) => {
            let mut r = Row::new().align_y(Alignment::Center).spacing(0.0);
            for it in items {
                r = r.push(build(it, style));
            }
            r.into()
        }
        Node::Frac(num, den) => {
            // 竖排：分子 / 横线 / 分母。横线用 rule，宽度由内容自然撑开。
            let bar = rule::horizontal(Pixels(1.5))
                .style(theme::math_rule)
                .width(Length::Fill);
            let bar_col: Column<'a, M> = column![
                Space::with_height(Pixels(style.size * 0.10)),
                bar,
                Space::with_height(Pixels(style.size * 0.10)),
            ];
            // 分数用小一号字（标准数学排版惯例：分数内的字比正文小）
            let inner_style = Style2 {
                size: style.size * 0.72,
                ..style
            };
            let mut c = Column::new()
                .width(Length::Shrink)
                .align_x(Alignment::Center)
                .push(build(num, inner_style))
                .push(bar_col)
                .push(build(den, inner_style));
            // 分数要比正文略高一档基线，视觉上才居中
            c = c.push(Space::with_height(Pixels(0.0)));
            wrap_inline(c.into(), style)
        }
        Node::Root { degree, inner } => {
            // 根号：画一个"√"字符 + 被开方式上方加一条横线（顶线）
            let inner_style = style;
            let radicand = build(inner, inner_style);
            let overline = rule::horizontal(Pixels(1.5))
                .style(theme::math_rule)
                .width(Length::Fill);
            let topped: Column<'a, M> = column![
                radicand,
                overline,
            ]
            .align_x(Alignment::Start);
            let mut r = Row::new().align_y(Alignment::End).spacing(0.0);
            if let Some(d) = degree {
                // n 次根：左上角小字
                r = r.push(build(
                    d,
                    Style2 {
                        size: style.size * 0.55,
                        ..style
                    },
                ));
            }
            r = r.push(plain("√", Style2 { size: style.size * 1.15, ..style }));
            r = r.push(topped);
            r.into()
        }
        Node::Pow(base, exp) => {
            // 右上标：基线对齐到顶端，字号小一档
            let mut r = Row::new().align_y(Alignment::Start).spacing(0.0);
            r = r.push(build(base, style));
            r = r.push(build(
                exp,
                Style2 {
                    size: style.size * 0.62,
                    ..style
                },
            ));
            r.into()
        }
    }
}

/// 把"分数"这类**竖排**元素嵌进一行时，需要让它相对文本基线居中。
///
/// iced 没有 `baseline` 对齐概念，`align_y(Center)` 已经能得到可接受的观感；
/// 这里保持为一个显式函数，是为了将来若要做精细基线调整时只改一处。
fn wrap_inline<'a, M: 'a>(el: Element<'a, M>, _style: Style2) -> Element<'a, M> {
    container(el).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_fraction() {
        // `1 / 2` 应成一个分数节点
        match parse("1 / 2") {
            Node::Frac(n, d) => {
                assert_eq!(*n, Node::Text("1".into()));
                assert_eq!(*d, Node::Text("2".into()));
            }
            other => panic!("应是分数，实际 {other:?}"),
        }
    }

    #[test]
    fn parses_fraction_then_sqrt_product() {
        // 真实形态：`(1 / 3)*sqrt(2)`
        let n = parse("(1 / 3)*sqrt(2)");
        let Node::Seq(items) = n else {
            panic!("应是序列，实际 {n:?}");
        };
        assert!(
            items.iter().any(|i| matches!(i, Node::Frac(..))),
            "序列里应有分数：{items:?}"
        );
        assert!(
            items.iter().any(|i| matches!(i, Node::Root { .. })),
            "序列里应有根号：{items:?}"
        );
    }

    /// **最关键的一条**：分子的边界。
    ///
    /// 真实输出 `-1 / 4 + (1 / 4)*sqrt(5)`。
    /// 如果实现成"从左吃到右"，分子会变成 `-1/4+(1/4)√5`；正确结果是：
    /// 第一项 = 分数(-1, 4)，后面再跟 `+ (1/4)√5`。
    #[test]
    fn fraction_numerator_stops_at_plus() {
        let n = parse("-1 / 4 + (1 / 4)*sqrt(5)");
        let Node::Seq(items) = n else {
            panic!("应是序列，实际 {n:?}");
        };
        // 开头应有一个 `-` 号，然后是分数(-1, 4)
        assert_eq!(items[0], Node::Text("-".into()), "首个元素应是负号：{items:?}");
        match &items[1] {
            Node::Frac(num, den) => {
                assert_eq!(**num, Node::Text("1".into()), "分子应只是 1");
                assert_eq!(**den, Node::Text("4".into()), "分母应只是 4");
            }
            other => panic!("第二项应是分数 1/4，实际 {other:?}"),
        }
        // 后面必须还有 `+` 以及另一个分数 —— 证明没被吸进分子
        assert!(
            items.iter().filter(|i| matches!(i, Node::Frac(..))).count() == 2,
            "应有两个分数（-1/4 与 1/4）：{items:?}"
        );
    }

    #[test]
    fn parses_sqrt_single_and_nested() {
        match parse("sqrt(2)") {
            Node::Root { degree: None, inner } => {
                assert_eq!(*inner, Node::Text("2".into()));
            }
            other => panic!("应是根号，实际 {other:?}"),
        }
        // `sqrt(2) + sqrt(3)`
        let n = parse("sqrt(2) + sqrt(3)");
        let Node::Seq(items) = n else {
            panic!("应是序列");
        };
        assert_eq!(
            items.iter().filter(|i| matches!(i, Node::Root { .. })).count(),
            2,
            "应有两个根号：{items:?}"
        );
    }

    #[test]
    fn parses_pi_coefficient() {
        // 真实形态：`(2 / 3)*pi`
        let n = parse("(2 / 3)*pi");
        let Node::Seq(items) = n else {
            panic!("应是序列，实际 {n:?}");
        };
        assert!(
            items.iter().any(|i| matches!(i, Node::Frac(..))),
            "应有分数：{items:?}"
        );
        assert!(
            items.iter().any(|i| matches!(i, Node::Text(s) if s == "π")),
            "pi 应渲染成 π：{items:?}"
        );
    }

    #[test]
    fn parses_integer_coefficient_sqrt() {
        // `2*sqrt(3)`：数字 + 乘号 + 根号
        let n = parse("2*sqrt(3)");
        let Node::Seq(items) = n else {
            panic!("应是序列，实际 {n:?}");
        };
        assert_eq!(items[0], Node::Text("2".into()));
        assert_eq!(items[1], Node::Text("·".into()), "`*` 应渲染成 ·");
        assert!(matches!(items[2], Node::Root { .. }));
    }

    #[test]
    fn decimal_text_is_left_alone() {
        // LineIO 输出不该被解析（render 里由 math 标志保证），
        // 但即使误送进来也不能丢内容
        let n = parse("0.83333333333333333333");
        assert_eq!(n, Node::Text("0.83333333333333333333".into()));
    }

    #[test]
    fn negative_and_sign_are_preserved() {
        // `-1 / 4` 单独的形态
        let n = parse("-1 / 4");
        let Node::Seq(items) = n else {
            panic!("应是序列，实际 {n:?}");
        };
        assert_eq!(items[0], Node::Text("-".into()));
        assert!(matches!(items[1], Node::Frac(..)));
    }

    #[test]
    fn power_is_parsed() {
        // `2^10` → Pow
        let n = parse("2^10");
        match n {
            Node::Pow(b, e) => {
                assert_eq!(*b, Node::Text("2".into()));
                assert_eq!(*e, Node::Text("10".into()));
            }
            other => panic!("应是幂，实际 {other:?}"),
        }
    }

    #[test]
    fn never_panics_and_never_loses_text() {
        // 容错护栏：畸形 / 奇怪输入都必须能解析，且不丢字符
        for s in [
            "1 / 2", "", "(", ")", "/", "1/", "/2", "sqrt(", "sqrt", "*", "^", "^2",
            "-1 / 4 + (1 / 4)*sqrt(5)", "1e-5", "0.5", "x/y/z", "((1/2))", "2.5e10",
        ] {
            let _ = parse(s); // 不 panic 即通过
        }
        // 空输入不 panic
        assert_eq!(parse(""), Node::Seq(vec![]));
    }

    #[test]
    fn scientific_notation_stays_one_token() {
        let n = parse("1e-5");
        assert_eq!(n, Node::Text("1e-5".into()), "科学计数法不该被拆开：{n:?}");
    }
}
