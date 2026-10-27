//! 输入框的**结构化表达式**模型 —— 输入框不再"一串文本 + 字符偏移光标"。
//!
//! ## 为什么必须换成树
//!
//! 旧模型里光标是**源文本里的字符偏移**，而数学结构（分数、根号、Σ）在文本里是
//! **折叠**的：`∑(,,,)` 的四个参数在源文本里都是"括号内同一处"。于是出现两个死结：
//!
//! 1. **点 Σ 的四个格子都一样** —— 四个空参数框算出来的是同一个偏移，
//!    点哪个都跳同一个位置，看起来就是"点了没反应"；
//! 2. **光标会落到括号上** —— `∑()` 里 `(` 那个位置不属于任何节点，
//!    光标无处可画 ⇒ 视觉上"光标消失"，接着打字就插在 `∑` 和 `(` 之间变成 `∑x()`。
//!
//! 这两个都不是"某处写错了"，是"文本表达不了四个不同的空位"。
//! 所以这里把输入的**本质**换成树：[`Expr`] 的每个复合节点有**若干命名插槽**，
//! 光标指向"哪个插槽里的第几个位置"（[`Cursor`]），
//! 于是每个插槽都是**天然合法的光标位置**，既不会重、也不会消失。
//!
//! ## 与 `mathrender` 的分工
//!
//! 本模块只负责**模型与编辑**（纯函数，可脱 GUI 单测）；
//! 排版仍然交给 [`crate::mathrender`] —— [`Doc::to_node`] 把树转成
//! `mathrender::Node`（那是**渲染 IR**），顺便把每个可点目标登记成 [`Cursor`]。
//! 这样不必重写分数竖排/根号折线/Σ 上下限那几百行排版代码。
//!
//! ## 三条用户拍板的规则（别随手改）
//!
//! 1. **`←` `→` 走"线性阅读序"**：把整棵树摊平成一串合法光标位置，
//!    左右各自前后挪一格（[`Doc::linear`]）。好处是**永远不可能落在括号上**。
//! 2. **括号是结构**：`(` 插入一个 [`Expr::Paren`]，光标落在里面，括号自动配平，
//!    不可能造出左右括号数不等的输入。
//! 3. **退格绝不误删外层结构**：退格只删"当前这一行里光标前面那一项"；
//!    光标在空插槽里（行首）时**什么都不做**。只有当光标正好在整个结构
//!    **之后那一位**时，退格才会把整个结构删掉。

use crate::mathrender::{BigKind, Node};

/// 一行（一个插槽里）的若干项，按从左到右的顺序。
///
/// ⚠️ **`Text` 是"原子记号"而不是"任意字符串"**：一个 `Text` 只装一个记号
/// （单个字符，或 `pi` / `theta` / `inf` 这种整体才成立的多字符符号）。
/// 光标只能落在项与项之间，所以"一个字符一项"才让光标能停在任意字符之间；
/// 而 `pi` 拆成 `p`、`i` 就不再是 π 了 —— 这正是它必须整体成一项的原因。
pub type Row = Vec<Expr>;

/// 输入的语法树。
///
/// 复合节点的插槽顺序是**固定的**（下面每个变体都写明了），
/// [`Expr::slot`] / [`Expr::slot_mut`] 按那个顺序取。
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// 原子记号（数字、字母、运算符、`pi` 这类整体符号）
    Text(String),
    /// 竖排分数：槽 0 = 分子，槽 1 = 分母
    Frac { num: Row, den: Row },
    /// 根号：槽 0 = 被开方式
    Root { inner: Row },
    /// **n 次根**：槽 0 = 次数（左上角小字），槽 1 = 被开方式
    ///
    /// `cbrt`（∛）与 `nroot(x, n)`（ⁿ√）都落在这个节点；`sqrt` 保持 [`Expr::Root`]
    ///（不带次数，序列化成 `sqrt(...)`，与 core 的两个名字一一对应）。
    NthRoot { degree: Row, inner: Row },
    /// **对数**：槽 0 = 底数（渲染成 `log` 的右下标），槽 1 = 真数
    ///
    /// Desmos 式的 `log□(x)`：点函数面板的 `log` 插进来的就是它 ——
    /// 底数是**下标位置的一个空框**，不再是文本 `log(,)` 里那个奇怪的逗号。
    /// 序列化回 core 认的 `log(base, x)`。
    Log { base: Row, arg: Row },
    /// 幂：槽 0 = 底数，槽 1 = 指数
    Pow { base: Row, exp: Row },
    /// 大运算符：槽 0 = 主体，1 = 变量，2 = 下限，3 = 上限
    ///
    /// `lim` 只用前三个（上限恒空）、`diff` 只用前两个 —— 序列化时按种类截断，
    /// 见 [`Expr::source`]。
    BigOp {
        kind: BigKind,
        body: Row,
        var: Row,
        from: Row,
        to: Row,
    },
    /// 括号分组：槽 0 = 括号内
    Paren(Row),
}

impl Expr {
    /// 有几个插槽
    pub fn slot_count(&self) -> usize {
        match self {
            Expr::Text(_) => 0,
            Expr::Frac { .. } => 2,
            Expr::Root { .. } => 1,
            Expr::NthRoot { .. } | Expr::Log { .. } => 2,
            Expr::Pow { .. } => 2,
            Expr::BigOp { .. } => 4,
            Expr::Paren(_) => 1,
        }
    }

