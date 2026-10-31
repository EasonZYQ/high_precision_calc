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

use iced::widget::canvas::{Frame, Geometry, LineCap, LineJoin, Path, Stroke};
use iced::widget::{Canvas, Column, Row, Space, container, rule, text};
use iced::{Alignment, Element, Length, Padding, Pixels, Point, Rectangle, Renderer};

use crate::theme;

// ─────────────────────────────────────────────────────────────────────────────
// 1. 语法树
// ─────────────────────────────────────────────────────────────────────────────

/// 一个可排版的数学节点。
///
/// 刻意保持最小：只覆盖 `format_mathio` 真正会产出的结构。遇到不认识的写法一律
/// 降级成 `Node::Raw`（原样文本），**保证任何输入都能显示**，绝不 panic、绝不吞内容。
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
    /// 底数与**下标**（渲染成右下标，如 `log_b` 的 b）。
    ///
    /// 与 [`Node::Pow`]（上标）成对：排版规则同款，只是对齐方向相反 ——
    /// 上标顶对齐、下标底对齐。目前唯一的生产者是 `log` 的底数框。
    Sub(Box<Node>, Box<Node>),
    /// 一行按顺序排列的若干节点（乘号、加减号都在这里显式出现）
    Seq(Vec<Node>),
    /// **括号里的空参数**：不画方框，只留一个几像素宽的透明区（可点）。
    ///
    /// 用户 2026-10-27："括号里就不需要加方框了，空括号会报错"。
    /// 括号本身已经把"这里可以填"表达清楚了，再套一个灰框反而喧宾夺主；
    /// 但**点击目标不能省** —— 否则空括号里点不进去、只能用方向键。
    EmptyBare { at: usize },
    /// **用户还没输入的空参数** —— 渲染成灰色方块（Desmos 风格）。
    ///
    /// `at` 是它在**源文本里的字符偏移**：点这个方块时要知道"光标该落在哪"。
    /// 这是本模块唯一带位置信息的节点，因为只有它需要被点。
    Empty { at: usize },
    /// **括号分组**（左右一对**可缩放**定界符 + 内容）。
    ///
    /// 为什么单独一个变体：`(` `)` 当普通字符时**高度固定**，旁边是分数/Σ 时
    /// 括号只有一行高，看起来像"括号没包住"。正规排版里定界符要按内容高度拉伸
    /// （LaTeX 的 `\left( ... \right)`），所以这里用 canvas 手绘两条弧线，
    /// 高度取 `node_height(inner)`。
    Paren { inner: Box<Node> },
    /// **聚焦中的空参数框**：框 + 画在框**里面**的光标。
    ///
    /// 为什么不直接用 `Seq[Caret, Empty]`：那样光标落在框**左边外面**，
    /// 视觉上仍是"光标在框外"。用户的要求是"框至少要能囊括光标的高度"，
    /// 所以聚焦时空框变成"框 + 框内光标"，看起来就是"这个空位正在被编辑"。
    EmptyFocused { at: usize },
    /// **输入光标**（一根竖线）。
    ///
    /// 输入区改成"预览即输入栏"之后，背后的 `text_editor` 被透明化，
    /// 它自己的光标也跟着看不见了（iced 的光标用的就是 `style.value` 的颜色，
    /// 见 `iced_widget` 的 `text_editor::draw`）。所以光标由排版层自己画：
    /// 解析时按光标偏移插一个 `Caret`，**位置交给布局引擎算** ——
    /// 比自己去量字形宽度可靠得多。
    Caret,
    /// Σ / Π 这类**大运算符**（上下限竖排在大符号上方/下方）。
    BigOp {
        kind: BigKind,
        /// 被运算的表达式
        body: Box<Node>,
        /// 求和/求积变量（`k`）
        var: Box<Node>,
        /// 下限
        from: Box<Node>,
        /// 上限
        to: Box<Node>,
    },
    /// **带源位置的包装**：让这棵子树可被点击（点了就把光标送到 `at`）。
    ///
    /// ## 为什么点击定位必须靠它
    ///
    /// 背后的 `text_editor` 也会处理点击，但它是按**纯文本字形**算位置的 ——
    /// 而用户看到的是排版层（`√2` 的宽度和 `sqrt(2)` 完全不同）。
    /// 于是"想改 Σ 上面的 10，点下去光标却跑到别处"，用户的原话是
    /// **"完全无法选中修改"**。
    ///
    /// 所以位置必须来自**解析时的源偏移**：解析时给每个顶层节点包一层 `At`，
    /// 渲染成一个可点区域。这样"点哪 → 光标去哪"是我们自己说了算的，精确。
    At { at: usize, inner: Box<Node> },
    /// **字符级点击目标**（结构化输入框用）—— 与 [`Node::At`] 的唯一区别是
    /// **不加内边距**。
    ///
    /// ## 为什么要单独一个变体
    ///
    /// 结构化输入框把每个记号都做成一个点击目标（点它 ⇒ 光标落在它**之前**），
    /// 于是"点哪个字就在哪儿插"就成立了。而 [`Node::At`] 会给每块补
    /// `padding([2, 1])` —— 逐个记号都加的话，字符之间会被撑出缝，
    /// 整个算式看起来是散的。这里只包一层 `mouse_area`，不动布局。
    Hit { at: usize, inner: Box<Node> },
    /// **参数分隔符**（内部节点，渲染成空）。
    ///
    /// 本模块的解析器是为"排版"写的，它并不懂函数签名 —— 逗号在它眼里只是一个
    /// 普通字符。而 `sum(f, k, a, b)` 要按逗号切成 4 个参数，切分时要能说出
    /// "每个参数从源文本的哪个偏移开始"（空参数框才知道点击后光标该去哪）。
    /// 所以逗号单独成一个节点、并把偏移带在身上，切分完就丢掉。
    Comma { at: usize },
}

/// 大运算符的种类（都是"一个符号 + 上下限 + 主体"的排版家族）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigKind {
    /// `sum(f, k, a, b)` → Σ
    Sum,
    /// `prod(f, k, a, b)` → Π
    Prod,
    /// `int(f, x, a, b)` → ∫（上下限竖排，右侧 `f dx`）
    Integral,
    /// `lim(f, x, a)` → `lim` 下方写 `x→a`
    Limit,
    /// `diff(f, x)` → `d/dx` 竖排 + 主体
    Diff,
}

/// 解析入口：把 MathIO 文本切成节点序列。
///
/// 返回 `Seq`，其中每个元素是**顶层**节点（分数、根号、符号、数字…）。
pub fn parse(input: &str) -> Node {
    // ⚠️ **不包位置层**：这是"纯解析"入口，历史区、结果区与所有单测都用它。
    // 需要点击定位的输入区走 `parse_with_caret`（见那里的说明）。
    parse_impl(input, None, false)
}

/// 带光标的解析：`caret` 是光标在源文本里的**字符偏移**（`None` = 不画光标）。
///
/// 光标会作为 [`Node::Caret`] 插进对应的位置，于是它的横坐标由 iced 的布局
/// 引擎算出来 —— 我们不需要知道任何字体的字形宽度。
pub fn parse_with_caret(input: &str, caret: Option<usize>) -> Node {
    // 输入区专用：**包位置层**，这样每个顶层节点都能被点击定位
    parse_impl(input, caret, true)
}

