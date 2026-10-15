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

use iced::widget::{Column, Row, Space, container, rule, text};
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
    let mut p = Parser {
        toks: tokens,
        pos: 0,
    };
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
            match self.parse_primary() {
                Some(exp) => base = Node::Pow(Box::new(base), Box::new(exp)),
                // ⚠️ **指数缺失**（用户正打到 `x^`、`2^`）时不能连底数一起丢掉。
                //
                // 原来这里写的是 `let exp = self.parse_primary()?;` —— `?` 直接把
                // 整个 `parse_atom` 变成 `None`，调用处于是把**已经解析好的底数也扔了**，
                // 退回 `raw_token()`。实测症状：输入框里打 `x^` 的瞬间 `x` 消失，
                // 打完数字才回来 —— 打字过程中字符"闪掉"是很容易被当成 bug 的。
                //
                // 正确做法：保留底数，并把还没打出来的指数位置留成一个空的 `Pow`，
                // 这样预览里能看到"上标框是空的"，与 Desmos 一致。
                None => {
                    base = Node::Pow(Box::new(base), Box::new(Node::Text(String::new())));
                    break;
                }
            }
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
                    // ⚠️ **不能**把函数名后面的 `(...)` 单独当普通括号组解析 ——
                    // 那样括号会被当成"纯分组"而**丢掉**，`sin(theta)` 会被排成
                    // `sintheta`（实测就是如此：预览行里函数名与参数粘成一坨）。
                    // 所以函数名后面若跟着 `(`，这里要**自己吃下整个调用**并保留括号。
                    _ => {
                        let name = render_ident(&name);
                        if matches!(self.peek(), Some(Tok::LParen)) {
                            let (inner, consumed) = self.parse_call_args();
                            debug_assert!(consumed, "已经 peek 到 LParen，必然能吃下");
                            let mut seq = Vec::with_capacity(inner.len() + 2);
                            seq.push(Node::Text(name));
                            seq.push(Node::Text("(".into()));
                            seq.extend(inner);
                            seq.push(Node::Text(")".into()));
                            Some(Node::Seq(seq))
                        } else {
                            Some(Node::Text(name))
                        }
                    }
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

/// 标识符 → 显示符号
fn render_ident(name: &str) -> String {
    match name {
        "pi" => "π".into(),
        "inf" => "∞".into(),
        "INF" => "∞".into(),
        // `theta` → `θ`：虚拟键盘的字母面板上**有**一个 `θ` 键，它插进去的正是
        // `theta`（ASCII 好让解析器认）。那预览行就该把它显示成 `θ` ——
        // 否则用户点 `θ` 键、预览里冒出来一串 `theta`，像是键坏了。
        "theta" => "θ".into(),
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
    // 数学符号（π、√、∞…）在等宽字体里会掉进"数学斜体"字形，与周围的直立数字
    // 格格不入（第一版快照里 `π` 明显比其他字符斜）。含这些符号时改用默认字体。
    if style.monospace && !contains_math_glyph(src) {
        t = t.font(Font::MONOSPACE);
    }
    t.into()
}

/// 文本里是否含"等宽字体渲染会变形"的数学符号
fn contains_math_glyph(s: &str) -> bool {
    s.chars().any(|c| {
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
        )
    })
}