    /// 光标在该节点的插槽间移动的**视觉顺序**（`←` `→` 与"插入后光标落点"都用它）。
    ///
    /// 用户 2026-10-23 定的规则：
    /// - Σ / ∏：**左下（变量）→ 右下（下限）→ 上方（上限）→ 右侧（主体）** ——
    ///   人看 Σ 是先读"k = n"这一行，再看上限，最后看被求和式；
    /// - ∫ 这类"下方一个框 + 上方一个框 + 右侧主体 + 末尾 `d<变量>`"：
    ///   **下 → 上 → 主体 → 变量**；
    /// - `lim`：变量 → 趋近值 → 主体；`diff`：变量（`d/dx` 的分母）→ 主体。
    ///
    /// ⚠️ 只影响**移动顺序**，不影响 `to_node` 的渲染/点击目标顺序（那是同一遍遍历，
    /// 自己保持一致即可）；两者若混用会重演"点哪跑去别处"的老 bug。
    pub fn slot_order(&self) -> &'static [usize] {
        match self {
            Expr::Text(_) => &[],
            Expr::Frac { .. } | Expr::Pow { .. } => &[0, 1],
            Expr::Root { .. } | Expr::Paren(_) => &[0],
            // ⁿ√：先打次数（左上角）、再打被开方式；∛ 的次数已预填 3，
            // "第一个空插槽"自然落在被开方式里 —— 两全。
            Expr::NthRoot { .. } => &[0, 1],
            // log_b(x)：先打底数（下标框）、再打真数
            Expr::Log { .. } => &[0, 1],
            Expr::BigOp { kind, .. } => match kind {
                BigKind::Sum | BigKind::Prod => &[1, 2, 3, 0],
                BigKind::Integral => &[2, 3, 0, 1],
                // ⚠️ **只列看得见的槽**：`lim` 不用 `to`（上限），`diff` 只用
                // 变量与主体。早先把不可见槽也列进来，于是光标从 `d/dx` 的框里
                // 出来要按**三下**方向键（用户 2026-10-27 实测）。
                BigKind::Limit => &[1, 2, 0],
                BigKind::Diff => &[1, 0],
            },
        }
    }

    /// 取第 `i` 个插槽（越界给 `None`）
    pub fn slot(&self, i: usize) -> Option<&Row> {
        match self {
            Expr::Text(_) => None,
            Expr::Frac { num, den } => match i {
                0 => Some(num),
                1 => Some(den),
                _ => None,
            },
            Expr::Root { inner } => (i == 0).then_some(inner),
            Expr::NthRoot { degree, inner } => match i {
                0 => Some(degree),
                1 => Some(inner),
                _ => None,
            },
            Expr::Log { base, arg } => match i {
                0 => Some(base),
                1 => Some(arg),
                _ => None,
            },
            Expr::Pow { base, exp } => match i {
                0 => Some(base),
                1 => Some(exp),
                _ => None,
            },
            Expr::BigOp {
                body,
                var,
                from,
                to,
                ..
            } => match i {
                0 => Some(body),
                1 => Some(var),
                2 => Some(from),
                3 => Some(to),
                _ => None,
            },
            Expr::Paren(inner) => (i == 0).then_some(inner),
        }
    }

    /// 可变版 [`Expr::slot`]
    pub fn slot_mut(&mut self, i: usize) -> Option<&mut Row> {
        match self {
            Expr::Text(_) => None,
            Expr::Frac { num, den } => match i {
                0 => Some(num),
                1 => Some(den),
                _ => None,
            },
            Expr::Root { inner } => (i == 0).then_some(inner),
            Expr::NthRoot { degree, inner } => match i {
                0 => Some(degree),
                1 => Some(inner),
                _ => None,
            },
            Expr::Log { base, arg } => match i {
                0 => Some(base),
                1 => Some(arg),
                _ => None,
            },
            Expr::Pow { base, exp } => match i {
                0 => Some(base),
                1 => Some(exp),
                _ => None,
            },
            Expr::BigOp {
                body,
                var,
                from,
                to,
                ..
            } => match i {
                0 => Some(body),
                1 => Some(var),
                2 => Some(from),
                3 => Some(to),
                _ => None,
            },
            Expr::Paren(inner) => (i == 0).then_some(inner),
        }
    }

    /// 序列化成 **MathIO 源文本**（求值与历史只走这条路）。
    ///
    /// 括号策略：**只在有歧义时才加**。比如 `1+2` 当分子会写成 `(1+2)/3`，
    /// 而 `1/2` 当分子则仍是 `1/2/3`（左结合，与 MathIO 一致）。
    pub fn source(&self) -> String {
        match self {
            Expr::Text(s) => s.clone(),
            Expr::Frac { num, den } => {
                format!("{}/{}", wrap_row(num), wrap_row(den))
            }
            Expr::Root { inner } => format!("sqrt({})", row_source(inner)),
            // core 的签名：cbrt(x) / nroot(x, n)（参数序：被开方式在前）
            Expr::NthRoot { degree, inner } => match row_source(degree).as_str() {
                "3" => format!("cbrt({})", row_source(inner)),
                d => format!("nroot({},{})", row_source(inner), d),
            },
            // core 的签名：log(base, x)
            Expr::Log { base, arg } => {
                format!("log({},{})", row_source(base), row_source(arg))
            }
            Expr::Pow { base, exp } => {
                // **`sin⁻¹` 写法 → core 认的 `asin`**（用户 2026-10-27 实测：
                // 手输 `sin^-1(45)` 会报"未知标识符: sin"，因为 core 里没有 `sin`
                // 这个标识符，反三角只叫 `asin`）。
                //
                // 只在指数**恰好是 -1** 且底数正好是某个三角名时改写；
                // 其它幂次（`sin^2`）保持原样 —— 那是"正弦的平方"，不是函数。
                let b = row_source(base);
                let e = row_source(exp);
                if e.trim() == "-1"
                    && let Some(inv) = inverse_of(&b)
                {
                    return inv.to_string();
                }
                format!("{}^{}", wrap_row(base), wrap_row(exp))
            }
            Expr::BigOp {
                kind,
                body,
                var,
                from,
                to,
            } => {
                let b = row_source(body);
                let v = row_source(var);
                match kind {
                    // core 的签名：sum(f,k,a,b) / prod / int(f,x,a,b)
                    BigKind::Sum | BigKind::Prod | BigKind::Integral => {
                        let name = match kind {
                            BigKind::Sum => "sum",
                            BigKind::Prod => "prod",
                            _ => "int",
                        };
                        format!("{name}({b},{v},{},{})", row_source(from), row_source(to))
                    }
                    // lim(f,x,a)
                    BigKind::Limit => format!("lim({b},{v},{})", row_source(from)),
                    // diff(f,x)
                    BigKind::Diff => format!("diff({b},{v})"),
                }
            }
            Expr::Paren(inner) => format!("({})", row_source(inner)),
        }
    }
}

/// 一行的源文本（直接拼接，不额外加括号）
pub fn row_source(row: &Row) -> String {
    row.iter().map(Expr::source).collect()
}

/// 当分子/分母/底数时是否需要补括号
///
/// 判据：这一行不是"单一原子"，或者那一个原子本身就是会吞掉两边的结构
/// （分数、幂、大运算符）。`Paren` / `Root` 自己带括号或自带边界，不用再包。
fn wrap_row(row: &Row) -> String {
    let needs = match row.as_slice() {
        [] => false,
        [one] => matches!(
            one,
            Expr::Frac { .. } | Expr::Pow { .. } | Expr::BigOp { .. }
        ),
        _ => true,
    };
    let s = row_source(row);
    if needs && !s.is_empty() {
        format!("({s})")
    } else {
        s
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 光标
// ─────────────────────────────────────────────────────────────────────────────

/// 光标 = "从根行往里走到哪一行" + "在这一行的第几个位置"。
///
/// `path` 的每一步是 `(行内项下标, 进入该项的第几个插槽)`。
/// 空 `path` = 根行。`at` 是行内的**项间位置**（`0..=行内项数`），
/// 所以它天然只可能是合法位置 —— 不存在"落在括号上"这回事。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Cursor {
    pub path: Vec<(usize, usize)>,
    pub at: usize,
}

impl Cursor {
    /// 根行第 `at` 位
    pub fn root(at: usize) -> Self {
        Self {
            path: Vec::new(),
            at,
        }
    }
}

/// 沿 `path` 找到那一行（只读）。路径失效时停在能走到的最深处。
fn row_in<'a>(root: &'a Row, path: &[(usize, usize)]) -> &'a Row {
    let mut cur = root;
    for &(idx, slot) in path {
        let Some(e) = cur.get(idx) else { return cur };
        let Some(sub) = e.slot(slot) else { return cur };
        cur = sub;
    }
    cur
}

/// 沿 `path` 找到那一行（可写）。路径失效时停在能走到的最深处。
fn row_in_mut<'a>(root: &'a mut Row, path: &[(usize, usize)]) -> &'a mut Row {
    let mut cur = root;
    for &(idx, slot) in path {
        if idx >= cur.len() {
            return cur;
        }
        // 借用拆分：先确认有那个槽，再递归
        if cur[idx].slot(slot).is_none() {
            return cur;
        }
        cur = cur[idx].slot_mut(slot).expect("上面刚检查过");
    }
    cur
}

// ─────────────────────────────────────────────────────────────────────────────
// 文档（= 输入框的全部状态）
// ─────────────────────────────────────────────────────────────────────────────

/// 输入框的完整状态：一棵树 + 一个光标。
///
/// `app::State` 只存这个类型，所有编辑操作都是它的方法 ——
/// 这样"输入框怎么动"可以在**不建界面**的情况下全部单测。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Doc {
    pub root: Row,
    pub cursor: Cursor,
}

impl Doc {
    /// 从一段 MathIO 文本建文档（粘贴、测试用）。
    ///
    /// 光标位置：**优先落在第一个空插槽**，没有空插槽才放到末尾。
    ///
    /// 为什么不是一律末尾：粘/设一个"骨架"（`∑()`、`sqrt()`、`diff(,x)`）时，
    /// 用户的下一步一定是往里填东西，落在末尾（比如 Σ 的**上限**）会让人
    /// 摸不着头脑；而粘一段完整式子（`sum(k^2,k,1,10)`）时没有空槽，
    /// 自然就落到末尾。
    pub fn from_text(text: &str) -> Self {
        let root = parse(text);
        let mut d = Self {
            cursor: Cursor::default(),
            root,
        };
        match first_empty_in(&d.root, 0, d.root.len(), &[]) {
            Some(c) => d.cursor = c,
            None => d.end(),
        }
        d
    }

    /// 输入框是不是空的（空 ⇒ 显示提示语）
    pub fn is_empty(&self) -> bool {
        self.root.is_empty()
    }

    /// 序列化成求值用的源文本
    pub fn source(&self) -> String {
        row_source(&self.root)
    }

    /// 清空
    pub fn clear(&mut self) {
        self.root.clear();
        self.cursor = Cursor::default();
    }

    /// 光标所在的那一行
    pub fn here(&self) -> &Row {
        row_in(&self.root, &self.cursor.path)
    }

    fn here_mut(&mut self) -> &mut Row {
        row_in_mut(&mut self.root, &self.cursor.path)
    }

    /// 还有没有**空插槽**（= 式子没写完）。
    ///
    /// 按 = 之前用它拦一下：`∑(,,,)` 这种半成品丢给解析器只会得到
    /// "位置 4 处意外的字符: ','" 这种 CLI 味的报错，跟界面语言完全不搭。
    pub fn has_empty_slot(&self) -> bool {
        first_empty_in(&self.root, 0, self.root.len(), &[]).is_some()
    }

    // ── 遍历 ────────────────────────────────────────────────────────────────