/// 解析实现（`wrap` 决定要不要给节点包 [`Node::At`]）
fn parse_impl(input: &str, caret: Option<usize>, wrap: bool) -> Node {
    let (tokens, offs) = tokenize(input);
    let mut p = Parser {
        toks: tokens,
        offs,
        pos: 0,
        caret,
        total: input.chars().count(),
        wrap,
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

fn tokenize(s: &str) -> (Vec<Tok>, Vec<usize>) {
    let cs: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    // 与 `out` 一一对应：每个 token 在**源文本里的字符偏移**。
    // 光标与空参数框都要靠它映回源文本（见 `Node::Caret` / `Node::Empty`）。
    let mut offs: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        match c {
            ' ' => i += 1,
            '(' => {
                offs.push(i);
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                offs.push(i);
                out.push(Tok::RParen);
                i += 1;
            }
            '*' | '·' | '×' => {
                offs.push(i);
                out.push(Tok::Star);
                i += 1;
            }
            '^' => {
                offs.push(i);
                out.push(Tok::Caret);
                i += 1;
            }
            '+' | '-' | '/' => {
                offs.push(i);
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
                offs.push(start);
                out.push(Tok::Num(cs[start..i].iter().collect()));
            }
            _ if c.is_alphabetic() || c == '_' => {
                let start = i;
                while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '_') {
                    i += 1;
                }
                offs.push(start);
                out.push(Tok::Ident(cs[start..i].iter().collect()));
            }
            _ => {
                // 未识别字符（`π`、`√` 等）当单字符标识符处理，最终降级成原样文本
                offs.push(i);
                out.push(Tok::Ident(c.to_string()));
                i += 1;
            }
        }
    }
    (out, offs)
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
    /// 与 `toks` 一一对应：每个 token 在**源文本里的字符偏移**
    offs: Vec<usize>,
    pos: usize,
    /// 还没被安置的光标偏移；插进树里之后就置 `None`，保证只插一次
    caret: Option<usize>,
    /// 源文本的字符总数（判断"光标在末尾"要用）
    total: usize,
    /// 是否给节点包上**位置层**（[`Node::At`]，让它们可被点击）。
    ///
    /// ⚠️ 只有**输入区**需要它。历史区是只读的（`on_placeholder` 传 `None`），
    /// 包了反而会让节点树的形状变复杂 —— 而测试都是按"裸结构"断言的
    ///（`assert_eq!(seq[0], Node::Text("-"))` 这类），包上会让它们全部失配。
    wrap: bool,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    /// 当前解析位置对应的**源文本偏移**（没有更多 token 时就是输入末尾）
    fn here(&self) -> usize {
        self.offs.get(self.pos).copied().unwrap_or(self.total)
    }

    /// 光标正好落在当前位置时，把它插进节点流（只插一次）。
    ///
    /// 插进去的是一根**真实参与布局的竖线**，所以它的横坐标由 iced 算 ——
    /// 我们不需要任何字体的字形宽度（那是排版层最不该去猜的东西）。
    fn take_caret(&mut self, out: &mut Vec<Node>) {
        if let Some(c) = self.caret
            && c == self.here()
        {
            out.push(Node::Caret);
            self.caret = None;
        }
    }

    /// 光标落在**标识符**内部时，把它对齐到这个 token 的边界（而不是拆开）。
    ///
    /// ## 为什么不能拆
    ///
    /// 拆开的话 `sum` 就变成 `s|um` 两段文本 —— 解析器再也认不出这是求和函数，
    /// Σ 瞬间退化成普通字符（用户实测：光标用方向键移进 `sum` 内部，Σ 变成了 "sum"）。
    ///
    /// 而对齐到边界之后：函数名保持完整（Σ 照常渲染），光标画在它**右边** ——
    /// 偏差不到一个函数名的宽度，而排版保住了。这个取舍是值得的。
    fn align_caret_to_boundary(&mut self, at: usize, len: usize) {
        if let Some(c) = self.caret
            && c > at
            && c < at + len
        {
            self.caret = Some(at + len);
        }
    }

    /// 光标落在 `s` **内部**时把文本拆成三段（前 / 光标 / 后）。
    ///
    /// 返回 `None` 表示光标不在内部（在端点上的情况由 [`Parser::take_caret`] 负责，
    /// 那才是常规路径）。没有这一步的话，`12|3` 这种"光标在数字中间"就画不出光标 ——
    /// 因为 tokenizer 把 `123` 当成**一个** token，光标根本不在任何 token 边界上。
    fn split_inside(&mut self, s: &str, at: usize) -> Option<Node> {
        let c = self.caret?;
        let len = s.chars().count();
        if c <= at || c >= at + len {
            return None;
        }
        let k = c - at;
        self.caret = None;
        let head: String = s.chars().take(k).collect();
        let tail: String = s.chars().skip(k).collect();
        Some(Node::Seq(vec![
            Node::Text(head),
            Node::Caret,
            Node::Text(tail),
        ]))
    }

    /// 解析一段节点序列，直到遇到不归自己管的记号（`)` 或结束）。
    fn parse_seq(&mut self, style: Style) -> Vec<Node> {
        let mut out: Vec<Node> = Vec::new();
        loop {
            self.take_caret(&mut out);
            let Some(tok) = self.peek().cloned() else {
                break;
            };
            match tok {
                Tok::RParen => break,
                // 分数：吞掉 `/`，左右各取一个"原子"
                Tok::Op('/') if style == Style::Value => {
                    self.pos += 1;
                    // ⚠️ 先把"刚插进来的光标"摘出来：它是**位置标记**，不是分子的一部分。
                    //
                    // 光标正好停在 `/` 前面时（`1|/2`），`take_caret` 会在这一轮循环
                    // 开头把它推进 `out`，于是下面那句 `out.pop()` 会把**光标**当成
                    // 分子 pop 走 —— 分子没了，光标还跑到了树外面。实测就是这个症状。
                    let mut pending_caret = None;
                    while matches!(out.last(), Some(Node::Caret)) {
                        pending_caret = out.pop();
                    }
                    // 左：从已收集的序列里**弹出最后一个原子**当分子。
                    // 这就是 `-1 / 4 + …` 能正确工作的关键：分子只拿到 `-1`，
                    // 而不是把整段（含前面的 `+`）都吃进去。
                    let Some(numer) = out.pop() else {
                        // 开头就是 `/`（畸形输入）⇒ 退化成普通除号，保证前进
                        if let Some(c) = pending_caret {
                            out.push(c);
                        }
                        out.push(Node::Text("/".into()));
                        continue;
                    };
                    // 光标放回分数**之前**（视觉上就该在这儿：它停在分子右边）
                    if let Some(c) = pending_caret {
                        out.push(c);
                    }
                    // 右：取一个原子当分母；取不到就放一个**空占位框**
                    //（用户正打到 `1/`，Desmos 里这个位置也是一个灰方块，
                    //  而不是什么都没有 —— 那是"这里还缺个东西"的唯一提示）
                    let at = self.here();
                    let denom = self.parse_atom(Style::Term).unwrap_or(Node::Empty { at });
                    out.push(Node::Frac(Box::new(numer), Box::new(denom)));
                }
                _ => {
                    let at = self.here();
                    if let Some(node) = self.parse_atom(style) {
                        // 只在交互路径包位置层（见 `Parser::wrap` 的说明）
                        out.push(if self.wrap { wrap_at(node, at) } else { node });
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
                // 正确做法：保留底数，并把还没打出来的指数位置留成一个**空占位框**，
                // 这样预览里能看到"上标框是空的"，与 Desmos 一致。
                None => {
                    let at = self.here();
                    base = Node::Pow(Box::new(base), Box::new(Node::Empty { at }));
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
                let at = self.here();
                self.pos += 1;
                // 光标在数字**内部**（`12|3`）时拆开，否则光标没地方画
                match self.split_inside(&s, at) {
                    Some(n) => Some(n),
                    None => Some(Node::Text(s)),
                }
            }
            Tok::Ident(name) => {
                let at = self.here();
                self.pos += 1;
                // 逗号：只服务于 `sum/prod` 的参数切分，排版时不显示（见 `Node::Comma`）
                if name == "," {
                    return Some(Node::Comma { at });
                }
                // ⚠️ 光标在标识符内部时**不拆**（拆了函数识别就毁了，Σ 会变成 "sum"），
                // 对齐到这个 token 的末尾 —— 见 `align_caret_to_boundary` 的说明
                self.align_caret_to_boundary(at, name.chars().count());
                match name.as_str() {
                    // 反三角：`acos(0.5)` 要排成 `cos⁻¹(0.5)` —— 与**输入栏**一致。
                    //
                    // 为什么在这里做：输入框走 `expr` 树（自有映射），而历史回显、
                    // 函数面板走的是**文本解析**。两边各认一套名字，就会出现
                    // "输入栏 cos⁻¹、计算栏 acos"（用户 2026-10-27 实测）。
                    "asin" | "acos" | "atan" | "acot" | "asec" | "acsc" | "arcsin" | "arccos"
                    | "arctan" | "arccot" | "arcsec" | "arccsc" => {
                        let base = match name.trim_start_matches("arc") {
                            "asin" | "sin" => "sin",
                            "acos" | "cos" => "cos",
                            "atan" | "tan" => "tan",
                            "acot" | "cot" => "cot",
                            "asec" | "sec" => "sec",
                            "acsc" | "csc" => "csc",
                            _ => "sin",
                        };
                        let (inner, consumed) = self.parse_call_args();
                        if !consumed {
                            return Some(Node::Text(name));
                        }
                        let arg = if inner.len() == 1 {
                            inner.into_iter().next().unwrap()
                        } else if inner.is_empty() {
                            Node::Empty {
                                at: at + name.chars().count() + 1,
                            }
                        } else {
                            Node::Seq(inner)
                        };
                        Some(Node::Seq(vec![
                            Node::Pow(
                                Box::new(Node::Text(base.to_string())),
                                Box::new(Node::Text("−1".to_string())),
                            ),
                            Node::Paren {
                                inner: Box::new(arg),
                            },
                        ]))
                    }
                    // `sqrt(x)` → 根号
                    "sqrt" => {
                        // `(` 的位置：`at` 是 `sqrt` 的起点，跳过 4 个字符就是括号
                        let inner_at = at + "sqrt".len();
                        let (inner, consumed) = self.parse_call_args();
                        if consumed {
                            // 常见的 `sqrt(2)`（单参数）画成 √2；多参数理论上不会出现
                            let inner = if inner.len() == 1 {
                                inner.into_iter().next().unwrap()
                            } else if inner.is_empty() {
                                // `sqrt()` —— 里面还没输入：放一个**空占位框**。
                                // 点它就把光标送进括号（Desmos 的行为），
                                // 而不是让用户对着一个空荡荡的根号发呆。
                                Node::Empty { at: inner_at + 1 }
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
                    // `cbrt(x)` → ∛（带次数 3 的根号）；`nroot(x, n)` → ⁿ√
                    //
                    // 与 `sqrt` 同一套"矢量根号"渲染，只是 degree 不为空。
                    // nroot 的参数序与 core 一致：`nroot(被开方式, 次数)`
                    //（实测 `nroot(27,3) = 3`）。
                    "cbrt" | "nroot" => {
                        let inner_at = at + name.len();
                        let (inner, consumed) = self.parse_call_args();
                        if !consumed {
                            // 没有括号跟随：当普通标识符
                            return Some(Node::Text(name));
                        }
                        let e = || Node::Empty { at: inner_at + 1 };
                        let (degree, radix) = if name == "cbrt" {
                            (Node::Text("3".into()), inner)
                        } else {
                            // nroot(x, n)：第一个参数是被开方式、第二个是次数
                            let mut it = split_args(inner, inner_at + 1).into_iter();
                            let radix = it.next().unwrap_or_else(e);
                            let degree = it.next().unwrap_or_else(e);
                            (degree, vec![radix])
                        };
                        let radix = match radix.len() {
                            1 => radix.into_iter().next().unwrap(),
                            // `nroot(,3)`（被开方式还没打）—— 空占位框
                            0 => e(),
                            _ => Node::Seq(radix),
                        };
                        Some(Node::Root {
                            degree: Some(Box::new(degree)),
                            inner: Box::new(radix),
                        })
                    }
                    // `sum(f, k, a, b)` → Σ / `prod(...)` → Π
                    //
                    // 语法来自 core 的 `sum` / `prod`（见 `keypad::FN_GROUPS`：
                    // `sum(f,k,a,b)`）。四个参数的含义：
                    // 被运算式 / 变量 / 下限 / 上限。
                    // `∑` / `∏` 与 `sum` / `prod` 等价 —— 键盘插入的是**符号本身**
                    // （用户要求：不要用函数名，否则光标在 `sum` 中间移动时会把它
                    // 拆成 `sumx`，排版立刻崩成文本）。
                    "sum" | "prod" | "int" | "lim" | "diff" | "∑" | "∏" => {
                        // ⚠️ 用**字符数**而不是 `name.len()`（字节数）：
                        // `∑` 的 UTF-8 是 3 字节，用字节算偏移会把它后面的位置全带偏。
                        let inner_at = at + name.chars().count();
                        let (inner, consumed) = self.parse_call_args();
                        if !consumed {
                            return Some(Node::Text(name));
                        }
                        let kind = match name.as_str() {
                            "sum" | "∑" => BigKind::Sum,
                            "prod" | "∏" => BigKind::Prod,
                            "int" => BigKind::Integral,
                            "lim" => BigKind::Limit,
                            _ => BigKind::Diff,
                        };
                        let args = split_args(inner, inner_at + 1);
                        let mut it = args.into_iter();
                        // 参数不全（用户还在打）时补空占位框，而不是放弃整个结构 ——
                        // 一打 `sum(` 就能看到 Σ 出来，这才是"所见即所得"。
                        let e = || Node::Empty { at: inner_at + 1 };
                        let body = it.next().unwrap_or_else(e);
                        let var = it.next().unwrap_or_else(e);
                        let from = it.next().unwrap_or_else(e);
                        let to = it.next().unwrap_or_else(e);
                        Some(Node::BigOp {
                            kind,
                            body: Box::new(body),
                            var: Box::new(var),
                            from: Box::new(from),
                            to: Box::new(to),
                        })
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

/// 给节点包一层"源位置"（可点击），但**已经知道位置的节点不重复包**。
///
/// - `Empty` 自己带 `at`，且它已经渲染成可点的虚线框；
/// - `Caret` 是位置标记本身；
/// - `Comma` 只服务于参数切分、渲染成空，包了反而会挡住 `split_args` 的识别。
fn wrap_at(node: Node, at: usize) -> Node {
    match node {
        Node::Empty { .. } | Node::Caret | Node::Comma { .. } | Node::At { .. } => node,
        other => Node::At {
            at,
            inner: Box::new(other),
        },
    }
}

/// 把 `sum(f, k, a, b)` 的参数按**顶层逗号**切成若干段。
///
/// `open` 是第一个参数在源文本里的起点（即 `(` 之后），空参数框靠它定位 ——
/// 点了空框之后光标要落在"那个参数该在的位置"。
fn split_args(nodes: Vec<Node>, open: usize) -> Vec<Node> {
    let mut out: Vec<Node> = Vec::new();
    let mut cur: Vec<Node> = Vec::new();
    let mut seg_at = open;
    for n in nodes {
        if let Node::Comma { at } = n {
            out.push(collapse(cur, seg_at));
            cur = Vec::new();
            seg_at = at + 1;
            continue;
        }
        cur.push(n);
    }
    out.push(collapse(cur, seg_at));
    out
}

/// 把一段节点收成一个：空的 → [`Node::Empty`]，单个 → 它自己，多个 → [`Node::Seq`]
fn collapse(nodes: Vec<Node>, at: usize) -> Node {
    match nodes.len() {
        0 => Node::Empty { at },
        1 => nodes.into_iter().next().unwrap(),
        _ => Node::Seq(nodes),
    }
}

/// 标识符 → 显示符号
///
/// `pub(crate)`：结构化输入框（[`crate::expr`]）也要用同一套映射 ——
/// 它把 `Text("pi")` 送进渲染 IR 时必须变成 `π`，否则输入框里会显示成 `pi`、
/// 而历史区显示 `π`，同一个东西两种样子。
pub(crate) fn render_ident(name: &str) -> String {
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
    /// 超长内容要不要**折行**（`false` = 不折，靠横向滚动看全）。
    ///
    /// ⚠️ 折行必须用 `Wrapping::Glyph`，**不能**用 iced 默认的 `Wrapping::Word`：
    /// `Word` 只在**词边界**折行，而一个 100 位的整数中间没有任何词边界
    ///（也没有空格）⇒ 它被当成一个不可折的"词"，照样横向溢出、被容器的
    /// 右边界裁掉。用户报的「当行长过长时会被挡住，右边看不到后半段」
    /// 正是这么来的 —— 换成 `Glyph` 才能在任意字符间断行。
    pub wrap: bool,
    /// **纯字母的记号是否用斜体**（函数名一律斜体）。
    ///
    /// 默认 `false`：历史区/结果区遵守数学排版惯例 —— 函数名（`sin`、`log`）
    /// 直立、只有单字母变量斜体。输入栏与函数面板**开 true**：
    /// 输入栏把函数名当一个个独立记号渲染，用户明确要求"函数都用斜体，
    /// 与输入栏字体相同"（2026-10-24），所以这两处统一开。
    pub italic_names: bool,
}

impl Default for Style2 {
    fn default() -> Self {
        Self {
            size: 26.0,
            color: None,
            // 默认折行：能一眼看全是第一位的，横向滚动那档留给有偏好的用户
            wrap: true,
            italic_names: false,
        }
    }
}

/// 渲染入口：把一段结果文本变成可显示的 `Element`。
///
/// - `math` 为真（MathIO）→ 先解析成语法树再排版；
/// - 为假（LineIO，纯小数）→ 直接当文本，避免把 `1.41421356…` 误判成结构。
/// - `on_placeholder`：空参数框被点了发什么消息（`None` = 不可点，历史区用这个）。
pub fn render<'a, M: Clone + 'a>(
    src: &str,
    math: bool,
    style: Style2,
    on_placeholder: Option<&'a dyn Fn(usize, bool) -> M>,
) -> Element<'a, M> {
    if !math {
        return plain(src, style);
    }
    // 需要点击（输入区）才包位置层；只读的历史区走纯解析（树更简单、更快）
    let node = if on_placeholder.is_some() {
        parse_impl(src, None, true)
    } else {
        parse(src)
    };
    build(&node, style, on_placeholder)
}

/// 纯文本渲染
fn plain<'a, M: 'a>(src: &str, style: Style2) -> Element<'a, M> {
    // ⚠️ **必须显式给字体**（用户反馈："虚拟键盘和计算栏的字母和符号并没有使用
    // 数学字体，而是等线"）。
    //
    // 以前这里只在 `monospace` 时才设字体，其余情况**不设** ⇒ 落到 iced 的
    // 默认字体（系统无衬线）上，于是同一屏里键面是等线、历史区又不一样。
    // 现在统一走 [`crate::fonts::font_for`]：含中文用等线，其余一律数学字体。
    plain_font(src, style, crate::fonts::font_for(src))
}

/// 纯文本渲染，**字体由调用方给**（变量斜体走这里）。
fn plain_font<'a, M: 'a>(src: &str, style: Style2, font: iced::Font) -> Element<'a, M> {
    let mut t = text(src.to_string())
        .size(Pixels(style.size))
        .font(font)
        // 见 `Style2::wrap` 的说明：折行要按**字符**断，不能按词断。
        .wrapping(if style.wrap {
            iced::widget::text::Wrapping::Glyph
        } else {
            iced::widget::text::Wrapping::None
        });
    if let Some(c) = style.color {
        t = t.color(c);
    }
    t.into()
}

/// 这个记号是不是**变量**（数学排版里变量用斜体、数字和函数名直立）
///
/// 判据：**单个 ASCII 字母**。`sin`（三个字母）是函数名，直立；
/// `π` / `θ` / `∞` 是符号或常数，也直立。
/// 公开版本：**虚拟键盘**也要用同一判据（用户要求"键面与输入栏字体同步"）
pub fn looks_like_variable(s: &str) -> bool {
    is_variable(s)
}

fn is_variable(s: &str) -> bool {
    let mut it = s.chars();
    matches!((it.next(), it.next()), (Some(c), None) if c.is_ascii_alphabetic())
}

/// 递归把节点变成 widget。
///
/// 公开是为了让调用方**先 parse 再 build**，从而摆脱 `&str` 的生命周期约束：
/// [`render`] 的返回值会借用入参（它内部要 `src.to_string()`），
/// 入参若是局部变量就没法把结果返回出去。分成两步后，
/// [`Node`] 拥有自己的字符串，借它渲染出来的 widget 生命周期就自由了。
///
/// `on_placeholder`：**空参数框被点击**时要发什么消息（参数是那个框的源文本偏移）。
/// 传 `None` 就是纯视觉（历史区里的旧结果用 `None` —— 那里不该能点）。
/// 之所以要传闭包而不是直接给 `Message`：本模块对消息类型是**泛型**的
///（`M` 由调用方决定），它不认识 `app::Message`。
pub fn build<'a, M: Clone + 'a>(
    node: &Node,
    style: Style2,
    on_placeholder: Option<&'a dyn Fn(usize, bool) -> M>,
) -> Element<'a, M> {
    match node {
        // 变量（单个字母）用斜体 —— 数学排版规范。
        // `italic_names` 开启时（输入栏/函数面板），**纯字母的函数名也斜体**
        //（用户 2026-10-24："函数都用黑色斜体字，与输入栏字体相同"）。
        Node::Text(s)
            if is_variable(s)
                || (style.italic_names
                    && !s.is_empty()
                    && s.chars().all(|c| c.is_ascii_alphabetic())) =>
        {
            plain_font(s, style, crate::fonts::font_math_italic())
        }
        Node::Text(s) => plain(s, style),
        // 带源位置的包装 ⇒ 一个**可点区域**：点它就把光标送到 `at`。
        //
        // 这是"点哪改哪"能精确的根本 —— 位置来自**解析时的源偏移**。
        // 背后的透明 editor 也会处理点击，但它按纯文本字形算位置，
        // 与我们画出来的排版层对不上（用户："完全无法选中修改"）。
        Node::At { at, inner } => {
            let el = build(inner, style, on_placeholder);
            match on_placeholder {
                Some(f) => {
                    // 纵向补足到接近整行高：算式的字形本身很矮（比如一个 `+`），
                    // 只按字形命中的话点起来要非常准 —— 补一层 padding 让整块都可点，
                    // 是编辑器里常见的做法（点运算符旁边也算点它）。
                    iced::widget::mouse_area(
                        container(el).padding([2, 1]).align_y(Alignment::Center),
                    )
                    .on_press(f(*at, false))
                    .into()
                }
                None => el,
            }
        }
        // 分隔符（`sum` 的参数逗号）只在切分阶段有用，排版时不显示
        Node::Comma { .. } => Space::new().width(Pixels(0.0)).height(Pixels(0.0)).into(),
        // 字符级点击目标：**只包一层热区，不加内边距**（见 `Node::Hit` 的说明）
        Node::Hit { at, inner } => {
            let el = build(inner, style, on_placeholder);
            match on_placeholder {
                Some(f) => {
                    // **左右半边交给不同语义**（用户 2026-10-31 报的 bug：
                    // "点字符右半段无法把光标移到该字符后面"）。
                    //
                    // 做法：整块热区按"落在该字符之前"处理；再用一层**透明覆盖**
                    // 盖住右半边，点它就"落在该字符之后"。用 `stack!` 叠上去，
                    // 不改动字形本身的布局（覆盖层是不占位、无内容的 Space）。
                    let left = iced::widget::mouse_area(el).on_press(f(*at, false));
                    let right_half = Row::new()
                        .height(Length::Fill)
                        .push(Space::new().width(Length::FillPortion(1)))
                        .push(
                            iced::widget::mouse_area(
                                Space::new()
                                    .width(Length::FillPortion(1))
                                    .height(Length::Fill),
                            )
                            .on_press(f(*at, true)),
                        );
                    iced::widget::stack![left, right_half].into()
                }
                None => el,
            }
        }
        // 聚焦中的空框：框作底、光标叠在框内（贴左、垂直居中）
        Node::EmptyBare { at } => {
            // 3px 宽、一整行高：视觉上几乎不可见，但够点中
            let el: Element<'a, M> = container(Space::new().width(Length::Fixed(3.0)))
                .width(Length::Fixed(3.0))
                .height(Length::Fixed(style.size * 1.35))
                .into();
            match on_placeholder {
                Some(f) => iced::widget::mouse_area(el).on_press(f(*at, false)).into(),
                None => el,
            }
        }
        Node::EmptyFocused { at } => {
            let (w, h) = empty_box_size(style.size);
            let c = style.color.unwrap_or_else(|| theme::colors().text);
            let box_el: Element<'a, M> = Canvas::new(EmptyBox {
                w,
                h,
                fill: c.scale_alpha(0.10),
                stroke: c.scale_alpha(0.55),
            })
            .width(Length::Fixed(w))
            .height(Length::Fixed(h))
            .into();
            // 光标层：同尺寸，内容左对齐 + 垂直居中，左侧留 2px 呼吸
            let caret_el: Element<'a, M> = container(caret(style.size))
                .width(Length::Fixed(w))
                .height(Length::Fixed(h))
                .align_x(Alignment::Start)
                .align_y(Alignment::Center)
                .padding([0, 2])
                .into();
            let stacked: Element<'a, M> = iced::widget::stack![box_el, caret_el].into();
            match on_placeholder {
                Some(f) => iced::widget::mouse_area(stacked)
                    .on_press(f(*at, false))
                    .into(),
                None => stacked,
            }
        }
        // 括号：左右两条**可缩放**弧线，高度跟着内容走
        Node::Paren { inner } => {
            let h = node_height(inner, style.size).max(style.size * 1.35);
            let w = (h * 0.26).clamp(3.0, 15.0);
            let c = style.color.unwrap_or_else(|| theme::colors().text);
            let thick = (style.size * 0.055).clamp(1.0, 1.8);
            let make = |open: bool| -> Element<'a, M> {
                Canvas::new(Delim {
                    h,
                    open,
                    color: c,
                    thick,
                })
                .width(Length::Fixed(w))
                .height(Length::Fixed(h))
                .into()
            };
            Row::new()
                .align_y(Alignment::Center)
                .spacing(style.size * 0.06)
                .push(make(true))
                .push(build(inner, style, on_placeholder))
                .push(make(false))
                .into()
        }
        // 光标：一根竖线。它的位置由布局引擎算 —— 见 `Node::Caret`
        Node::Caret => caret(style.size),
        // 空参数：**手绘的圆角空框**（用户要求："框不要用字符表示，请你手动渲染，
        // 并且框至少要能囊括光标的高度"）。
        //
        // ## 为什么不再用 `□` 字符
        //
        // 1. **高度不受控**：`□` 的高度由字体度量决定，而光标高度是
        //    `size * 1.25` —— 上下限那种小字号下框明显比光标矮，光标会"戳到框外面"
        //    （用户实测）。手绘之后宽高都按公式算，**框高恒等于光标高**，不可能露出去。
        // 2. **宽度对不齐**：字形宽度只能靠 `CHAR_WIDTH_RATIO` 估，而画出来的是精确值。
        // 3. 字体缺 `□` 时会退化成豆腐块；手绘没有这个问题。
        //
        // 尺寸由 [`empty_box_size`] 统一给（`node_width` 也用同一份，否则
        // 分数横线/根号顶线会按错的宽度画）。
        Node::Empty { at } => {
            let (w, h) = empty_box_size(style.size);
            let c = style.color.unwrap_or_else(|| theme::colors().text);
            let box_el: Element<'a, M> = Canvas::new(EmptyBox {
                w,
                h,
                fill: c.scale_alpha(0.10),
                stroke: c.scale_alpha(0.55),
            })
            .width(Length::Fixed(w))
            .height(Length::Fixed(h))
            .into();
            match on_placeholder {
                Some(f) => iced::widget::mouse_area(box_el)
                    .on_press(f(*at, false))
                    .into(),
                None => box_el,
            }
        }

        Node::BigOp {
            kind,
            body,
            var,
            from,
            to,
        } => {
            // 这一家族都是"一个大符号 + 上下限 + 主体"的排版：上下限用小一号字
            // （标准数学排版惯例），主体保持正文大小。
            let limit_style = Style2 {
                size: bigop_limit_size(style.size),
                ..style
            };
            // 大符号自己一号（见 `bigop_glyph_size`）
            let glyph_style = Style2 {
                size: bigop_glyph_size(style.size),
                ..style
            };
            match kind {
                BigKind::Sum | BigKind::Prod => {
                    // 上限在上、`变量 = 下限` 在下，主体在右侧（Desmos 的样子）
                    let glyph = if matches!(kind, BigKind::Sum) {
                        "∑"
                    } else {
                        "∏"
                    };
                    // ⚠️ 下限那一行**显式给高度**：`Column` 自动取高度时它会塌掉 ——
                    // 证据是它**仍占宽度**（父容器被撑宽了、Σ 的居中位置也跟着偏），
                    // 却画不出任何东西。用户看到的正是"Σ 下面那行 `▯ = ▯` 整个没了"。
                    // 给一个硬高度最省事，也最不容易再出意外。
                    let row_h = (limit_style.size * 1.9).max(18.0);
                    let lower = container(
                        Row::new()
                            .align_y(Alignment::Center)
                            .spacing(2.0)
                            .push(build(var, limit_style, on_placeholder))
                            .push(plain("=", limit_style))
                            .push(build(from, limit_style, on_placeholder)),
                    )
                    .height(Length::Fixed(row_h))
                    .align_y(Alignment::Center);
                    let stack = Column::new()
                        .width(Length::Shrink)
                        .align_x(Alignment::Center)
                        .push(build(to, limit_style, on_placeholder))
                        .push(plain(glyph, glyph_style))
                        .push(lower);
                    Row::new()
                        .align_y(Alignment::Center)
                        .spacing(style.size * 0.16)
                        .push(stack)
                        .push(build(body, style, on_placeholder))
                        .into()
                }
                BigKind::Integral => {
                    // ∫：上限在上、下限在下，右侧写成 `f dx`
                    //（`dx` 是积分写法的一部分，少了它读起来就不像积分）
                    let stack = Column::new()
                        .width(Length::Shrink)
                        .align_x(Alignment::Center)
                        .push(build(to, limit_style, on_placeholder))
                        .push(plain("∫", glyph_style))
                        .push(build(from, limit_style, on_placeholder));
                    let dx = Row::new()
                        .align_y(Alignment::Center)
                        .spacing(style.size * 0.04)
                        .push(plain("d", style))
                        .push(build(var, style, on_placeholder));
                    Row::new()
                        .align_y(Alignment::Center)
                        .spacing(style.size * 0.09)
                        .push(stack)
                        .push(build(body, style, on_placeholder))
                        .push(dx)
                        .into()
                }
                BigKind::Limit => {
                    // `lim` 在上、`x→a` 在下，主体在右侧
                    let arrow = Row::new()
                        .align_y(Alignment::Center)
                        .spacing(1.5)
                        .push(build(var, limit_style, on_placeholder))
                        .push(plain("→", limit_style))
                        .push(build(from, limit_style, on_placeholder));
                    let stack = Column::new()
                        .width(Length::Shrink)
                        .align_x(Alignment::Center)
                        .push(plain("lim", style))
                        .push(arrow);
                    Row::new()
                        .align_y(Alignment::Center)
                        .spacing(style.size * 0.18)
                        .push(stack)
                        .push(build(body, style, on_placeholder))
                        .into()
                }
                BigKind::Diff => {
                    // `d/dx`：竖排分数（d 在上、`dx` 在下），右侧接主体。
                    //
                    // 复用 `Frac` 的渲染而不是另写一遍横线逻辑 —— 那块的宽度估算
                    //（`frac_bar_width`）已经有测试守着。
                    let frac = Node::Frac(
                        Box::new(Node::Text("d".into())),
                        Box::new(Node::Seq(vec![Node::Text("d".into()), (**var).clone()])),
                    );
                    Row::new()
                        .align_y(Alignment::Center)
                        .spacing(style.size * 0.10)
                        .push(build(&frac, style, on_placeholder))
                        .push(build(body, style, on_placeholder))
                        .into()
                }
            }
        }
        Node::Seq(items) => {
            let mut r = Row::new().align_y(Alignment::Center).spacing(0.0);
            for (i, it) in items.iter().enumerate() {
                // 运算符两侧留白（规则见 `op_pads`：行首的 `+ - ±` 才算一元）
                match op_of(it).and_then(|t| op_pads(i, t)) {
                    Some((l, rr)) => {
                        r = r.push(
                            container(build(it, style, on_placeholder)).padding(Padding {
                                top: 0.0,
                                right: style.size * rr,
                                bottom: 0.0,
                                left: style.size * l,
                            }),
                        );
                    }
                    None => r = r.push(build(it, style, on_placeholder)),
                }
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
            let num_el = build(num, inner_style, on_placeholder);
            let den_el = build(den, inner_style, on_placeholder);
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
            // 根号 = **矢量路径** 画的根号（√ 折线 + 顶线，连成一条折线）+ 被开方式。
            //
            // ## 为什么不再用 `√` 字符 + 一条 `Rule`
            //
            // 第一版是"`√` 字符 + 上面一条横线"拼出来的。快照里能看出**上下分离**：
            // 横线悬在被开方式上方，而 `√` 字符的顶钩够不到横线（用户反馈：
            // "仔细看渲染的根号发现上下有点分离"）。
            //
            // 根因是字体度量：`√` 字形的墨迹顶只到 x-height / cap-height 附近，
            // 而横线贴在行盒**顶部**，两者之间必然空一截。而且这个空隙**随字体变化**
            //（Cambria / Georgia / Times 的 `√` 各不相同），改不好、也测不准。
            //
            // 所以改用 `canvas` 把根号画成**一条连续折线**（印刷排版里根号本就是
            // 装饰性符号，不是靠字体的 `√` 字形）。这样才能保证：
            //   * 顶钩与顶线**严丝合缝**（同一路径，不可能分离）；
            //   * 顶线长度**精确等于**被开方式宽度（由布局给的尺寸算）；
            //   * 换任何数学字体，根号形状都不变。
            //
            // ## 布局
            //
            // 根号是"底图"，被开方式是"前景"，两者**叠起来**（`stack!`）：
            //   * 底图：`Canvas`，宽度 = 根号钩宽 + 被开方式宽，画 √ 折线 + 顶线；
            //   * 前景：被开方式，**左移进钩子右侧**（左 padding = 钩宽），
            //     并**下移**到顶线以下（上 padding = 顶线位置）。
            // 用叠加而不是 `Row`，是因为被开方式必须落在顶线**正下方**、
            // 而 `Row` 的横向排布做不到"压到前一个元素下面"。
            let radicand = build(inner, style, on_placeholder);
            let radicand_w = node_width(inner, style.size);
            let color = style.color.unwrap_or_else(|| theme::colors().text);
            // 整个根号的高度：比被开方式略高一点，顶线压在上沿、钩子下探一点。
            let height = style.size * 1.32;
            // 钩宽（√ 折线占的横向宽度）—— 随字号缩放，保证不同字号下比例一致。
            // 与 `node_width` 里的 `root_hook_width` 是同一个来源，别各写各的。
            let hook_w = root_hook_width(style.size);

            let sign = Canvas::new(Radical {
                hook_w,
                color,
                thickness: (style.size * 0.065).clamp(1.0, 2.0),
            })
            .width(Length::Fixed(hook_w + radicand_w))
            .height(Length::Fixed(height));

            // 前景：被开方式，右移到钩子右侧、下移到顶线以下。
            let laid_out = container(radicand)
                .width(Length::Fixed(hook_w + radicand_w))
                .height(Length::Fixed(height))
                .padding(Padding {
                    // 上边距 = 顶线到被开方式上沿的间隙（让字不贴着线）
                    top: height - style.size * 1.0,
                    // 左边距 = 钩宽：把被开方式推到钩子右边
                    left: hook_w,
                    right: 0.0,
                    bottom: 0.0,
                })
                .align_x(Alignment::Start)
                .align_y(Alignment::End);

            // ⚠️ **顶对齐**：次数要骑在根号钩子上方（`∛` 的 3 在左上），
            // 早先这里是 `End`，次数被压到了根号**下面**（用户实测截图）。
            let mut r = Row::new().align_y(Alignment::Start).spacing(0.0);
            if let Some(d) = degree {
                // n 次根：左上角小字（如 `∛`）
                r = r.push(build(
                    d,
                    Style2 {
                        size: style.size * 0.55,
                        ..style
                    },
                    on_placeholder,
                ));
            }
            r = r.push(iced::widget::stack![sign, laid_out]);
            r.into()
        }
        Node::Pow(base, exp) => {
            // 右上标：基线对齐到顶端，字号小一档
            let mut r = Row::new().align_y(Alignment::Start).spacing(0.0);
            r = r.push(build(base, style, on_placeholder));
            r = r.push(build(
                exp,
                Style2 {
                    size: style.size * 0.62,
                    ..style
                },
                on_placeholder,
            ));
            r.into()
        }
        Node::Sub(base, sub) => {
            // 右下标：与 Pow（右上标）成对 —— 对齐方向相反（底对齐），
            // 字号同样小一档。目前唯一生产者是 `log` 的底数框。
            let mut r = Row::new().align_y(Alignment::End).spacing(0.0);
            r = r.push(build(base, style, on_placeholder));
            r = r.push(build(
                sub,
                Style2 {
                    size: style.size * 0.62,
                    ..style
                },
                on_placeholder,
            ));
            r.into()
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 根号（矢量绘制）
// ─────────────────────────────────────────────────────────────────────────────

/// 根号符号的 `canvas` 程序：把 `√` 折线与顶线画成**一条连续折线**。
///
/// 这样顶钩与顶线是同一条路径的两段，**结构上**不可能分离 —— 这正是"用 `√` 字符
/// 拼横线"做不到的（用户反馈的"上下有点分离"就是那个拼法的固有缺陷）。
///
/// 形状（坐标系：左上角为原点，y 向下）：
///
/// ```text
///  (0, mid)            (hook_w, near_top)                     (w, near_top)
///      ●───────────────────●──────────────────────────────────────●   ← 顶线
///       \                 /
///        \               /
///         ●─────────────●
///     (hook_w*0.28, bottom)  ...  (下探的钩子尖)
/// ```
///
/// 实际上是标准书写式根号：左侧一个"对勾"（下探 → 上行 → 转平 → 顶线右行）。
struct Radical {
    /// 钩子（左侧 `√` 折线）占的横向宽度
    hook_w: f32,
    /// 线条颜色
    color: iced::Color,
    /// 线宽
    thickness: f32,
}

impl<Message> iced::widget::canvas::Program<Message> for Radical {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());

        let w = bounds.width.max(1.0);
        let h = bounds.height.max(1.0);
        // 顶线离顶端的间隙：让线身完全落在画布内（半线宽 + 1px 余量）
        let line_y = self.thickness * 0.5 + 0.5;
        // 钩子尖（最低点）抬高一个半线宽，避免超出画布下沿被裁
        let bottom_y = h - self.thickness * 0.5;

        // 钩子的三个关键点：
        //   A：左端起点（中偏下）
        //   B：下探的最低点（钩子尖）
        //   C：抬起头、准备接顶线的位置
        let a = Point::new(self.thickness * 0.5, line_y + h * 0.42);
        let b = Point::new(self.hook_w * 0.30, bottom_y);
        let c = Point::new(self.hook_w - self.thickness * 0.5, line_y);
        let d = Point::new(w - self.thickness * 0.5, line_y);

        let path = Path::new(|p| {
            p.move_to(a);
            p.line_to(b);
            p.line_to(c);
            p.line_to(d);
        });

        frame.stroke(
            &path,
            Stroke::default()
                .with_color(self.color)
                .with_width(self.thickness)
                // 尖角处用圆角接头，观感更接近印刷根号
                .with_line_join(LineJoin::Round)
                .with_line_cap(LineCap::Round),
        );

        vec![frame.into_geometry()]
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

/// 根号钩子（左侧 `√` 折线）占的横向宽度 ÷ 字号。
///
/// 渲染（`build` 里的 `Radical`）与宽度估算（`node_width`）**必须**用同一个值，
/// 否则根号在序列里占的宽度与实际画出来的对不上。抽成函数就是为了防止两处写岔。
fn root_hook_width(size: f32) -> f32 {
    size * 0.62
}

/// **可缩放的圆括号**（左/右各一条弧线，高度随内容变）。
///
/// 用 canvas 手绘而不是 `(` 字符：字符高度是固定的，内容一变高括号就"没包住"。
/// 弧线用二次贝塞尔 —— 控制点落在同侧边中点，出来的就是标准括号外形。
struct Delim {
    h: f32,
    /// 左括号 `(` / 右括号 `)`
    open: bool,
    color: iced::Color,
    thick: f32,
}

impl<Message> iced::widget::canvas::Program<Message> for Delim {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let w = bounds.width.max(1.0);
        let h = self.h.min(bounds.height.max(1.0));
        // 弧线两端各留半个线宽，免得被画布边缘裁掉
        let pad = self.thick * 0.5;
        let (x0, x1) = if self.open {
            (w - pad, pad)
        } else {
            (pad, w - pad)
        };
        let path = Path::new(|b| {
            b.move_to(Point::new(x0, pad));
            // 控制点在**另一侧**的垂直中点：形成中段最细、两端回收的括号外形
            b.quadratic_curve_to(Point::new(x1, h * 0.5), Point::new(x0, h - pad));
        });
        frame.stroke(
            &path,
            Stroke::default()
                .with_color(self.color)
                .with_width(self.thick),
        );
        vec![frame.into_geometry()]
    }
}

/// 估算一个节点的**高度**（像素）—— 定界符与括号内边距要用它。
///
/// 只是估算：它只需要"够高、别太夸张"，精确排版交给布局引擎。
fn node_height(node: &Node, size: f32) -> f32 {
    match node {
        Node::Text(_) | Node::Caret => size * 1.25,
        Node::Empty { .. } | Node::EmptyFocused { .. } | Node::EmptyBare { .. } => {
            empty_box_size(size).1
        }
        Node::Comma { .. } => 0.0,
        Node::At { inner, .. } | Node::Hit { inner, .. } => node_height(inner, size),
        Node::Paren { inner } => node_height(inner, size).max(size * 1.35),
        Node::Seq(v) => v.iter().map(|n| node_height(n, size)).fold(0.0, f32::max),
        // 分子 + 分母 + 横线与上下间隙（与 `build` 里的间距同源）
        Node::Frac(a, b) => {
            let inner = size * 0.72;
            node_height(a, inner) + node_height(b, inner) + size * 0.16 + 1.6
        }
        // `build` 里根号高度写死 `size * 1.32`
        Node::Root { .. } => size * 1.32,
        Node::Pow(a, b) => node_height(a, size).max(node_height(b, size * 0.62) + size * 0.3),
        // 下标与上标对称：小字号 + 向下的伸出量
        Node::Sub(a, b) => node_height(a, size).max(node_height(b, size * 0.62) + size * 0.3),
        Node::BigOp { .. } => {
            bigop_glyph_size(size) * 1.2 + 2.0 * (bigop_limit_size(size) * 1.9).max(18.0)
        }
    }
}

/// 画一根**输入光标**（竖线）。
///
/// 高度 = `size * 1.25`，与空参数框的高度**同一个公式** —— 框和光标一样高，
/// 光标才不会戳出框外。
///
/// 单独提出来是因为**输入框为空时也要能看见光标**（用户反馈："没有输入时点击
/// 输入栏不会出现光标"）：那时画的是提示语、没有 `Node` 树可依赖。
pub fn caret<'a, M: 'a>(size: f32) -> Element<'a, M> {
    container(Space::new().width(Pixels(0.0)).height(Pixels(size)))
        .width(Length::Fixed((size * 0.09).max(1.5)))
        .height(Length::Fixed(size * 1.25))
        .style(theme::caret)
        .into()
}

/// **大运算符符号**（Σ / Π / ∫）的字号。
///
/// ⚠️ 用户反馈："sigma 的大小感觉非常小"。以前符号和正文**同字号**，
/// 而 Σ 的字形本身就矮（墨迹只到 cap-height 附近）⇒ 看起来比右边的 `k²`
/// 还小，完全不像"一个把整个式子括起来的大符号"。
///
/// 正规数学排版里，显示式的大运算符高度约是正文的 **1.6–2 倍**
/// （LaTeX `\displaystyle` 的 Σ 就是 1.75em 上下）。这里取 1.75。
fn bigop_glyph_size(size: f32) -> f32 {
    size * 1.75
}

/// 大运算符的**上下限**字号（标准排版里比正文小一号）
fn bigop_limit_size(size: f32) -> f32 {
    size * 0.55
}

/// 大运算符符号占的横向宽度（与 [`bigop_glyph_size`] 同源，否则主体会贴太近/太远）
fn bigop_glyph_width(size: f32) -> f32 {
    bigop_glyph_size(size) * 0.58
}

/// **二元运算符 / 关系符两侧的留白**（单位：em，乘字号）。
///
/// 正规数学排版里，运算符不是"贴着数字"的：
/// `1+1` 要排成 `1 + 1`，而 `=` 两侧还要更宽一点（LaTeX 的 `\thinmuskip` 4mu、
/// `\thickmuskip` 5mu，这里按 0.14em / 0.20em 折算）。
///
/// ⚠️ **只给"出现在两个操作数之间"的运算符加**（见 [`op_pad`] 的调用处）：
/// 行首的 `-`（负号，如 `-1/4`）不该加，否则会变成 `- 1`。
fn op_pad(s: &str) -> Option<f32> {
    match s {
        // 二元运算符（窄一点）
        "+" | "-" | "−" | "·" | "×" | "÷" | "±" | "%" => Some(0.14),
        // 关系符（宽一点）
        "=" | "<" | ">" | "≤" | "≥" | "≠" | "≈" | "→" => Some(0.20),
        _ => None,
    }
}

/// 某个位置上的运算符**左右**各留多少白（em）。
///
/// `index` 是它在所在行里的下标。只有行首的 `+` / `-` / `±` 才可能是**一元**符号
/// （`-1/4` 的负号），那时左侧不留白；其余情况（含行首的 `=`，关系符不可能是一元）
/// 两侧都留。
fn op_pads(index: usize, s: &str) -> Option<(f32, f32)> {
    let k = op_pad(s)?;
    let unary_at_head = index == 0 && matches!(s, "+" | "-" | "−" | "±");
    Some((if unary_at_head { 0.0 } else { k }, k))
}

/// 从节点里取出"它就是一个运算符"时的那个运算符文本
/// （`At` / `Hit` 是位置包装，语义上还是里面那个记号）
fn op_of(node: &Node) -> Option<&str> {
    match node {
        Node::Text(s) => Some(s),
        Node::At { inner, .. } | Node::Hit { inner, .. } => op_of(inner),
        _ => None,
    }
}

/// 空参数框的尺寸 `(宽, 高)`。
///
/// ⚠️ **高 = 光标高**（`size * 1.25`，见 `build` 里 `Node::Caret` 那一段）：
/// 框比光标矮的话，光标会从框里戳出去 —— 用户实测报的就是这个。
/// 宽按字号给一个比例，并留一个下限保证点得中。
fn empty_box_size(size: f32) -> (f32, f32) {
    ((size * 0.62).max(10.0), (size * 1.25).max(14.0))
}

/// 空参数占位框 —— **手绘的圆角矩形**（见 `build` 里 `Node::Empty` 的说明）。
///
/// 为什么用 `canvas` 而不是 `□` 字符：字符的高度由字体度量决定，与光标高度
/// （`size * 1.25`）对不上，光标会戳到框外面；宽度也只能靠比例估算。
/// 手绘之后宽高都是公式算出来的精确值。
struct EmptyBox {
    w: f32,
    h: f32,
    fill: iced::Color,
    stroke: iced::Color,
}

impl<Message> iced::widget::canvas::Program<Message> for EmptyBox {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        // 别超出画布（`Canvas` 的 bounds 就是我们在 `build` 里给的 Fixed 宽高）
        let w = self.w.min(bounds.width).max(1.0);
        let h = self.h.min(bounds.height).max(1.0);
        // 圆角跟着高度走，小字号下不会变成"胶囊"
        let radius = iced::border::Radius::from((h * 0.18).min(3.5));
        // ⚠️ iced 0.14 只有 `Path::rounded_rectangle(top_left, size, radius)`
        // 与 `Frame::fill(path, Fill)`（没有 `fill_rounded_rectangle`）。
        let path = Path::rounded_rectangle(Point::ORIGIN, iced::Size::new(w, h), radius);
        frame.fill(&path, self.fill);
        frame.stroke(
            &path,
            Stroke::default().with_color(self.stroke).with_width(1.0),
        );
        vec![frame.into_geometry()]
    }
}

/// 估算一个节点占的宽度（像素）
fn node_width(node: &Node, size: f32) -> f32 {
    match node {
        Node::Text(s) => s.chars().count() as f32 * size * CHAR_WIDTH_RATIO,
        // 位置包装：宽度就是里面那棵子树
        Node::At { inner, .. } | Node::Hit { inner, .. } => node_width(inner, size),
        // 空占位框 / 光标：固定的小宽度
        // ⚠️ 必须与 `empty_box_size` 同源，否则分数横线/根号顶线按错的宽度画
        Node::Empty { .. } | Node::EmptyFocused { .. } => empty_box_size(size).0,
        // 括号里的空参数：只有 3px（见 `EmptyBare`）
        Node::EmptyBare { .. } => 3.0,
        Node::Caret => size * 0.12,
        Node::Comma { .. } => 0.0,
        // Σ/Π：宽度取"大符号"与"上下限里较宽的那组"之较大者
        Node::BigOp { var, from, to, .. } => {
            // ⚠️ 必须与 `build` 里的 `bigop_limit_size` / `bigop_glyph_width` 同源，
            // 否则大符号的居中位置会偏（上下限比符号宽时尤其明显）。
            let l = bigop_limit_size(size);
            let up = node_width(to, l);
            let low = node_width(var, l) + node_width(from, l);
            up.max(low) + bigop_glyph_width(size)
        }
        // ⚠️ 必须与 `build` 的留白规则同源（见 `op_pad` 的调用处）
        Node::Seq(items) => items
            .iter()
            .enumerate()
            .map(|(i, it)| {
                let extra = op_of(it)
                    .and_then(|t| op_pads(i, t))
                    .map_or(0.0, |(l, r)| size * (l + r));
                node_width(it, size) + extra
            })
            .sum(),
        // 分数：取分子/分母较宽者（横线宽度），外加一点内边距
        Node::Frac(num, den) => {
            let inner = size * 0.72;
            let w = node_width(num, inner).max(node_width(den, inner));
            (w + size * 0.30).max(size * 0.85)
        }
        // 根号：钩宽（`√` 折线占的横向空间）+ 被开方式宽 + 顶线右侧出头一点
        Node::Root { inner, degree } => {
            let d = degree
                .as_ref()
                .map(|d| node_width(d, size * 0.55))
                .unwrap_or(0.0);
            // ⚠️ 必须与 `build` 里 `Radical` 的布局一致：钩宽 = `size * 0.62`。
            // 两处不一致的话，根号在 `Seq` 里占的宽度会与实际画出来的对不上，
            // 后面的元素会盖上来或留出过大空隙。
            d + root_hook_width(size) + node_width(inner, size) + 3.0
        }
        // 括号：两条弧线的宽度 + 内容（宽度由 `node_height` 推出来）
        Node::Paren { inner } => {
            let h = node_height(inner, size).max(size * 1.35);
            let w = (h * 0.26).clamp(3.0, 15.0);
            2.0 * w + node_width(inner, size) + size * 0.12
        }
        // 幂：底数 + 上标（字号更小）
        Node::Pow(base, exp) => node_width(base, size) + node_width(exp, size * 0.62),
        // 下标与上标同宽规则
        Node::Sub(base, sub) => node_width(base, size) + node_width(sub, size * 0.62),
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
                    wrap: true,
                    ..Style2::default()
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
            // 下标拍平成 `log_b` 的读法
            Node::Sub(a, b) => format!("{}_{}", flatten(a), flatten(b)),
            // 位置包装：拍平后就是里面那棵子树（位置不属于"文本"）
            Node::At { inner, .. } | Node::Hit { inner, .. } => flatten(inner),
            // 不携带源文本的三种（占位框 / 光标 / 参数逗号）
            Node::Empty { .. }
            | Node::EmptyFocused { .. }
            | Node::EmptyBare { .. }
            | Node::Caret
            | Node::Comma { .. } => String::new(),
            // 括号分组：拍平成 `(内容)`（与历史区的显示一致）
            Node::Paren { inner } => format!("({})", flatten(inner)),
            Node::BigOp {
                body,
                var,
                from,
                to,
                ..
            } => format!(
                "sum({},{},{},{})",
                flatten(body),
                flatten(var),
                flatten(from),
                flatten(to)
            ),
        }
    }

    /// 节点树里有没有光标（测试辅助）
    fn contains_caret(n: &Node) -> bool {
        match n {
            // 聚焦中的空框里画了光标
            Node::Caret | Node::EmptyFocused { .. } => true,
            Node::Paren { inner } => contains_caret(inner),
            Node::Seq(v) => v.iter().any(contains_caret),
            Node::Frac(a, b) => contains_caret(a) || contains_caret(b),
            Node::Root { degree, inner } => {
                degree.as_deref().is_some_and(contains_caret) || contains_caret(inner)
            }
            Node::Pow(a, b) => contains_caret(a) || contains_caret(b),
            Node::BigOp {
                body,
                var,
                from,
                to,
                ..
            } => {
                contains_caret(body)
                    || contains_caret(var)
                    || contains_caret(from)
                    || contains_caret(to)
            }
            Node::At { inner, .. } | Node::Hit { inner, .. } => contains_caret(inner),
            _ => false,
        }
    }

    /// `sum(...)` / `prod(...)` 要排成 Σ / Π，而不是当成普通函数名。
    ///
    /// 参数顺序是 core 的约定：`sum(被运算式, 变量, 下限, 上限)`
    ///（见 `keypad::FN_GROUPS` 里的 `sum(f,k,a,b)`）。
    #[test]
    fn parses_big_operators() {
        let n = parse("sum(k^2,k,1,10)");
        let Node::BigOp { kind, to, from, .. } = &n else {
            panic!("应解析成大运算符：{n:?}");
        };
        assert_eq!(*kind, BigKind::Sum);
        assert_eq!(flatten(to), "10", "上限应是第 4 个参数（末位）");
        assert_eq!(flatten(from), "1", "下限应是第 3 个参数");

        let n = parse("prod(i,i,1,5)");
        assert!(
            matches!(
                n,
                Node::BigOp {
                    kind: BigKind::Prod,
                    ..
                }
            ),
            "prod 应排成 Π：{n:?}"
        );
    }

    /// 空的参数位置要变成**占位框**，而且带正确的源文本偏移
    ///（点了框才知道该把光标送到哪 —— 这是用户明确要求的交互）。
    #[test]
    fn empty_arguments_become_placeholders() {
        // 空根号：`sqrt(` 是 5 个字符，所以括号内是偏移 5
        let n = parse("sqrt()");
        let Node::Root { inner, .. } = &n else {
            panic!("{n:?}")
        };
        assert_eq!(**inner, Node::Empty { at: 5 }, "sqrt() 的占位框应在括号内");

        // Σ 的第 4 个参数空着
        let n = parse("sum(k,k,1,)");
        let Node::BigOp { to, .. } = &n else {
            panic!("{n:?}")
        };
        assert!(
            matches!(**to, Node::Empty { .. }),
            "空参数应是占位框：{to:?}"
        );

        // 分数少一边：`1/` 的分母也是占位框
        let n = parse("1/");
        let Node::Frac(_, den) = &n else {
            panic!("{n:?}")
        };
        assert!(matches!(**den, Node::Empty { .. }), "1/ 的分母应是占位框");
    }

    /// 光标：`parse_with_caret` 在对应偏移处插一根竖线，且**不破坏结构**。
    ///
    /// `1|/2` 这种"光标正好停在 `/` 前面"是最刁的位置：分数分支会
    /// `out.pop()` 取分子，一不小心就把光标 pop 走了（第一版就是这样，
    /// 分子直接消失）。这条守着它。
    #[test]
    fn caret_is_inserted_without_breaking_structure() {
        // 末尾
        assert!(
            contains_caret(&parse_with_caret("ab", Some(2))),
            "末尾应插光标"
        );
        // 开头
        assert!(
            contains_caret(&parse_with_caret("ab", Some(0))),
            "开头应插光标"
        );
        // 数字内部
        assert!(
            contains_caret(&parse_with_caret("12", Some(1))),
            "数字中间应插光标"
        );

        // 最刁的一个：光标停在 `/` 前 —— 结构必须还是一个完整分数
        let n = parse_with_caret("1/2", Some(1));
        assert!(
            matches!(n, Node::Seq(_) | Node::Frac(..)),
            "结构不该被光标打散：{n:?}"
        );
        assert!(contains_caret(&n), "光标不能丢：{n:?}");
        assert!(
            flatten(&n).contains("1/2") || flatten(&n).contains("1"),
            "分子不能被光标吃掉：{:?}",
            flatten(&n)
        );

        // 不给光标时不该出现 Caret
        assert!(!contains_caret(&parse("ab")));
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

    /// **变量判据**：单个字母才斜体；数字、函数名、符号都直立。
    ///
    /// 数学排版规范：变量斜体（`x`）、数字直立（`2`）、函数名直立（`sin`）、
    /// 常数与符号直立（`π`）。判错了就会出现"2 也斜着"或"x 不斜"。
    #[test]
    fn only_single_letters_are_variables() {
        for v in ["x", "k", "a", "A", "t"] {
            assert!(is_variable(v), "{v} 应是变量");
        }
        for not in ["sin", "pi", "7", "π", "θ", "x1", "", " ", "+"] {
            assert!(!is_variable(not), "{not:?} 不该被当成变量");
        }
    }

    /// 根号的**几何不变量**：钩宽是渲染与测宽的共同来源，且总宽必须容得下
    /// 「钩 + 被开方式」。
    ///
    /// 背景：根号曾经是「`√` 字符 + 一条 `Rule`」拼的，字符字形顶钩够不到横线 ⇒
    /// 视觉上"上下分离"（用户反馈）。现在改成 `canvas` 画的**单条折线**
    ///（`√` 折线 + 顶线连成一条路径），路径内部不可能分离 —— 但由此引入一个新约束：
    /// **渲染用的钩宽**与**测宽用的钩宽**必须是同一个数，否则根号在序列里
    /// 占的宽度与实际画出来的对不上（后面的元素会盖上来）。
    /// 所以这里钉死 `root_hook_width` 与渲染代码共用同一函数。
    ///
    /// 分离本身无法在纯函数里断言（要靠肉眼 / 快照），这里守的是**宽度一致性**。
    #[test]
    fn root_geometry_is_consistent() {
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
            let size = 24.0;
            let inner_w = node_width(&inner, size);
            let hook = root_hook_width(size);
            let total = node_width(&parse(s), size);
            // 总宽 = 钩宽 + 被开方式宽 + 右侧出头（与 `node_width` 的公式一致）
            let expected = hook + inner_w + 3.0;
            assert!(
                (total - expected).abs() < 0.01,
                "{s}: 总宽 {total} 应等于 钩{hook} + 内容{inner_w} + 3.0"
            );
            // 钩宽必须为正、且随字号线性缩放
            assert!(hook > 0.0, "{s}: 钩宽必须为正");
            assert!(
                (root_hook_width(size * 2.0) - hook * 2.0).abs() < 0.01,
                "钩宽应随字号线性缩放"
            );
            // 总宽必须**严格大于**被开方式宽 —— 否则被开方式会顶到根号外面
            assert!(
                total > inner_w,
                "{s}: 根号总宽 {total} 必须大于被开方式宽 {inner_w}"
            );
        }
    }
}