/// 递归把节点变成 widget。
///
/// 公开是为了让调用方**先 parse 再 build**，从而摆脱 `&str` 的生命周期约束：
/// [`render`] 的返回值会借用入参（它内部要 `src.to_string()`），
/// 入参若是局部变量就没法把结果返回出去。分成两步后，
/// [`Node`] 拥有自己的字符串，借它渲染出来的 widget 生命周期就自由了。
pub fn build<'a, M: 'a>(node: &Node, style: Style2) -> Element<'a, M> {
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
            // 竖排：分子 / 横线 / 分母。
            // ⚠️ `Rule` **没有 `.width()` 方法**（第一版写错过）—— 它内部厚度是 `Length`，
            // 但长度只能由父容器决定。所以把 rule 放进一个定宽容器里，靠容器控制宽度。
            // 分数用小一号字（标准数学排版惯例：分数内的字比正文小）
            let inner_style = Style2 {
                size: style.size * 0.72,
                ..style
            };
            let num_el = build(num, inner_style);
            let den_el = build(den, inner_style);
            // 横线宽度取分子/分母中较长者的近似宽度：用字符数 × 字号估一个
            // （iced 没有"测量文字宽度"的公开接口，这是务实的近似）
            let w = frac_bar_width(num, den, inner_style.size);
            let bar = container(
                // Rule 没有 `.width()`；放进定宽容器后它会自动撑满容器宽度
                rule::horizontal(Pixels(1.6)).style(theme::math_rule),
            )
            .width(Length::Fixed(w));

            let c = Column::new()
                .width(Length::Shrink)
                .align_x(Alignment::Center)
                .push(num_el)
                .push(Space::new().height(Pixels(style.size * 0.08)))
                .push(bar)
                .push(Space::new().height(Pixels(style.size * 0.08)))
                .push(den_el);
            wrap_inline(c.into(), style)
        }
        Node::Root { degree, inner } => {
            // 根号 = `√` 字符 + 被开方式 + **上方**一条顶线。
            //
            // ⚠️ 顶线在**上**、被开方式在**下** —— 第一版把 `Column` 的 push 顺序写反了，
            // 结果横线跑到数字下面（看起来像下划线）。`Column` 是自上而下排列，
            // 所以必须先 push 顶线。
            let radicand = build(inner, style);
            let top_line = container(
                // `Rule` 没有 `.width()`；放进定宽容器后它会自动撑满容器宽度
                rule::horizontal(Pixels(1.6)).style(theme::math_rule),
            )
            .width(Length::Fixed(node_width(inner, style.size) + 4.0));
            let topped = Column::new()
                .align_x(Alignment::Start)
                .push(top_line)
                .push(radicand);
            // `√` 贴在数字左侧，底端与数字基线对齐
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
            r = r.push(plain(
                "√",
                Style2 {
                    size: style.size * 1.15,
                    ..style
                },
            ));
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

// ─────────────────────────────────────────────────────────────────────────────
// 宽度估算
// ─────────────────────────────────────────────────────────────────────────────

/// 一个字符的**平均**宽度 ÷ 字号。
///
/// iced 没有公开"测量文字宽度"的接口（要拿宽度得走 `Renderer` 内部），
/// 而分数横线与根号顶线**必须**有个宽度才能画出来。这里用一个经验比例估算：
/// 0.56 对拉丁字母/数字是偏保守的取值（略宽于真实等宽字体），
/// 结果是横线稍微长出内容一点点 —— 这比"太短、盖不住分子"的观感好得多。
const CHAR_WIDTH_RATIO: f32 = 0.56;

/// 估算一个节点占的宽度（像素）
fn node_width(node: &Node, size: f32) -> f32 {
    match node {
        Node::Text(s) => s.chars().count() as f32 * size * CHAR_WIDTH_RATIO,
        Node::Seq(items) => items.iter().map(|i| node_width(i, size)).sum(),
        // 分数：取分子/分母较宽者（横线宽度），外加一点内边距
        Node::Frac(num, den) => {
            let inner = size * 0.72;
            let w = node_width(num, inner).max(node_width(den, inner));
            (w + size * 0.30).max(size * 0.85)
        }
        // 根号：`√` 本体 + 被开方式 + 顶线出头
        Node::Root { inner, degree } => {
            let d = degree
                .as_ref()
                .map(|d| node_width(d, size * 0.55))
                .unwrap_or(0.0);
            d + size * 0.78 + node_width(inner, size) + 4.0
        }
        // 幂：底数 + 上标（字号更小）
        Node::Pow(base, exp) => node_width(base, size) + node_width(exp, size * 0.62),
    }
}

/// 分数横线宽度：分子与分母中较宽者，再留一点两侧余量（更接近印刷排版）
fn frac_bar_width(num: &Node, den: &Node, size: f32) -> f32 {
    let w = node_width(num, size).max(node_width(den, size));
    (w + size * 0.24).max(size * 0.7)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **数学模式下空格会被吃掉** —— 这是设计如此（`2 x` 与 `2x` 应排得一样），
    /// 但**调用方必须知道**：凡是要原样显示的文本（错误信息、提示语）
    /// **不能**走数学排版，否则空格没了、`' '` 会挤成 `''`。
    ///
    /// 这条测试的作用就是把"吃掉空格"钉成**已知的、被依赖的行为**，
    /// 免得以后有人"顺手"改成保留空格、把分数的判据改坏（`1 / 2` 会变成
    /// 带空格的分数节点，`Node::Frac` 里的 `Text` 会多出前后空格）。
    #[test]
    fn math_mode_drops_spaces_by_design() {
        // 分数两侧的空格不影响识别
        match parse("1 / 2") {
            Node::Frac(n, d) => {
                assert_eq!(*n, Node::Text("1".into()), "分子不该带空格");
                assert_eq!(*d, Node::Text("2".into()), "分母不该带空格");
            }
            other => panic!("应是分数，实际 {other:?}"),
        }

        // 错误信息里的 `' '`（引号包着一个空格）会被排成两个紧挨的引号 ——
        // 正是这个现象让界面上的错误信息看起来像 `..."`。
        // 所以错误信息必须走纯文本（见 `app::history_entry`）。
        let node = parse("错误: 位置 2 处意外的字符: ' '");
        let flat = flatten(&node);
        assert!(
            !flat.contains(' '),
            "数学排版应吃掉空格，实际排出了：{flat:?}"
        );
    }

    /// **函数调用的括号必须保住**（阶段五在预览行里发现的真 bug）。
    ///
    /// 症状：输入框里打 `sin(theta)`，下面的预览行排成 `sintheta` ——
    /// 函数名与参数粘在一起。原因是 `Ident("sin")` 生成 `Text("sin")` 之后，
    /// 后面那个 `(` 被 `parse_primary` 当**普通分组括号**吃掉了，而分组的括号
    /// 是**不渲染**的（`(x)` 就是要显示成 `x`），于是 `(` `)` 双双消失。
    ///
    /// 单看 `sqrt` 是发现不了这个 bug 的 —— `sqrt` 有自己的分支，压根不走这条路。
    /// 所以这条测试特意用 `sin` / `log` / 普通字母函数来覆盖**默认分支**。
    #[test]
    fn function_call_keeps_its_parentheses() {
        for (src, want) in [
            ("sin(theta)", "sin(θ)"),
            ("sin(x)", "sin(x)"),
            ("log(2)", "log(2)"),
            // 纯分组括号仍应**不带**括号（这是分组该有的样子）
            ("(x)", "x"),
            ("(1/2)", "1/2"),
        ] {
            let got = flatten(&parse(src));
            assert_eq!(got, want, "`{src}` 应排成 `{want}`，实际 `{got}`");
        }
    }

    /// 纯文本模式必须**一个字符都不动**（错误信息、LineIO 都靠它）
    #[test]
    fn plain_mode_preserves_every_character() {
        // `plain` 直接包 `text(src)`，不经 tokenizer，所以只要比对源码文本即可
        for src in [
            "错误: 位置 2 处意外的字符: ' '",
            "≈ 0.8333333333",
            "= 1 / 2",
            "未定义变量: x",
        ] {
            // 构造出来不该 panic，且内容（除字体/颜色外）不回炉重排
            let _: Element<()> = plain(
                src,
                Style2 {
                    size: 15.0,
                    color: None,
                    monospace: false,
                },
            );
        }
    }

    /// 把节点树拍平成字符串（测试断言用）
    fn flatten(n: &Node) -> String {
        match n {
            Node::Text(s) => s.clone(),
            Node::Seq(items) => items.iter().map(flatten).collect(),
            Node::Frac(a, b) => format!("{}/{}", flatten(a), flatten(b)),
            Node::Root { degree, inner } => match degree {
                Some(d) => format!("{}({})", flatten(d), flatten(inner)),
                None => format!("√({})", flatten(inner)),
            },
            Node::Pow(a, b) => format!("{}^{}", flatten(a), flatten(b)),
        }
    }

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
        assert_eq!(
            items[0],
            Node::Text("-".into()),
            "首个元素应是负号：{items:?}"
        );
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
            Node::Root {
                degree: None,
                inner,
            } => {
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
            items
                .iter()
                .filter(|i| matches!(i, Node::Root { .. }))
                .count(),
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
            "1 / 2",
            "",
            "(",
            ")",
            "/",
            "1/",
            "/2",
            "sqrt(",
            "sqrt",
            "*",
            "^",
            "^2",
            "-1 / 4 + (1 / 4)*sqrt(5)",
            "1e-5",
            "0.5",
            "x/y/z",
            "((1/2))",
            "2.5e10",
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

    /// 含数学符号的文本不能走等宽字体（否则 π/√ 会变成斜体衬线字形）
    #[test]
    fn math_glyphs_skip_monospace() {
        assert!(contains_math_glyph("π"));
        assert!(contains_math_glyph("2π"));
        assert!(contains_math_glyph("√2"));
        assert!(contains_math_glyph("1 · 2"));
        assert!(!contains_math_glyph("0.8333333"));
        assert!(!contains_math_glyph("1 / 2"));
        assert!(!contains_math_glyph("sin(x)"));
    }

    /// 根号顶线的**宽度估算**必须盖住被开方式（否则线短于数字，观感很差）。
    ///
    /// 这条测试是针对一个真实 bug 的回归护栏：顶线与被开方式的**上下顺序**
    /// 曾在渲染代码里被写反（线跑到数字下面）。顺序无法在纯函数里断言，
    /// 但宽度是能断言的；顺序由 `snap` 快照（`dark-wgpu.png` 里的 `√2 + √3`）把关。
    #[test]
    fn root_top_line_covers_radicand() {
        for s in [
            "sqrt(2)",
            "sqrt(12)",
            "sqrt(123456)",
            "sqrt(x+y)",
            "sqrt(2.5)",
        ] {
            let Node::Root { inner, .. } = parse(s) else {
                panic!("{s} 应解析成根号");
            };
            let inner_w = node_width(&inner, 24.0);
            let line_w = node_width(&inner, 24.0) + 4.0;
            assert!(
                line_w > inner_w,
                "{s}: 顶线宽 {line_w} 必须大于被开方式宽 {inner_w}"
            );
        }
    }
}