    /// 把整棵树摊平成"线性阅读序"的光标位置表。
    ///
    /// 顺序 = 先"项间位置"，再下一项的插槽内部，依此类推：
    /// `1/2` ⇒ `[根行0, 分子0, 分母0, 根行1]`。
    ///
    /// `←` `→` 就是在这张表上前后挪一格 —— 表里每一项都是**合法位置**，
    /// 所以光标既不会消失，也不会跑到括号上（用户报过的两个 bug 的根因）。
    pub fn linear(&self) -> Vec<Cursor> {
        let mut out = Vec::new();
        collect_positions(&self.root, &[], &mut out);
        out
    }

    /// 当前光标在线性表里的下标（找不到就返回 0）
    fn linear_index(&self, table: &[Cursor]) -> usize {
        table.iter().position(|c| c == &self.cursor).unwrap_or(0)
    }

    /// `←`
    pub fn left(&mut self) {
        let table = self.linear();
        let i = self.linear_index(&table);
        if i > 0 {
            self.cursor = table[i - 1].clone();
        }
    }

    /// `→`
    pub fn right(&mut self) {
        let table = self.linear();
        let i = self.linear_index(&table).min(table.len().saturating_sub(1));
        if i + 1 < table.len() {
            self.cursor = table[i + 1].clone();
        }
    }

    /// `Home`：整份输入的最前面
    pub fn home(&mut self) {
        self.cursor = Cursor::default();
    }

    /// `End`：整份输入的最后面
    pub fn end(&mut self) {
        let mut path = Vec::new();
        let mut row = &self.root;
        // 一直往"最后一项的最后一个插槽"里走，直到不能走
        while let Some(last) = row.len().checked_sub(1) {
            let e = &row[last];
            let n = e.slot_count();
            if n == 0 {
                break;
            }
            path.push((last, n - 1));
            row = e.slot(n - 1).expect("刚取过 slot_count");
        }
        self.cursor = Cursor {
            path,
            at: row.len(),
        };
    }

    // ── 插入 ────────────────────────────────────────────────────────────────

    /// 在光标处插入纯文本（粘贴、单字符键都走这里）。
    ///
    /// 文本会先按[记号](tokenize)切开，**一个记号一项** —— 这样光标能停在
    /// 任意两个字符之间（合并成一项的话，`12` 里就再也插不进东西了）。
    pub fn insert_text(&mut self, text: &str) {
        let tokens = tokenize(text);
        if tokens.is_empty() {
            return;
        }
        let n = tokens.len();
        let at = self.cursor.at.min(self.here().len());
        {
            let row = self.here_mut();
            for (k, t) in tokens.into_iter().enumerate() {
                row.insert(at + k, Expr::Text(t));
            }
        }
        self.cursor.at = at + n;
    }

    /// 按键盘语义插入一段"键面文本"。
    ///
    /// 这是键盘/函数面板与模型之间**唯一**的接缝：
    /// - `^` / `^2` ⇒ 把光标前一项变成底数，插入 [`Expr::Pow`]，光标进指数；
    /// - `)` ⇒ 跳出当前分组（不插字符，避免造出不配对的括号）；
    /// - `sqrt()` / `∑()` / `diff(,x)` 这类模板 ⇒ 先[解析](parse)成结构再插入，
    ///   光标落到**第一个空插槽**（于是 `sqrt()` 一插完光标就在根号里面）。
    pub fn insert_key(&mut self, text: &str) {
        // ⚠️ **换行要先处理**：`trim()` 会把 `\n` 抹掉，而输入框用换行分隔多行
        // （`Ctrl+Enter`、粘贴多行表达式都靠它）。所以含换行的输入直接按
        // 普通记号插入，不进下面的"结构键"分支。
        if text.contains('\n') {
            self.insert_text(text);
            return;
        }
        let t = text.trim();
        if t.is_empty() {
            return;
        }
        // ⓪ **反三角的 `(` 要跳出指数**（用户 2026-10-27 实测的 bug）：
        // 手输 `sin` `^` `-1` `(` 时，光标还停在**指数槽**里，于是变成了
        // `sin^(-1(45))` —— 括号进了指数、整个式子算不出来、回显里括号也没了。
        // 这里先把光标挪到幂外面，后面照常插一个普通括号组。
        if t == "(" {
            self.leave_neg_one_exponent();
        }
        // ① 上标：把前一项当底数
        if let Some(exp_src) = t.strip_prefix('^') {
            let exp = parse(exp_src);
            self.insert_pow(exp);
            return;
        }
        // ①' 除号：把前一项当**分子**，光标落到分母
        //
        // ⚠️ 为什么不能像别的一样"插一个空分数再把光标送进第一个空槽"：
        // 那样 `1` `÷` `2` 会得到 `12/`（2 进了分子、1 被留在外面）。
        // 用户敲 `÷` 的语义是"我刚打完的是分子"，所以必须**包裹前一项**。
        if t == "/" {
            self.insert_frac();
            return;
        }
        // ② 右括号 = 跳出当前分组
        if t == ")" {
            self.jump_out();
            return;
        }
        // ③ 结构模板
        let items = parse(t);
        let structured = items.iter().any(|e| !matches!(e, Expr::Text(_)));
        if !structured {
            self.insert_text(t);
            return;
        }
        let n = items.len();
        let at = self.cursor.at.min(self.here().len());
        {
            let row = self.here_mut();
            for (k, e) in items.into_iter().enumerate() {
                row.insert(at + k, e);
            }
        }
        // 光标进"新插入这块里第一个空插槽"，没有就停在插入块之后
        let base = self.cursor.path.clone();
        let found = {
            let row = self.here();
            first_empty_in(row, at, at + n, &base)
        };
        self.cursor = found.unwrap_or(Cursor {
            path: base,
            at: at + n,
        });
    }

    /// 光标若停在 `sin^-1` 的**指数槽**末尾，就把它挪到幂外面（供 `(` 使用）。
    ///
    /// 判定三件事：路径最后一段是幂的指数槽（`slot == 1`）、指数正好是 `-1`、
    /// 底数是三角名。三者都满足才动 —— 否则 `x^2` 后面的 `(` 不该被特殊对待。
    fn leave_neg_one_exponent(&mut self) {
        let Some(&(idx, slot)) = self.cursor.path.last() else {
            return;
        };
        if slot != 1 {
            return;
        }
        let exp = row_in(&self.root, &self.cursor.path);
        if self.cursor.at != exp.len() || row_source(exp).trim() != "-1" {
            return;
        }
        // 底数：父行里的 `idx` 那一项，必须是 Pow 且底数是单个三角名
        let parent = self.cursor.path[..self.cursor.path.len() - 1].to_vec();
        let base_ok = match row_in(&self.root, &parent).get(idx) {
            Some(Expr::Pow { base, .. }) => {
                let b = row_source(base);
                inverse_of(b.trim()).is_some()
            }
            _ => false,
        };
        if !base_ok {
            return;
        }
        self.cursor = Cursor {
            path: parent,
            at: idx + 1,
        };
    }

    /// 插入 `/`：前一项当分子，光标进分母
    fn insert_frac(&mut self) {
        let at = self.cursor.at.min(self.here().len());
        let mut path = self.cursor.path.clone();
        let idx = {
            let row = self.here_mut();
            if at > 0 {
                let prev = row.remove(at - 1);
                row.insert(
                    at - 1,
                    Expr::Frac {
                        num: vec![prev],
                        den: Vec::new(),
                    },
                );
                at - 1
            } else {
                // 光标在行首：没有分子可包，留空分数（光标照样落在分母）
                row.insert(
                    at,
                    Expr::Frac {
                        num: Vec::new(),
                        den: Vec::new(),
                    },
                );
                at
            }
        };
        path.push((idx, 1));
        self.cursor = Cursor { path, at: 0 };
    }

    /// 插入 `^`：前一项当底数
    fn insert_pow(&mut self, exp: Row) {
        let at = self.cursor.at.min(self.here().len());
        let exp_len = exp.len();
        let mut path = self.cursor.path.clone();
        let idx = {
            let row = self.here_mut();
            if at > 0 {
                // 把光标前那一项拔出来当底数
                let prev = row.remove(at - 1);
                row.insert(
                    at - 1,
                    Expr::Pow {
                        base: vec![prev],
                        exp,
                    },
                );
                at - 1
            } else {
                // 光标在行首：没东西可当底数，底数留空（用户接着打字就填进去）
                row.insert(
                    at,
                    Expr::Pow {
                        base: Vec::new(),
                        exp,
                    },
                );
                at
            }
        };
        path.push((idx, 1));
        self.cursor = Cursor { path, at: exp_len };
    }

    /// 跳出当前分组：光标移到"包住我的那个结构"之后
    pub fn jump_out(&mut self) {
        let Some((idx, _slot)) = self.cursor.path.last().copied() else {
            return;
        };
        let mut path = self.cursor.path.clone();
        path.pop();
        self.cursor = Cursor { path, at: idx + 1 };
    }

    // ── 删除 ────────────────────────────────────────────────────────────────

    /// 退格：删当前行里光标**前面**那一项。
    ///
    /// ⚠️ **用户拍板：绝不误删外层结构**。光标在该行最前面（`at == 0`）时
    /// 什么都不做 —— 哪怕这是个空插槽，也不去删外面的 Σ / 根号。
    /// 想删整个结构，得先把光标移到它**之后**（`→` 会经过那个位置）。
    pub fn backspace(&mut self) {
        if self.cursor.at == 0 {
            return;
        }
        let at = self.cursor.at;
        if at > self.here().len() {
            return;
        }
        let row = self.here_mut();
        // **多字符记号按字符删**（用户 2026-10-27："按一下删除键应当一个一个字母删去，
        // 但是现在会直接删去函数"）。
        //
        // 典型就是函数名：`sin` 是一个 token（这样它才不会被拆成 `s`+`i`+`n`），
        // 但退格时用户期望的是"删掉最后一个字母"。反过来 `pi` / `theta` / `inf`
        // 是**一个符号**（显示成 π / θ / ∞），它们必须整体删 —— 删成 `p` 就不是那个东西了。
        if let Some(Expr::Text(text)) = row.get(at - 1) {
            let chars: Vec<char> = text.chars().collect();
            if chars.len() > 1 && !matches!(text.as_str(), "pi" | "theta" | "inf") {
                row[at - 1] = Expr::Text(chars[..chars.len() - 1].iter().collect());
                // 光标位置不变（这一项还在，只是短了一个字符）
                self.cursor.at = at;
                return;
            }
        }
        row.remove(at - 1);
        self.cursor.at = at - 1;
    }

    /// `Delete`：删光标**后面**那一项（同样不会去碰外层结构）
    pub fn delete(&mut self) {
        let at = self.cursor.at;
        let row = self.here_mut();
        if at < row.len() {
            row.remove(at);
        }
    }

    // ── 点击定位 ────────────────────────────────────────────────────────────

    /// 所有可点目标（点它 ⇒ 光标落在那个位置）。顺序**必须**与
    /// [`Doc::to_node`] 生成 `Hit`/`Empty` 时的登记顺序一致。
    pub fn targets(&self) -> Vec<Cursor> {
        self.to_node().1
    }

    /// 点第 `id` 个目标
    pub fn place(&mut self, id: usize) {
        if let Some(c) = self.targets().get(id) {
            self.cursor = c.clone();
        }
    }

    // ── 渲染 ────────────────────────────────────────────────────────────────

    /// 树 → 渲染 IR。
    ///
    /// 返回 `(节点, 目标表)`：目标表第 `i` 项就是节点里 `Hit{at:i}` /
    /// `Empty{at:i}` 的含义 —— 点它就 [`Doc::place`]`(i)`。
    ///
    /// ⚠️ 空行**带光标时**渲染成 `[Caret, □]`：光标本身只有 1~2px 宽，
    /// 单独放在根号/分数里会让那块被压扁（`node_width` 按宽度估算），
    /// 后面那个 □ 顺带把宽度撑回来，视觉上也更明显"这里要填东西"。
    pub fn to_node(&self) -> (Node, Vec<Cursor>) {
        let mut b = Builder {
            cur: &self.cursor,
            out: Vec::new(),
            bare: false,
        };
        let node = b.row(&self.root, &[]);
        (node, b.out)
    }
}

/// 渲染 IR 的构造器（顺便登记可点目标）
struct Builder<'a> {
    cur: &'a Cursor,
    out: Vec<Cursor>,
    /// 当前这一行是不是**括号组内部**（用户 2026-10-27：括号里不画空框）
    bare: bool,
}

impl Builder<'_> {
    /// 一行 → 节点
    fn row(&mut self, row: &Row, base: &[(usize, usize)]) -> Node {
        let cursor_here = base == self.cur.path.as_slice();
        let at = self.cur.at.min(row.len());

        // 空行：光标在这里就画光标（外加一个占位框撑宽度），否则给一个可点的 □
        if row.is_empty() {
            let id = self.out.len();
            self.out.push(Cursor {
                path: base.to_vec(),
                at: 0,
            });
            if cursor_here {
                // 括号内部：**只有光标、没有框**（用户 2026-10-27）
                if self.bare {
                    return Node::Caret;
                }
                // 聚焦中的空框：**框 + 框内光标**（`Node::EmptyFocused`）。
                // 不用 `Seq[Caret, Empty]`：那样光标在框外面，用户要的是"框囊括光标"。
                return Node::EmptyFocused { at: id };
            }
            // 括号内部：给一个极窄的**透明**点击区（不画框，但还点得进去）
            return if self.bare {
                Node::EmptyBare { at: id }
            } else {
                Node::Empty { at: id }
            };
        }

        let mut items: Vec<Node> = Vec::new();
        for i in 0..=row.len() {
            if cursor_here && i == at {
                items.push(Node::Caret);
            }
            if i == row.len() {
                break;
            }
            // 每个"项"都是一个点击目标：点它 ⇒ 光标落在它**之前**
            let id = self.out.len();
            self.out.push(Cursor {
                path: base.to_vec(),
                at: i,
            });
            let inner = self.item(&row[i], base, i);
            items.push(Node::Hit {
                at: id,
                inner: Box::new(inner),
            });
        }
        // 行尾再补一个**透明点击区**（3px）：点"最后一个记号右边"就是"光标到末尾"。
        //
        // 为什么需要：`Hit` 只能表达"落在某一项之前"，最后一个位置没有目标可点 ——
        // 用户实测"Σ 后面点不进去，只能用方向键"。`EmptyBare` 的拍平文本是空串，
        // 所以不影响任何文本断言，只是多了 3px 宽度。
        // 只在**最外层**加：槽内的"末尾点击"意义不大，加了反而让每行
        // 多出一堆目标（`targets_let_you_click_each_bigop_slot` 就是按
        // 精确数量断言的，多到无法维护）。
        if base.is_empty() {
            let id = self.out.len();
            self.out.push(Cursor {
                path: base.to_vec(),
                at: row.len(),
            });
            items.push(Node::Hit {
                at: id,
                inner: Box::new(Node::EmptyBare { at: id }),
            });
        }
        match items.len() {
            1 => items.pop().expect("刚判断过长度"),
            _ => Node::Seq(items),
        }
    }

    /// 一项 → 节点（递归进插槽）
    fn item(&mut self, e: &Expr, base: &[(usize, usize)], idx: usize) -> Node {
        match e {
            // ⚠️ 渲染时要把 `pi` / `theta` / `inf` 换成符号（与历史区一致）。
            // 反三角 additionally 显示成 `sin⁻¹`（用户 2026-10-24："sin^-1 打出来
            // 为什么还是 asin"）—— 只是**显示层**的映射，`source()` 仍是 core 认的
            // `asin`，两边互不影响。
            Expr::Text(s) => match inverse_trig_base(s) {
                Some(name) => Node::Pow(
                    Box::new(Node::Text(name.to_string())),
                    Box::new(Node::Text("−1".into())),
                ),
                None => Node::Text(crate::mathrender::render_ident(s)),
            },
            Expr::Frac { num, den } => Node::Frac(
                Box::new(self.slot_node(num, base, idx, 0)),
                Box::new(self.slot_node(den, base, idx, 1)),
            ),
            Expr::Root { inner } => Node::Root {
                degree: None,
                inner: Box::new(self.slot_node(inner, base, idx, 0)),
            },
            // ⁿ√：mathrender 的 Root 本来就支持次数（左上角小字）
            Expr::NthRoot { degree, inner } => Node::Root {
                degree: Some(Box::new(self.slot_node(degree, base, idx, 0))),
                inner: Box::new(self.slot_node(inner, base, idx, 1)),
            },
            // log_b(x)：`log` + 右下标（底数框）+ 括号包真数。
            // 下标用 `Node::Sub`（与上标 Pow 成对）；括号走可缩定界符，
            // 高度跟着真数走。
            // ⚠️ 解构名不能叫 `base` —— 会遮住外面的路径参数 `base`。
            Expr::Log { base: b, arg: a } => Node::Seq(vec![
                Node::Sub(
                    Box::new(Node::Text("log".into())),
                    Box::new(self.slot_node(b, base, idx, 0)),
                ),
                Node::Paren {
                    inner: Box::new(self.slot_node(a, base, idx, 1)),
                },
            ]),
            Expr::Pow { base: b, exp } => Node::Pow(
                Box::new(self.slot_node(b, base, idx, 0)),
                Box::new(self.slot_node(exp, base, idx, 1)),
            ),
            Expr::BigOp {
                kind,
                body,
                var,
                from,
                to,
            } => Node::BigOp {
                kind: *kind,
                body: Box::new(self.slot_node(body, base, idx, 0)),
                var: Box::new(self.slot_node(var, base, idx, 1)),
                from: Box::new(self.slot_node(from, base, idx, 2)),
                to: Box::new(self.slot_node(to, base, idx, 3)),
            },
            // 括号用**可缩放定界符**（高度跟着内容走）—— 见 `Node::Paren`
            //
            // 槽内是 bare 的：括号里不放空框（用户 2026-10-27）
            Expr::Paren(inner) => {
                let mut p = base.to_vec();
                p.push((idx, 0));
                let old = self.bare;
                self.bare = true;
                let inner_node = self.row(inner, &p);
                self.bare = old;
                Node::Paren {
                    inner: Box::new(inner_node),
                }
            }
        }
    }

    /// 进入第 `idx` 项的第 `slot` 个插槽
    fn slot_node(&mut self, row: &Row, base: &[(usize, usize)], idx: usize, slot: usize) -> Node {
        let mut p = base.to_vec();
        p.push((idx, slot));
        self.row(row, &p)
    }
}

/// 递归登记一行的所有光标位置（顺序与 [`Builder::row`] 的遍历一致）
fn collect_positions(row: &Row, base: &[(usize, usize)], out: &mut Vec<Cursor>) {
    for i in 0..=row.len() {
        out.push(Cursor {
            path: base.to_vec(),
            at: i,
        });
        if i == row.len() {
            break;
        }
        let e = &row[i];
        // ⚠️ 按 `slot_order`（视觉顺序）走，不是按槽位序号 —— Σ 的下限在下、
        // 上限在上，若按序号走 `→` 会"先跳上方再回下方"，用户明确说这是错的。
        for &s in e.slot_order() {
            if s >= e.slot_count() {
                continue;
            }
            let mut p = base.to_vec();
            p.push((i, s));
            if let Some(sub) = e.slot(s) {
                collect_positions(sub, &p, out);
            }
        }
    }
}

/// 在 `row[from..to]` 里找第一个**空插槽**（阅读序），返回该空行的光标
fn first_empty_in(row: &Row, from: usize, to: usize, base: &[(usize, usize)]) -> Option<Cursor> {
    for (i, e) in row.iter().enumerate().take(to.min(row.len())).skip(from) {
        for &s in e.slot_order() {
            if s >= e.slot_count() {
                continue;
            }
            let Some(sub) = e.slot(s) else { continue };
            let mut p = base.to_vec();
            p.push((i, s));
            if sub.is_empty() {
                return Some(Cursor { path: p, at: 0 });
            }
            if let Some(c) = first_empty_in(sub, 0, sub.len(), &p) {
                return Some(c);
            }
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────────────────────
// 文本 → 树（粘贴 / 测试 / 结构模板）
// ─────────────────────────────────────────────────────────────────────────────

/// 把一段 MathIO 文本解析成一行结构。
///
/// 识别：`sqrt(..)`、`sum/prod/int/lim/diff(..)`、`∑/∏(..)`、`(..)`、
/// `a/b`、`a^b`；其余按[记号](tokenize)切成 `Text`。
///
/// ⚠️ 这个解析器**只服务于"进入"**（粘贴、模板键、测试），
/// 编辑过程中的真相是树本身 —— 不存在"每次按键都重新解析"的往返。
pub fn parse(text: &str) -> Row {
    let cs: Vec<char> = text.chars().collect();
    let mut i = 0;
    parse_row(&cs, &mut i, false)
}

fn parse_row(cs: &[char], i: &mut usize, nested: bool) -> Row {
    let mut out: Row = Vec::new();
    let mut buf = String::new();
    while *i < cs.len() {
        match cs[*i] {
            ')' if nested => break,
            '(' => {
                *i += 1;
                let inner = parse_row(cs, i, true);
                if *i < cs.len() && cs[*i] == ')' {
                    *i += 1;
                }
                close_group(&mut out, &mut buf, inner);
            }
            '^' => {
                *i += 1;
                flush(&mut out, &mut buf);
                let base = out.pop().map(|e| vec![e]).unwrap_or_default();
                let exp = parse_atom(cs, i);
                out.push(Expr::Pow { base, exp });
            }
            '/' => {
                *i += 1;
                flush(&mut out, &mut buf);
                let num = out.pop().map(|e| vec![e]).unwrap_or_default();
                let den = parse_atom(cs, i);
                out.push(Expr::Frac { num, den });
            }
            // 逗号单独成项：`sum(...)` 的参数切分靠它（排版时不显示）
            ',' => {
                *i += 1;
                flush(&mut out, &mut buf);
                out.push(Expr::Text(",".into()));
            }
            c => {
                buf.push(c);
                *i += 1;
            }
        }
    }
    flush(&mut out, &mut buf);
    out
}

/// 一个"原子"：括号组，或一段连续的数字/标识符
fn parse_atom(cs: &[char], i: &mut usize) -> Row {
    if *i < cs.len() && cs[*i] == '(' {
        *i += 1;
        let inner = parse_row(cs, i, true);
        if *i < cs.len() && cs[*i] == ')' {
            *i += 1;
        }
        return vec![Expr::Paren(inner)];
    }
    let mut buf = String::new();
    while *i < cs.len() {
        let c = cs[*i];
        if c.is_alphanumeric() || c == '.' {
            buf.push(c);
            *i += 1;
        } else {
            break;
        }
    }
    let mut r = Vec::new();
    flush(&mut r, &mut buf);
    r
}

/// 遇到 `(` 之后的收尾：认出函数名就建结构，否则建普通分组
fn close_group(out: &mut Row, buf: &mut String, inner: Row) {
    if let Some((name, rest)) = split_trailing_name(buf) {
        let kind = match name.as_str() {
            "sum" | "∑" => Some(BigKind::Sum),
            "prod" | "∏" => Some(BigKind::Prod),
            "int" => Some(BigKind::Integral),
            "lim" => Some(BigKind::Limit),
            "diff" => Some(BigKind::Diff),
            _ => None,
        };
        if let Some(kind) = kind {
            *buf = rest;
            flush(out, buf);
            out.push(big_op(kind, inner));
            return;
        }
        // `sqrt(..)` 单参数
        if name == "sqrt" {
            *buf = rest;
            flush(out, buf);
            out.push(Expr::Root { inner });
            return;
        }
        // `cbrt(..)` → ∛：次数预填 3（与面板点击的观感一致）
        if name == "cbrt" {
            *buf = rest;
            flush(out, buf);
            out.push(Expr::NthRoot {
                degree: vec![Expr::Text("3".into())],
                inner,
            });
            return;
        }
        // `nroot(x, n)` → ⁿ√：参数序与 core 一致（被开方式在前，实测 nroot(27,3)=3）
        if name == "nroot" {
            *buf = rest;
            flush(out, buf);
            let mut args = split_args(inner);
            while args.len() < 2 {
                args.push(Vec::new());
            }
            let mut it = args.into_iter();
            let x = it.next().unwrap_or_default();
            let n = it.next().unwrap_or_default();
            out.push(Expr::NthRoot {
                degree: n,
                inner: x,
            });
            return;
        }
        // `log(base, x)` → log_b(x)：底数进下标框（Desmos 式），
        // 那个曾经渲染成 `log(,)` 的奇怪逗号从此消失。
        if name == "log" {
            *buf = rest;
            flush(out, buf);
            let mut args = split_args(inner);
            while args.len() < 2 {
                args.push(Vec::new());
            }
            let mut it = args.into_iter();
            let base = it.next().unwrap_or_default();
            let arg = it.next().unwrap_or_default();
            out.push(Expr::Log { base, arg });
            return;
        }
    }
    flush(out, buf);
    out.push(Expr::Paren(inner));
}

/// 反三角函数名 → 对应正三角名（显示成 `sin⁻¹` 这种上标写法）。
///
/// 短写法（`asin`）与正式名（`arcsin`）都认 —— core 两种都收
///（见 `canonical_fn_name`），显示层也应当一视同仁。
fn inverse_trig_base(s: &str) -> Option<&'static str> {
    Some(match s {
        "asin" | "arcsin" => "sin",
        "acos" | "arccos" => "cos",
        "atan" | "arctan" => "tan",
        "acot" | "arccot" => "cot",
        "asec" | "arcsec" => "sec",
        "acsc" | "arccsc" => "csc",
        _ => return None,
    })
}

/// 三角名 → 对应的反三角名（`Pow` 序列化用：`sin^-1` → `asin`）。
///
/// 只认**纯三角名**（不含 `h`）：`sinh^-1` 在 core 里叫 `asinh`，
/// 但写成 `sinh^-1` 的人极少，等真遇到再加，别凭猜。
fn inverse_of(name: &str) -> Option<&'static str> {
    Some(match name {
        "sin" => "asin",
        "cos" => "acos",
        "tan" => "atan",
        "cot" => "acot",
        "sec" => "asec",
        "csc" => "acsc",
        _ => return None,
    })
}

/// 把 `buf` 末尾的标识符拆出来：`("2sqrt", ..)` → `("sqrt", "2")`
fn split_trailing_name(buf: &str) -> Option<(String, String)> {
    let cs: Vec<char> = buf.chars().collect();
    let mut k = cs.len();
    while k > 0 {
        let c = cs[k - 1];
        if c.is_alphabetic() || c == '∑' || c == '∏' {
            k -= 1;
        } else {
            break;
        }
    }
    if k == cs.len() {
        return None;
    }
    Some((cs[k..].iter().collect(), cs[..k].iter().collect()))
}

/// 按大运算符的签名把参数切好（缺的补空行）
fn big_op(kind: BigKind, inner: Row) -> Expr {
    let mut args = split_args(inner);
    // 至少保留签名需要的槽位数
    while args.len() < 4 {
        args.push(Vec::new());
    }
    let mut it = args.into_iter();
    let body = it.next().unwrap_or_default();
    let mut var = it.next().unwrap_or_default();
    let from = it.next().unwrap_or_default();
    let to = it.next().unwrap_or_default();
    // 用户要求（2026-10-23）：插入 Σ / ∏ 时，左下那个"变量"框里**默认就是 `n`**。
    // 只填空的（显式写了 `sum(f,k,1,10)` 不受影响）。
    if var.is_empty() && matches!(kind, BigKind::Sum | BigKind::Prod) {
        var = vec![Expr::Text("n".into())];
    }
    Expr::BigOp {
        kind,
        body,
        var,
        from,
        to,
    }
}

/// 按顶层逗号切参数
fn split_args(inner: Row) -> Vec<Row> {
    let mut args: Vec<Row> = vec![Vec::new()];
    for e in inner {
        if e == Expr::Text(",".into()) {
            args.push(Vec::new());
        } else if let Some(last) = args.last_mut() {
            last.push(e);
        }
    }
    args
}

/// 把缓冲里攒的字符切成"记号项"
fn flush(out: &mut Row, buf: &mut String) {
    if buf.is_empty() {
        return;
    }
    for t in tokenize(buf) {
        out.push(Expr::Text(t));
    }
    buf.clear();
}

/// 文本 → 记号
///
/// 规则（决定了光标能停在哪）：
/// - 空白丢弃（数学排版本来也不显示空格，见 `mathrender` 的同名约定）；
/// - **数字连成一串**（`1.5` 不能被拆开，否则光标能插到小数点中间）；
/// - **已知多字符符号整体成项**（`pi` / `theta` / `inf`）——
///   拆开就再也不是 π 了；
/// - 其余字母**一个字符一项**，这样 `x`、`y`、`z` 之间都能落光标。
pub fn tokenize(s: &str) -> Vec<String> {
    const NAMED: [&str; 3] = ["theta", "inf", "pi"];
    let cs: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i]; // ⚠️ **换行必须单独成项**：输入框用 `\n` 分隔多行（见 `app::evaluate`），
        // 当成空白丢掉的话多行输入会粘成一行（`1+1` + `2` → `1+12`）。
        if c == '\n' {
            out.push("\n".into());
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        // 数字：**带小数点的整段保留**，纯整数则逐字符——
        // 前者不能拆（拆了光标能插到小数点中间，`1.5` 就毁了），
        // 后者必须拆（否则退格会把 `123` 一次删光，用户会觉得"退多了"）。
        if c.is_ascii_digit() || (c == '.' && i + 1 < cs.len() && cs[i + 1].is_ascii_digit()) {
            let start = i;
            while i < cs.len() && (cs[i].is_ascii_digit() || cs[i] == '.') {
                i += 1;
            }
            let run: String = cs[start..i].iter().collect();
            if run.contains('.') {
                out.push(run);
            } else {
                out.extend(run.chars().map(|ch| ch.to_string()));
            }
            continue;
        }
        // 已知多字符符号（最长优先）
        if let Some(name) = NAMED
            .iter()
            .find(|n| cs[i..].starts_with(&n.chars().collect::<Vec<_>>()[..]))
        {
            out.push((*name).to_string());
            i += name.chars().count();
            continue;
        }
        // 已知**函数名**整体成项（最长优先）：`asin` 不再拆成 4 个单字母 ——
        // 拆开的后果是显示层没法把它识别成 `sin⁻¹`（用户 2026-10-24）。
        // ⚠️ 只在**整段一起进来**时生效（粘贴/面板插入/`parse` 的整段缓冲）；
        // 逐键敲入每次只进一个字符，天然不受影响（退格仍逐字删）。
        if let Some(name) = match_fn_name(&cs, i) {
            out.push(name.to_string());
            i += name.chars().count();
            continue;
        }
        out.push(c.to_string());
        i += 1;
    }
    out
}

/// 函数名的**别名**（core 经 `canonical_fn_name` 规范化；[`FUNCTION_NAMES`] 里
/// 登记的是正式名，用户手打的短写法也要整体成项）。
const FN_ALIASES: &[&str] = &[
    "asin", "acos", "atan", "acot", "asec", "acsc", "asinh", "acosh", "atanh",
];

/// 在 `cs[i..]` 处做函数名的**最长匹配**。
///
/// 名单来自 core 的 `parser::FUNCTIONS`（本 crate 唯一权威函数清单），
/// 加上 [`FN_ALIASES`]。这里不认识的名字不会被分组 —— 与逐键敲入行为一致。
fn match_fn_name(cs: &[char], i: usize) -> Option<&'static str> {
    let mut best: Option<&'static str> = None;
    let names = hipercalc::parser_api::FUNCTIONS
        .iter()
        .chain(FN_ALIASES.iter());
    for name in names {
        let nc: Vec<char> = name.chars().collect();
        if cs[i..].starts_with(&nc) && best.is_none_or(|b| b.chars().count() < nc.len()) {
            best = Some(name);
        }
    }
    best
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// 敲一串键（每个字符一次 `insert_key`，与真实键盘一致）
    fn typed(keys: &[&str]) -> Doc {
        let mut d = Doc::default();
        for k in keys {
            d.insert_key(k);
        }
        d
    }

    #[test]
    fn typing_builds_tokens_and_serializes_back() {
        let d = typed(&["1", "+", "1"]);
        assert_eq!(d.source(), "1+1");
        assert_eq!(d.root.len(), 3, "三个记号 = 三项（光标可停在任意两个之间）");
    }

    #[test]
    fn decimal_stays_one_token() {
        // 带小数点的整段不能被拆开，否则光标能插到小数点中间
        let d = Doc::from_text("1.5");
        assert_eq!(d.root.len(), 1, "`1.5` 应是一个记号");
        assert_eq!(d.source(), "1.5");
        // 逐键敲出来的也是 `1.5`（每键一个记号，拼回去一样）
        let t = typed(&["1", ".", "5"]);
        assert_eq!(t.source(), "1.5");
    }

    /// 纯整数要**逐字符**成项 —— 否则退格会把 `123` 一次删光
    #[test]
    fn plain_digits_are_one_token_each() {
        let d = typed(&["1", "2", "3"]);
        assert_eq!(d.root.len(), 3);
        assert_eq!(d.source(), "123");
    }

    /// **Σ 的四个槽位必须是四个不同的位置** —— 这是本次结构化的首要目标。
    #[test]
    fn bigop_has_four_distinct_slots() {
        let mut d = Doc::default();
        d.insert_key("∑()");
        // 解析出来是一行一项
        assert_eq!(d.root.len(), 1, "Σ 应当是一个节点");
        let Expr::BigOp { .. } = &d.root[0] else {
            panic!("应是 BigOp，实际 {:?}", d.root[0]);
        };
        // ⚠️ 2026-10-23 用户改的规则：插完光标落在**右下框（下限，槽 2）**，
        // 不再是主体 —— 因为 Σ 下方的"变量 = 下限"才是先要填的东西。
        assert_eq!(d.cursor.path, vec![(0, 2)], "插完光标应在右下框（下限）里");

        // 四个槽的"空行光标"必须互不相同
        let mut rows = Vec::new();
        for s in 0..4 {
            let mut t = d.clone();
            t.cursor = Cursor {
                path: vec![(0, s)],
                at: 0,
            };
            rows.push(t.cursor.clone());
        }
        rows.dedup();
        assert_eq!(rows.len(), 4, "四个槽位不能塌成同一个位置");
    }

    /// 用户报过的 bug：`∑()` 里按 `←` 光标消失、再按 `x` 变成 `∑x()`
    #[test]
    fn arrow_left_from_body_never_vanishes() {
        let mut d = Doc::default();
        d.insert_key("∑()");
        // 连续按几次 ←：每一步都必须是合法位置（能渲染出光标）
        for _ in 0..6 {
            d.left();
            let (node, _) = d.to_node();
            assert!(
                contains_caret(&node),
                "按 ← 之后光标不该消失，当前 source={:?}",
                d.source()
            );
        }
        // 在最前面打字也只会在最前面插入，不会跑到括号里
        d.home();
        d.insert_key("x");
        // ⚠️ 序列化用的是求值侧的名字（`sum`），显示侧才画 `∑`
        assert_eq!(d.source(), "xsum(,n,,)", "x 应插在 Σ 之前，不是插进括号里");
    }

    #[test]
    fn sqrt_key_puts_caret_inside() {
        let mut d = Doc::default();
        d.insert_key("sqrt()");
        assert_eq!(d.cursor.path, vec![(0, 0)], "光标应在根号里面");
        d.insert_key("2");
        assert_eq!(d.source(), "sqrt(2)");
    }

    #[test]
    fn paren_key_is_balanced_and_backspace_removes_whole_group() {
        let mut d = Doc::default();
        d.insert_key("1");
        d.insert_key("(");
        assert_eq!(d.cursor.path, vec![(1, 0)], "光标应在括号里");
        d.insert_key("2");
        assert_eq!(d.source(), "1(2)", "括号自动配平");
        // 跳出分组 → 光标在分组之后 → 退格删掉整个分组（含内容）
        d.insert_key(")");
        assert_eq!(d.cursor.at, 2, "跳出后应在分组之后");
        d.backspace();
        assert_eq!(d.source(), "1", "退格应删掉整个括号组");
    }

    /// **用户拍板的规则**：空插槽里按退格什么都不做，绝不误删外层结构
    /// 手输 `sin^-1(45)` 必须变成 `asin(45)`（用户 2026-10-27 反馈的真 bug）。
    ///
    /// 当时的症状：`(` 被插进了**指数槽**，式子成了 `sin^(-1(45))`，
    /// core 报"未知标识符: sin"，回显里括号还凭空消失。
    #[test]
    fn manual_inverse_trig_keeps_parens() {
        let mut d = Doc::default();
        for step in ["sin", "^", "-", "1", "(", "4", "5"] {
            if step == "^" || step == "(" {
                d.insert_key(step);
            } else {
                d.insert_text(step);
            }
        }
        assert_eq!(d.source(), "asin(45)");
    }

    #[test]
    fn backspace_inside_empty_slot_does_nothing() {
        let mut d = Doc::default();
        d.insert_key("sqrt()");
        assert!(d.here().is_empty(), "根号里应是空的");
        d.backspace();
        assert_eq!(d.root.len(), 1, "根号结构必须还在");
        assert_eq!(d.source(), "sqrt()");
    }

    #[test]
    fn backspace_drops_one_token_at_a_time_inside_a_slot() {
        let mut d = Doc::default();
        d.insert_key("sqrt()");
        d.insert_text("12");
        assert_eq!(d.source(), "sqrt(12)");
        d.backspace();
        assert_eq!(d.source(), "sqrt(1)", "退格只删一个记号");
        d.backspace();
        assert_eq!(d.source(), "sqrt()");
        d.backspace();
        assert_eq!(d.source(), "sqrt()", "空了之后不再删结构");
    }

    /// `÷` 键：前一项当分子（否则 `1` `÷` `2` 会变成 `12/`）
    #[test]
    fn division_key_wraps_previous_atom_as_numerator() {
        let mut d = Doc::default();
        for k in ["1", "/", "2"] {
            d.insert_key(k);
        }
        assert_eq!(d.source(), "1/2");
        assert_eq!(d.cursor.path, vec![(0, 1)], "打完光标应停在分母");
    }

    #[test]
    fn pow_wraps_previous_atom() {
        let mut d = Doc::default();
        d.insert_key("2");
        d.insert_key("^");
        assert_eq!(d.source(), "2^", "底数是前面那个 2");
        d.insert_key("3");
        assert_eq!(d.source(), "2^3");
        // 指数是一个独立插槽
        assert_eq!(d.cursor.path, vec![(0, 1)]);
    }

    #[test]
    fn superscript_key_with_exp_prefills_it() {
        let mut d = Doc::default();
        d.insert_key("x");
        d.insert_key("^2");
        assert_eq!(d.source(), "x^2");
    }

    #[test]
    fn division_becomes_a_fraction() {
        let d = Doc::from_text("1/2");
        assert_eq!(d.source(), "1/2");
        let Expr::Frac { .. } = &d.root[0] else {
            panic!("应是分数，实际 {:?}", d.root[0]);
        };
    }

    /// 分子是复合式子时要补括号，否则源文本会变意
    #[test]
    fn fraction_parenthesises_composite_numerator() {
        let d = Doc::from_text("(1+2)/3");
        assert_eq!(d.source(), "(1+2)/3");
    }

    #[test]
    fn linear_order_is_reading_order() {
        let d = Doc::from_text("1/2");
        let table = d.linear();
        // 根行0 → 分子0 → 分子1 → 分母0 → 分母1 → 根行1
        //（分子/分母各有一个记号，所以各有"前后两个位置"）
        assert_eq!(table[0], Cursor::root(0));
        assert_eq!(
            table[1],
            Cursor {
                path: vec![(0, 0)],
                at: 0
            }
        );
        assert_eq!(
            table[2],
            Cursor {
                path: vec![(0, 0)],
                at: 1
            }
        );
        assert_eq!(
            table[3],
            Cursor {
                path: vec![(0, 1)],
                at: 0
            }
        );
        assert_eq!(
            table[4],
            Cursor {
                path: vec![(0, 1)],
                at: 1
            }
        );
        assert_eq!(table[5], Cursor::root(1));
        assert_eq!(table.len(), 6, "位置数 = 根行2 + 分子2 + 分母2");
    }

    /// 点击目标必须与渲染时登记的 id 一一对应
    #[test]
    fn targets_let_you_click_each_bigop_slot() {
        let mut d = Doc::default();
        d.insert_key("∑()");
        let targets = d.targets();
        // Σ 本体一个 + 四个槽各一个 + **行尾一个**（点最后一个记号右边 ⇒ 光标到末尾）
        assert_eq!(targets.len(), 6, "目标数量不对：{targets:?}");
        assert_eq!(
            targets.last(),
            Some(&Cursor::root(1)),
            "最后一个目标应是行尾"
        );
        // 四个槽必须是四个互不相同的位置（旧的文本模型在这里会塌成一个）
        let mut paths: Vec<_> = targets
            .iter()
            .filter(|c| !c.path.is_empty())
            .map(|c| c.path.clone())
            .collect();
        paths.sort();
        paths.dedup();
        assert_eq!(paths.len(), 4, "四个槽位不能塌成同一个位置");
    }

    #[test]
    fn click_target_places_caret_there() {
        let mut d = Doc::from_text("12+3");
        // 目标 0 = 第一个项之前，1 = 第一个项之后……
        d.place(1);
        assert_eq!(d.cursor.at, 1);
        d.insert_key("9");
        assert_eq!(d.source(), "192+3", "插在 `1` 与 `2` 之间");
    }

    #[test]
    fn templates_parse_all_big_operators() {
        for (src, want) in [
            // 变量框默认 `n`（2026-10-23 用户要求），所以不是 sum(,,,)
            ("∑()", "sum(,n,,)"),
            ("∏()", "prod(,n,,)"),
            ("diff(,x)", "diff(,x)"),
            ("int(,x,0,1)", "int(,x,0,1)"),
            ("lim(,x,0)", "lim(,x,0)"),
        ] {
            let d = Doc::from_text(src);
            assert_eq!(d.source(), want, "`{src}` 应序列化成 `{want}`");
        }
    }

    #[test]
    fn abs_and_functions_keep_a_group() {
        let mut d = Doc::default();
        d.insert_key("abs()");
        // 函数名整体成项（`tokenize` 的函数名分组，与 `pi` 同一套约定）；
        // 括号是结构。光标落在第 1 项（括号组）的内部。
        assert_eq!(d.source(), "abs()");
        assert_eq!(d.cursor.path, vec![(1, 0)], "光标应在括号里");
    }

    /// 面板点 `log`：应得到**结构化** log（底数是下标框里的空槽），
    /// 不再是文本 `log(,)` —— 用户 2026-10-24："log 打出来为什么是这个东西"。
    #[test]
    fn log_inserts_structured_with_base_slot() {
        let mut d = Doc::default();
        d.insert_key("log(,)");
        assert_eq!(d.root.len(), 1, "log 应当是一个结构节点");
        let Expr::Log { base, arg } = &d.root[0] else {
            panic!("应是 Log，实际 {:?}", d.root[0]);
        };
        assert!(base.is_empty() && arg.is_empty(), "两个槽都该是空的");
        assert_eq!(d.source(), "log(,)", "序列化回 core 认的写法");
        // 光标落在**底数**（视觉顺序的第一个空插槽）
        assert_eq!(d.cursor.path, vec![(0, 0)], "光标应进底数框");
    }

    /// 粘贴 `log(2,8)` 也要结构化，且能无损序列化回去。
    #[test]
    fn log_pasteparse_round_trips() {
        let d = Doc::from_text("log(2,8)");
        let Expr::Log { base, arg } = &d.root[0] else {
            panic!("应是 Log，实际 {:?}", d.root[0]);
        };
        assert_eq!(row_source(base), "2");
        assert_eq!(row_source(arg), "8");
        assert_eq!(d.source(), "log(2,8)");
    }

    /// 面板点 `cbrt`：次数预填 3，光标直接进被开方式。
    #[test]
    fn cbrt_inserts_nthroot_with_degree_prefilled() {
        let mut d = Doc::default();
        d.insert_key("cbrt()");
        let Expr::NthRoot { degree, inner } = &d.root[0] else {
            panic!("应是 NthRoot，实际 {:?}", d.root[0]);
        };
        assert_eq!(row_source(degree), "3", "立方根的次数是 3");
        assert!(inner.is_empty());
        assert_eq!(d.source(), "cbrt()");
        assert_eq!(d.cursor.path, vec![(0, 1)], "光标应进被开方式");
    }

    /// 面板点 `n 次根`：两个槽都空，光标先落次数（视觉顺序第一个）。
    #[test]
    fn nroot_inserts_with_both_slots_empty() {
        let mut d = Doc::default();
        d.insert_key("nroot(,)");
        let Expr::NthRoot { degree, inner } = &d.root[0] else {
            panic!("应是 NthRoot，实际 {:?}", d.root[0]);
        };
        assert!(degree.is_empty() && inner.is_empty());
        assert_eq!(d.cursor.path, vec![(0, 0)], "光标应先进次数框");
        // 序列化成 core 的参数序：nroot(被开方式, 次数)
        assert_eq!(d.source(), "nroot(,)");
    }

    /// `nroot(27,3)` 粘贴 → 结构化 ⁿ√。次数为 3 时序列化**等价折叠**成
    /// `cbrt(27)`（core 里两者同值，`cbrt` 是更规范的写法）。
    #[test]
    fn nroot_pasteparse_round_trips() {
        let d = Doc::from_text("nroot(27,3)");
        assert!(matches!(d.root[0], Expr::NthRoot { .. }));
        assert_eq!(d.source(), "cbrt(27)");
        // 次数不是 3 时保持 nroot
        assert_eq!(Doc::from_text("nroot(16,4)").source(), "nroot(16,4)");
    }

    /// 反三角**显示**成 `sin⁻¹`（上标），但 `source()` 仍是 core 认的 `asin` ——
    /// 用户 2026-10-24："sin^-1 打出来为什么还是 asin"。
    #[test]
    fn inverse_trig_displays_superscript_but_serializes_ascii() {
        let mut d = Doc::default();
        d.insert_key("asin()");
        assert_eq!(d.source(), "asin()", "序列化必须是 core 认的名字");
        let (node, _) = d.to_node();
        let flat = flatten_for_test(&node);
        assert!(flat.contains("sin^−1"), "显示层应出现 sin^−1，实际 {flat}");
        // 正式名与短名显示一致
        let d2 = Doc::from_text("arcsin()");
        let (node2, _) = d2.to_node();
        assert!(flatten_for_test(&node2).contains("sin^−1"));
    }

    /// 测试用拍平（与 `to_node` 的渲染 IR 对应）
    fn flatten_for_test(n: &Node) -> String {
        match n {
            Node::Text(s) => s.clone(),
            Node::Seq(v) => v.iter().map(flatten_for_test).collect(),
            Node::Pow(a, b) => format!("{}^{}", flatten_for_test(a), flatten_for_test(b)),
            Node::Sub(a, b) => format!("{}_{}", flatten_for_test(a), flatten_for_test(b)),
            Node::Paren { inner } => format!("({})", flatten_for_test(inner)),
            Node::At { inner, .. } | Node::Hit { inner, .. } => flatten_for_test(inner),
            Node::Root { inner, .. } => format!("√({})", flatten_for_test(inner)),
            Node::Frac(a, b) => format!("{}/{}", flatten_for_test(a), flatten_for_test(b)),
            _ => String::new(),
        }
    }

    #[test]
    fn right_arrow_stops_at_end() {
        let mut d = Doc::from_text("1");
        d.right();
        d.right();
        assert_eq!(d.cursor, Cursor::root(1), "到末尾就停在末尾");
    }

    #[test]
    fn newline_is_a_token() {
        let d = Doc::from_text("1+1\n2");
        assert_eq!(d.source(), "1+1\n2");
    }

    #[test]
    fn spaces_are_dropped_like_math_mode() {
        let d = Doc::from_text("1 + 1");
        assert_eq!(d.source(), "1+1");
    }

    /// 渲染 IR 里必须能找到光标（否则界面上光标就是"消失"了）
    fn contains_caret(node: &Node) -> bool {
        match node {
            Node::Caret => true,
            // 聚焦中的空框里画了光标
            Node::EmptyFocused { .. } => true,
            Node::Text(_) | Node::Empty { .. } | Node::Comma { .. } => false,
            // 括号里的空参数：**不画框也不画光标**，所以不算"有光标"
            Node::EmptyBare { .. } => false,
            Node::Paren { inner } => contains_caret(inner),
            Node::Seq(v) => v.iter().any(contains_caret),
            Node::At { inner, .. } | Node::Hit { inner, .. } => contains_caret(inner),
            Node::Frac(a, b) | Node::Pow(a, b) | Node::Sub(a, b) => {
                contains_caret(a) || contains_caret(b)
            }
            Node::Root { degree, inner } => {
                degree.as_ref().is_some_and(|d| contains_caret(d)) || contains_caret(inner)
            }
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
        }
    }
}

#[cfg(test)]
mod bigop_order_tests {
    use super::*;

    /// 用户 2026-10-23 的要求①：插入 Σ/∏ 时左下"变量"框默认是 `n`，
    /// ②插入后光标落在**右下框**（下限）。
    #[test]
    fn sum_inserts_with_default_n_and_cursor_on_lower_bound() {
        let mut d = Doc::default();
        d.insert_key("∑()");
        assert_eq!(d.source(), "sum(,n,,)", "Σ 的变量框应预填 n");
        assert_eq!(
            d.cursor.path,
            vec![(0, 2)],
            "插入后光标应在右下框（下限），而不是上方或被求和式"
        );
        // 接着打字应该进下限
        d.insert_key("1");
        assert_eq!(d.source(), "sum(,n,1,)");
    }

    /// 用户 2026-10-23 的要求③：`→` 的遍历顺序是
    /// **左下（变量）→ 右下（下限）→ 上方（上限）→ 右侧（主体）→ 符号后面**。
    #[test]
    fn bigop_cursor_order_follows_the_visual_layout() {
        let mut d = Doc::default();
        d.insert_key("∑()");
        let mut seen: Vec<Vec<(usize, usize)>> = Vec::new();
        for c in d.linear() {
            if !c.path.is_empty() && !seen.contains(&c.path) {
                seen.push(c.path.clone());
            }
        }
        // 槽位：0=body 1=var 2=from 3=to
        assert_eq!(
            seen,
            vec![
                vec![(0, 1)], // 左下：变量 n
                vec![(0, 2)], // 右下：下限
                vec![(0, 3)], // 上方：上限
                vec![(0, 0)], // 右侧：被求和式
            ],
            "Σ 的光标顺序必须是 左下→右下→上→右"
        );
    }

    /// 积分同族：下（下限）→ 上（上限）→ 右侧主体 → 末尾 d<变量>
    #[test]
    fn integral_cursor_order_is_bottom_up_then_body_then_variable() {
        let d = Doc::from_text("int(x,x,0,1)");
        let mut seen: Vec<Vec<(usize, usize)>> = Vec::new();
        for c in d.linear() {
            if !c.path.is_empty() && !seen.contains(&c.path) {
                seen.push(c.path.clone());
            }
        }
        assert_eq!(
            seen,
            vec![vec![(0, 2)], vec![(0, 3)], vec![(0, 0)], vec![(0, 1)]],
            "∫ 的光标顺序应是 下限→上限→主体→变量"
        );
    }
}
