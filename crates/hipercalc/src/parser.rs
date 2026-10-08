use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive, Zero};

use hipercalc_core::bigfloat::{self, BigFloat};
use hipercalc_core::number::{ExactExpr, ExactTerm, Number};
use hipercalc_core::trig::{self, AngleMode};

/// 有效的单字母变量名
/// 有效的单字母变量名。
/// 注意：**不含小写 `i`**——它被虚数单位占用（`2i`、`i^2` 等）；
/// 这也是相对旧版本的破坏性变更：以往 `i` 可作未知数（如 `i^2-4=0`），现在表示虚数单位。
/// 内置物理常量：`(短名, 长名别名, 十进制字面量)`。
///
/// - 数值取 **CODATA 2022**；SI 定义值逐位精确（如光速 299792458 是定义值）。
/// - 名字**从变量命名空间收回**：`/let` 用这些名字会报错（只读内置常量）——
///   这是用户明确选定的取舍，因为习惯写法 `c`/`h`/`k` 已被单字母变量占死。
/// - 短名与长名都给，覆盖两种书写习惯。
pub const PHYS_CONSTANTS: &[(&str, &str, &str)] = &[
    ("C0", "LIGHT_SPEED", "299792458"),
    ("G0", "GRAV_CONST", "6.67430e-11"),
    ("HPL", "PLANCK", "6.62607015e-34"),
    ("HBAR", "REDUCED_PLANCK", "1.0545718176461565e-34"),
    ("KB", "BOLTZMANN", "1.380649e-23"),
    ("NA", "AVOGADRO", "6.02214076e23"),
    ("QE", "ELEMENTARY_CHARGE", "1.602176634e-19"),
    ("ME", "ELECTRON_MASS", "9.1093837139e-31"),
    ("MP", "PROTON_MASS", "1.67262192595e-27"),
    ("MN", "NEUTRON_MASS", "1.67492750056e-27"),
    ("RGAS", "GAS_CONST", "8.31446261815324"),
    ("SIGMA", "STEFAN_BOLTZMANN", "5.670374419e-8"),
    ("ALPHA", "FINE_STRUCTURE", "7.2973525643e-3"),
    ("MU0", "VACUUM_PERMEABILITY", "1.25663706212e-6"),
    ("EPS0", "VACUUM_PERMITTIVITY", "8.8541878128e-12"),
    ("GACC", "EARTH_G", "9.80665"),
];

/// 单位系数表：`(单位名, 折成 SI 的系数)`。系数写成十进制字面量 ⇒ 解析后是**精确有理数**。
///
/// **只收多字母单位**：单字母（`m`/`s`/`g`/`A`/`K`…）与变量命名空间冲突，
/// 刻意排除（可写 `meter`/`second`/`gram`）。`KB` 也不收 —— 它是玻尔兹曼常量。
/// 用法：`3 km` / `3km` / `500 m` 在解析期就折成 SI（`3 km` → `3*1000` → `3000`）。
/// 比较运算符 → 编码（与 `solver_ineq` 的常量一致）
pub fn classify_op(op: &str) -> (&'static str, u8) {
    match op {
        "<" => ("<", crate::solver_ineq::OP_LT),
        "<=" | "=<" | "≤" => ("<=", crate::solver_ineq::OP_LE),
        ">" => (">", crate::solver_ineq::OP_GT),
        ">=" | "=>" | "≥" => (">=", crate::solver_ineq::OP_GE),
        _ => ("!=", crate::solver_ineq::OP_NE),
    }
}

/// 单位表：`(单位名, 折成 SI 的系数, 量纲)`。
///
/// **只收多字母单位**：单字母（`m`/`s`/`g`/`A`/`K`…）与变量命名空间冲突，刻意排除
/// （可写 `meter`/`second`/`gram`）。`KB` 也不收 —— 它是玻尔兹曼常量。
/// 量纲顺序固定为 m, kg, s, A, K, mol, cd（见 `hipercalc_core::quantity`）。
/// 第 9 项起，`3 km` 不再只是"折成 3000"，而是**带量纲的值**（`3000` 配 `L`）。
pub const PHYS_UNITS: &[(&str, &str, hipercalc_core::quantity::Dim)] = &[
    // 长度 L
    ("meter", "1", hipercalc_core::quantity::LEN),
    ("metre", "1", hipercalc_core::quantity::LEN),
    ("km", "1000", hipercalc_core::quantity::LEN),
    ("cm", "0.01", hipercalc_core::quantity::LEN),
    ("mm", "0.001", hipercalc_core::quantity::LEN),
    ("um", "0.000001", hipercalc_core::quantity::LEN),
    ("nm", "0.000000001", hipercalc_core::quantity::LEN),
    ("inch", "0.0254", hipercalc_core::quantity::LEN),
    ("foot", "0.3048", hipercalc_core::quantity::LEN),
    ("feet", "0.3048", hipercalc_core::quantity::LEN),
    ("yard", "0.9144", hipercalc_core::quantity::LEN),
    ("mile", "1609.344", hipercalc_core::quantity::LEN),
    ("nmi", "1852", hipercalc_core::quantity::LEN),
    // 时间 T
    ("second", "1", hipercalc_core::quantity::TIME),
    ("sec", "1", hipercalc_core::quantity::TIME),
    ("ms", "0.001", hipercalc_core::quantity::TIME),
    ("us", "0.000001", hipercalc_core::quantity::TIME),
    ("ns", "0.000000001", hipercalc_core::quantity::TIME),
    ("minute", "60", hipercalc_core::quantity::TIME),
    ("min", "60", hipercalc_core::quantity::TIME),
    ("hour", "3600", hipercalc_core::quantity::TIME),
    ("day", "86400", hipercalc_core::quantity::TIME),
    ("week", "604800", hipercalc_core::quantity::TIME),
    // 质量 M
    ("gram", "0.001", hipercalc_core::quantity::MASS),
    ("kg", "1", hipercalc_core::quantity::MASS),
    ("mg", "0.000001", hipercalc_core::quantity::MASS),
    ("tonne", "1000", hipercalc_core::quantity::MASS),
    ("pound", "0.45359237", hipercalc_core::quantity::MASS),
    ("oz", "0.028349523125", hipercalc_core::quantity::MASS),
    // 体积 L³ / 面积 L²
    ("liter", "0.001", hipercalc_core::quantity::VOLUME),
    ("litre", "0.001", hipercalc_core::quantity::VOLUME),
    ("ml", "0.000001", hipercalc_core::quantity::VOLUME),
    ("hectare", "10000", hipercalc_core::quantity::AREA),
    ("acre", "4046.8564224", hipercalc_core::quantity::AREA),
    // 速度 L/T
    (
        "kph",
        "0.2777777777777777777777777777777777777777",
        hipercalc_core::quantity::SPEED,
    ),
    ("mph", "0.44704", hipercalc_core::quantity::SPEED),
    (
        "knot",
        "0.5144444444444444444444444444444444444444",
        hipercalc_core::quantity::SPEED,
    ),
    // 能量 M·L²/T² / 压强 M/(L·T²)
    ("cal", "4.184", hipercalc_core::quantity::ENERGY),
    (
        "eV",
        "0.0000000000000000001602176634",
        hipercalc_core::quantity::ENERGY,
    ),
    ("bar", "100000", hipercalc_core::quantity::PRESSURE),
    ("atm", "101325", hipercalc_core::quantity::PRESSURE),
];

/// 取某个单位的 SI 系数（供 `/unit` 指令使用）。
///
/// 表里的系数是十进制字面量 ⇒ 这里复用**既有的数字解析**，不另写一套字符串→数值转换。
pub fn unit_factor(name: &str) -> Option<Number> {
    let text = PHYS_UNITS
        .iter()
        .find(|(u, _, _)| *u == name)
        .map(|(_, f, _)| *f)?;
    let mut p = Parser::new(text);
    match p.parse_number() {
        Ok(Expr::Number(n)) => Some(n),
        _ => None,
    }
}

/// 该名字是否是内置物理常量（短名或长名）——`/let` 用它来做重名检查
pub fn is_phys_constant(name: &str) -> bool {
    PHYS_CONSTANTS
        .iter()
        .any(|(short, long, _)| *short == name || *long == name)
}

pub const VALID_VARIABLES: &str = "xyzabcdefghjklmnopqrstuvwABCDEFGHIJKLMNOPQRSTUVW";

/// 白名单函数名：解析校验、多字母拆分、REPL 高亮共用同一份常量。
/// 新增函数时只需改这里 + `Evaluator::eval_function` 两处（旧实现有三份重复数组，易漏改）。
pub const FUNCTIONS: &[&str] = &[
    "sqr",
    "sqrt",
    "sin",
    "cos",
    "tan",
    "cot",
    "sec",
    "csc",
    "arcsin",
    "arccos",
    "arctan",
    "arccot",
    "arcsec",
    "arccsc",
    "abs",
    "sd",
    "factor",
    "fac",
    "primefac",
    "triangle",
    "diff",
    "int",
    "lim",
    "taylor",
    "sum",
    "prod",
    "ln",
    "exp",
    "log",
    "log10",
    "log2",
    "floor",
    "ceil",
    "round",
    "frac",
    "sign",
    "sinh",
    "cosh",
    "tanh",
    "coth",
    "sech",
    "csch",
    "arcsinh",
    "arccosh",
    "arctanh",
    "cbrt",
    "nroot",
    "mod",
    "idiv",
    "nCr",
    "nPr",
    "gcd",
    "lcm",
    "isprime",
    "nextprime",
    "re",
    "im",
    "conj",
    "arg",
    "and",
    "or",
    "xor",
    "not",
    "shl",
    "shr",
    "fib",
    "catalan",
    "doublefac",
    "gamma",
    "erf",
    "erfc",
    "zeta",
    "mean",
    "var",
    "var_s",
    "stddev",
    "stddev_s",
    "median",
    "percentile",
    "corr",
    "list",
    "ineq",
    "det",
    "inv",
    "trace",
    "transpose",
    "rank",
    "eigen",
    "linsolve",
];

/// 需要两个参数的函数（其余函数都是单参；`log` 有专门的报错文案，单独处理）
pub const TWO_ARG_FUNCTIONS: &[&str] = &[
    "nroot", "mod", "idiv", "nCr", "nPr", "gcd", "lcm", "and", "or", "xor", "shl", "shr",
];

/// 分量操作：实部 / 虚部 / 共轭 / 辐角。它们对**实数**同样有定义（实数 = 虚部为 0 的复数），
/// 因此不受"实参含复数才走复数路径"的限制 —— 否则 `re(2)`、`arg(-1)` 会误报"未知函数"。
const COMPONENT_FUNCTIONS: &[&str] = &["re", "im", "conj", "arg"];

/// 取整数参数（要求精确有理数且为整数），失败时用 `err` 文案报错
fn as_int(v: &Number, err: &str) -> Result<BigInt, String> {
    match v.as_rational() {
        Some(r) if r.is_integer() => Ok(r.to_integer()),
        _ => Err(err.to_string()),
    }
}

/// 取非负整数参数
fn as_nonneg_int(v: &Number, err: &str) -> Result<BigInt, String> {
    let n = as_int(v, err)?;
    if n.is_negative() {
        return Err(err.to_string());
    }
    Ok(n)
}

/// 表达式 AST
#[derive(Debug, Clone)]
pub enum Expr {
    Number(Number),
    Variable(String),
    Binary(Box<Expr>, BinOp, Box<Expr>),
    Unary(UnaryOp, Box<Expr>),
    Pow(Box<Expr>, Box<Expr>),
    /// 函数调用，参数列表（绝大多数函数单参，log 支持 base, x 两参）
    Function(String, Vec<Expr>),
    Sd(Box<Expr>),
    /// 因式分解: factor(表达式)，必须是顶层
    Factor(Box<Expr>),
    /// 等式: left = right
    Equation(Box<Expr>, Box<Expr>),
    /// 方程组: 多个等式，逗号分隔
    System(Vec<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnaryOp {
    Pos,
    Neg,
}

/// 显示模式
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DisplayMode {
    MathIO,
    LineIO,
}

/// 解析器
pub struct Parser {
    input: Vec<char>,
    pos: usize,
    /// 绝对值嵌套深度：>0 时 '|' 视为闭合符，不被隐式乘法吞掉
    abs_depth: usize,
}

impl Parser {
    pub fn new(input: &str) -> Self {
        Parser {
            input: input.chars().collect(),
            pos: 0,
            abs_depth: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.input.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<char> {
        let ch = self.peek();
        if ch.is_some() {
            self.pos += 1;
        }
        ch
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() {
                self.next();
            } else {
                break;
            }
        }
    }

    /// 判断下一个字符是否暗示隐式乘法（数字、字母、左括号）
    fn is_implicit_mul_start(&self) -> bool {
        match self.peek() {
            Some(ch) if ch.is_ascii_digit() || ch == '.' => true,
            Some(ch) if ch.is_alphabetic() || ch == '_' => true,
            Some('(') => true,
            // 数字或括号后直接跟绝对值符号视为隐式乘法（如 2|x|、(a)|b|）；
            // 但绝对值内部的闭合 '|'（abs_depth > 0）不是乘法，必须放行
            Some('|') => self.abs_depth == 0,
            _ => false,
        }
    }

    /// 尝试解析 sd(表达式) —— 必须是顶层函数
    pub fn parse_sd(&mut self) -> Result<Expr, String> {
        self.skip_whitespace();
        if self.try_parse_identifier("sd") {
            self.skip_whitespace();
            if self.peek() != Some('(') {
                return Err("sd 函数需要参数: sd(表达式)".to_string());
            }
            self.next(); // 跳过 '('
            let inner = self.parse_equation()?; // 允许内部含有等式
            self.skip_whitespace();
            if self.peek() != Some(')') {
                return Err("缺少右括号 ')'".to_string());
            }
            self.next();
            self.skip_whitespace();
            if self.pos != self.input.len() {
                return Err("sd 必须是整个表达式的最外层函数".to_string());
            }
            return Ok(Expr::Sd(Box::new(inner)));
        }
        Err("不是 sd 表达式".to_string())
    }

    /// 尝试解析 fac(表达式) / factor(表达式) —— 必须是顶层函数
    pub fn parse_factor(&mut self) -> Result<Expr, String> {
        self.skip_whitespace();
        let is_name = self.try_parse_identifier("factor") || self.try_parse_identifier("fac");
        if is_name {
            self.skip_whitespace();
            if self.peek() != Some('(') {
                return Err("fac/factor 函数需要参数: fac(表达式)".to_string());
            }
            self.next(); // 跳过 '('
            let inner = self.parse_expression()?;
            self.skip_whitespace();
            if self.peek() != Some(')') {
                return Err("缺少右括号 ')'".to_string());
            }
            self.next();
            self.skip_whitespace();
            if self.pos != self.input.len() {
                return Err("fac/factor 必须是整个表达式的最外层函数".to_string());
            }
            return Ok(Expr::Factor(Box::new(inner)));
        }
        Err("不是 fac/factor 表达式".to_string())
    }

    /// 解析方程组（逗号分隔多个等式）
    /// 如: x + y = 5, 2x - y = 1
    pub fn parse_system(&mut self) -> Result<Expr, String> {
        let mut equations: Vec<Expr> = Vec::new();

        loop {
            self.skip_whitespace();
            if self.pos >= self.input.len() {
                break;
            }
            let eq = self.parse_equation()?;
            equations.push(eq);

            self.skip_whitespace();
            if self.peek() == Some(',') {
                self.next();
            } else {
                break;
            }
        }

        if equations.len() == 1 {
            Ok(equations.into_iter().next().unwrap())
        } else {
            Ok(Expr::System(equations))
        }
    }

    /// 识别比较运算符：返回 (运算符原文, 字符数)。注意 **两字符优先**（`<=` 不能被当成 `<`）
    fn peek_comparison_op(&self) -> Option<(&'static str, usize)> {
        let c0 = *self.input.get(self.pos)?;
        let c1 = self.input.get(self.pos + 1).copied();
        match (c0, c1) {
            ('<', Some('=')) => Some(("<=", 2)),
            ('=', Some('=')) => None, // `==` 不认（保持既有语义）
            ('=', Some('<')) => Some(("=<", 2)),
            ('>', Some('=')) => Some((">=", 2)),
            ('=', Some('>')) => Some(("=>", 2)),
            ('!', Some('=')) => Some(("!=", 2)),
            ('<', _) => Some(("<", 1)),
            ('>', _) => Some((">", 1)),
            ('≤', _) => Some(("≤", 1)),
            ('≥', _) => Some(("≥", 1)),
            ('≠', _) => Some(("≠", 1)),
            _ => None,
        }
    }

    /// 解析等式或表达式
    /// 先解析左边表达式，若紧跟 = 则解析右边并返回 Equation
    pub fn parse_equation(&mut self) -> Result<Expr, String> {
        let left = self.parse_expression()?;
        self.skip_whitespace();
        // 比较运算符 → 脱糖成 Function("ineq", [左边, 右边, 运算符编码])。
        // **不新增 Expr 变体**：`Expr` 在 14 个文件里被穷尽匹配，加变体会牵动一大片；
        // 复用 Function（与统计函数、`[...]` 列表同一手法）零波及。
        // 运算符两种写法都收：>= 与 =>、<= 与 =<（用户要求），另有 != 与 Unicode ≠。
        if let Some((op, len)) = self.peek_comparison_op() {
            let (_, code) = classify_op(op);
            for _ in 0..len {
                self.next();
            }
            let right = self.parse_expression()?;
            return Ok(Expr::Function(
                "ineq".to_string(),
                vec![left, right, Expr::Number(Number::from_int(code as i64))],
            ));
        }
        if self.peek() == Some('=') {
            self.next(); // 跳过 '='
            let right = self.parse_expression()?;
            Ok(Expr::Equation(Box::new(left), Box::new(right)))
        } else {
            Ok(left)
        }
    }

    /// 解析完整表达式
    pub fn parse_expression(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_term()?;
        loop {
            self.skip_whitespace();
            match self.peek() {
                Some('+') => {
                    self.next();
                    let right = self.parse_term()?;
                    left = Expr::Binary(Box::new(left), BinOp::Add, Box::new(right));
                }
                Some('-') => {
                    self.next();
                    let right = self.parse_term()?;
                    left = Expr::Binary(Box::new(left), BinOp::Sub, Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_term(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_unary()?;
        loop {
            self.skip_whitespace();
            match self.peek() {
                Some('*') => {
                    self.next();
                    let right = self.parse_unary()?;
                    left = Expr::Binary(Box::new(left), BinOp::Mul, Box::new(right));
                }
                Some('/') => {
                    self.next();
                    let right = self.parse_unary()?;
                    left = Expr::Binary(Box::new(left), BinOp::Div, Box::new(right));
                }
                // 隐式乘法: 数字后跟变量、(后跟(、)后跟(、变量后跟(
                _ => {
                    if self.is_implicit_mul_start() {
                        let right = self.parse_unary()?;
                        left = Expr::Binary(Box::new(left), BinOp::Mul, Box::new(right));
                    } else {
                        break;
                    }
                }
            }
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        self.skip_whitespace();
        match self.peek() {
            Some('+') => {
                self.next();
                // 递归解析，支持连用一元符号（如 -----5）
                let expr = self.parse_unary()?;
                Ok(Expr::Unary(UnaryOp::Pos, Box::new(expr)))
            }
            Some('-') => {
                self.next();
                // 递归解析，支持连用一元符号（如 -----5）
                let expr = self.parse_unary()?;
                Ok(Expr::Unary(UnaryOp::Neg, Box::new(expr)))
            }
            _ => self.parse_power(),
        }
    }

    fn parse_power(&mut self) -> Result<Expr, String> {
        let mut base = self.parse_atom()?;
        // 后缀阶乘 !（紧贴操作数，优先级高于幂：2^3! = 2^(3!)）
        // 使用内部函数名 "fact"，白名单不含该名，用户无法直接输入 fact(...)
        self.skip_whitespace();
        // 注意前瞻： 后面若跟 ，那是**不等号** （用户要求的不等写法），不是阶乘
        while self.peek() == Some('!') && self.input.get(self.pos + 1) != Some(&'=') {
            self.next();
            base = Expr::Function("fact".to_string(), vec![base]);
        }
        self.skip_whitespace();
        if self.peek() == Some('^') {
            self.next();
            // 右结合: a^b^c = a^(b^c)；指数走 parse_unary 以支持负指数（如 2^-2）
            let exp = self.parse_unary()?;
            // 常量指数折叠：x^(2-1) → x^1、x^(6/3) → x^2
            // 不折叠的话，下游的多项式判定（`is_polynomial`）、线性判定（`check_linear`）
            // 与系数提取（`collect_terms`）都只认"字面量数字指数"，会把
            // `x^(2-1)-x=0` 这类恒等式误判成非多项式并交给牛顿法，输出一堆伪根。
            let exp = match fold_const_int(&exp) {
                Some(v) => Expr::Number(Number::from_bigint(v)),
                None => exp,
            };
            return Ok(Expr::Pow(Box::new(base), Box::new(exp)));
        }
        Ok(base)
    }

    /// 解析"坐标 [+ 解析式]"的一行拟合输入（调用方已用 `looks_like_polynomial_fit` 确认语法）。
    /// 顶点标记**只认大写 `P`**；写成小写 `p(x,y)` 时给出专门提示（小写 `p` 仍是普通变量）。
    pub fn parse_fit(&mut self) -> Result<crate::solver_fit::FitInput, String> {
        use crate::solver_fit::{FitInput, FitPoint};
        let mut points: Vec<FitPoint> = Vec::new();
        loop {
            self.skip_whitespace();
            match self.peek() {
                Some('P') => {
                    let save = self.pos;
                    self.next();
                    self.skip_whitespace();
                    if self.peek() == Some('(') {
                        let (x, y) = self.parse_coord_pair()?;
                        points.push(FitPoint {
                            is_vertex: true,
                            x,
                            y,
                        });
                        continue;
                    }
                    self.pos = save;
                    break;
                }
                Some('p') => {
                    let save = self.pos;
                    self.next();
                    self.skip_whitespace();
                    if self.peek() == Some('(') {
                        return Err("顶点标记必须是大写 P（小写 p 是普通变量）".to_string());
                    }
                    self.pos = save;
                    break;
                }
                Some('(') => {
                    let (x, y) = self.parse_coord_pair()?;
                    points.push(FitPoint {
                        is_vertex: false,
                        x,
                        y,
                    });
                }
                _ => break,
            }
        }
        if points.is_empty() {
            return Err("坐标格式错误（应为 (x,y) 或 P(x,y)）".to_string());
        }
        self.skip_whitespace();
        let tail: String = self.input[self.pos..].iter().collect();
        self.pos = self.input.len();
        let template = parse_fit_template(&tail)?;
        Ok(FitInput { points, template })
    }

    /// 解析一个坐标 `(x, y)`（当前位置必须停在 `(`）
    fn parse_coord_pair(&mut self) -> Result<(Expr, Expr), String> {
        let bad = || "坐标格式错误（应为 (x,y) 或 P(x,y)）".to_string();
        if self.next() != Some('(') {
            return Err(bad());
        }
        let x = self.parse_expression()?;
        self.skip_whitespace();
        if self.next() != Some(',') {
            return Err(bad());
        }
        let y = self.parse_expression()?;
        self.skip_whitespace();
        if self.next() != Some(')') {
            return Err(bad());
        }
        Ok((x, y))
    }

    fn parse_atom(&mut self) -> Result<Expr, String> {
        self.skip_whitespace();
        match self.peek() {
            Some('|') => {
                // 绝对值 |expr|，优先级与括号相同
                self.abs_depth += 1;
                self.next(); // 跳过 '|'
                let inner = self.parse_expression()?;
                self.skip_whitespace();
                if self.peek() != Some('|') {
                    self.abs_depth = self.abs_depth.saturating_sub(1);
                    return Err("缺少闭合的 '|'（绝对值）".to_string());
                }
                self.next();
                self.abs_depth -= 1;
                Ok(Expr::Function("abs".to_string(), vec![inner]))
            }
            Some('[') => {
                // 方括号列表：**脱糖**成 Function("list", [...])，不引入新的 AST 变体。
                // 加 Expr 变体会牵动 14 个文件的穷尽匹配；复用 Function 则零波及，
                // 而且同一套语法将来直接给矩阵用（[[1,2],[3,4]]）。
                self.next(); // 跳过 '['
                let mut items: Vec<Expr> = Vec::new();
                loop {
                    self.skip_whitespace();
                    if self.peek() == Some(']') {
                        self.next();
                        break;
                    }
                    if self.peek().is_none() {
                        return Err("缺少右方括号 ']'".to_string());
                    }
                    items.push(self.parse_expression()?);
                    self.skip_whitespace();
                    match self.peek() {
                        Some(',') => {
                            self.next();
                        }
                        Some(']') => {
                            self.next();
                            break;
                        }
                        _ => return Err("列表元素之间需要逗号，或以 ']' 结束".to_string()),
                    }
                }
                Ok(Expr::Function("list".to_string(), items))
            }
            Some('(') => {
                self.next(); // 跳过 '('
                let expr = self.parse_expression()?;
                self.skip_whitespace();
                if self.peek() != Some(')') {
                    return Err("缺少右括号 ')'".to_string());
                }
                self.next();
                Ok(expr)
            }
            Some(c) if c.is_ascii_digit() || c == '.' => {
                let num = self.parse_number()?;
                // 单位后缀：`3 km` / `3km` ⇒ 解析期立刻折成 SI（乘系数）。
                // 必须在这里做：再往后走，"多字母隐式乘法"会把 `km` 拆成 k·m。
                self.skip_whitespace();
                let save = self.pos;
                let start = self.pos;
                while self
                    .peek()
                    .is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
                {
                    self.next();
                }
                if self.pos > start {
                    let word: String = self.input[start..self.pos].iter().collect();
                    if let Some(factor) = PHYS_UNITS
                        .iter()
                        .find(|(u, _, _)| *u == word)
                        .map(|(_, f, _)| *f)
                    {
                        let mut fp = Parser::new(factor);
                        let f = fp.parse_number()?;
                        return Ok(Expr::Binary(Box::new(num), BinOp::Mul, Box::new(f)));
                    }
                }
                // 不是单位 ⇒ 回退，交给原有的隐式乘法（`3 x` 仍是 3·x，语义不变）
                self.pos = save;
                Ok(num)
            }
            Some(c) if c.is_alphabetic() || c == '_' => self.parse_identifier_or_function(),
            _ => Err(format!(
                "位置 {} 处意外的字符: '{}'",
                self.pos,
                self.peek().unwrap_or(' ')
            )),
        }
    }

    fn parse_number(&mut self) -> Result<Expr, String> {
        // 进制字面量：0x/0X 十六进制、0o/0O 八进制、0b/0B 二进制。
        // `0` 后面不跟进制字母时按普通十进制走（所以 `0`、`0.5`、`0e3` 都不受影响）。
        if self.peek() == Some('0') {
            let radix = match self.input.get(self.pos + 1) {
                Some('x') | Some('X') => Some(16u32),
                Some('o') | Some('O') => Some(8),
                Some('b') | Some('B') => Some(2),
                _ => None,
            };
            if let Some(radix) = radix {
                self.next(); // 跳过 '0'
                self.next(); // 跳过进制字母
                let digits_start = self.pos;
                while self.peek().is_some_and(|c| c.is_digit(radix)) {
                    self.next();
                }
                if self.pos == digits_start {
                    return Err(
                        "进制字面量缺少数字（0x / 0o / 0b 后要跟对应进制的数字）".to_string()
                    );
                }
                let text: String = self.input[digits_start..self.pos].iter().collect();
                return match BigInt::parse_bytes(text.as_bytes(), radix) {
                    Some(v) => Ok(Expr::Number(Number::from_bigint(v))),
                    None => Err(format!("不是合法的 {radix} 进制数: {text}")),
                };
            }
        }

        let start = self.pos;
        let mut has_dot = false;

        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                self.next();
            } else if ch == '.' && !has_dot {
                has_dot = true;
                self.next();
            } else {
                break;
            }
        }

        let num_str: String = self.input[start..self.pos].iter().collect();
        if num_str == "." {
            return Err("小数点后缺少数字".to_string());
        }

        // 科学计数法: <尾数>e[±]<整数指数>，如 1e3、2.5e-2、1.2E+4
        if self.peek().is_some_and(|c| c == 'e' || c == 'E') {
            let save_pos = self.pos;
            self.next(); // 跳过 e/E
            let exp_start = self.pos;
            if self.peek().is_some_and(|c| c == '+' || c == '-') {
                self.next();
            }
            if self.peek().is_some_and(|c| c.is_ascii_digit()) {
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    self.next();
                }
                let exp_str: String = self.input[exp_start..self.pos].iter().collect();
                if let Ok(exp) = exp_str.parse::<i64>() {
                    // 死算模式 (/mode deep) 取消指数上限（会真的去算 10^exp，慎用）
                    if exp.abs() > 100000 && !hipercalc_core::calc_mode::is_deep() {
                        return Err(
                            "科学计数法指数超出支持范围（/mode deep 可取消限制）".to_string()
                        );
                    }
                    let mantissa = Self::parse_decimal_rational(&num_str, has_dot)?;
                    let exp_u32 = u32::try_from(exp.abs())
                        .map_err(|_| "科学计数法指数超出可表示范围".to_string())?;
                    let factor = BigInt::from(10).pow(exp_u32);
                    let value = if exp >= 0 {
                        mantissa * BigRational::from_integer(factor)
                    } else {
                        mantissa / BigRational::from_integer(factor)
                    };
                    return Ok(Expr::Number(Number::from_rational(value)));
                }
            }
            // 无效的科学计数法（如 2e 后无数字），回退按隐式乘法处理
            self.pos = save_pos;
        }

        if num_str.ends_with('.') {
            let whole: String = num_str[..num_str.len() - 1].to_string();
            if let Ok(n) = whole.parse::<i64>() {
                return Ok(Expr::Number(Number::from_int(n)));
            }
        }

        // 解析为有理数
        if !has_dot {
            // 整数
            if let Ok(n) = num_str.parse::<i64>() {
                return Ok(Expr::Number(Number::from_int(n)));
            }
            // 大整数
            if let Some(bi) = BigInt::parse_bytes(num_str.as_bytes(), 10) {
                return Ok(Expr::Number(Number::from_bigint(bi)));
            }
            return Err(format!("无法解析数字: {}", num_str));
        }

        // 小数
        let parts: Vec<&str> = num_str.split('.').collect();
        let int_part = parts[0];
        let frac_part = if parts.len() > 1 { parts[1] } else { "" };

        if parts.len() > 1 {
            let frac_len = frac_part.len();
            let combined = format!("{}{}", int_part, frac_part);
            let combined = combined.trim_start_matches('0');
            if combined.is_empty() {
                return Ok(Expr::Number(Number::from_int(0)));
            }

            if let Some(numer) = BigInt::parse_bytes(combined.as_bytes(), 10) {
                let denom = BigInt::from(10).pow(frac_len as u32);
                // num_str 由 parse_number 从数字/小数点开始截取，不含符号（符号由一元解析处理）
                return Ok(Expr::Number(Number::from_rational(BigRational::new(
                    numer, denom,
                ))));
            }
        }

        Err(format!("无法解析数字: {}", num_str))
    }

    /// 将十进制数字串解析为有理数（串本身不含符号，has_dot 表示是否为小数）
    fn parse_decimal_rational(num_str: &str, has_dot: bool) -> Result<BigRational, String> {
        if has_dot {
            let parts: Vec<&str> = num_str.split('.').collect();
            let int_part = parts[0];
            let frac_part = if parts.len() > 1 { parts[1] } else { "" };
            let combined = format!("{}{}", int_part, frac_part);
            let combined = combined.trim_start_matches('0');
            if combined.is_empty() {
                return Ok(BigRational::zero());
            }
            if let Some(numer) = BigInt::parse_bytes(combined.as_bytes(), 10) {
                let denom = BigInt::from(10).pow(frac_part.len() as u32);
                return Ok(BigRational::new(numer, denom));
            }
        } else if let Some(bi) = BigInt::parse_bytes(num_str.as_bytes(), 10) {
            return Ok(BigRational::from_integer(bi));
        }
        Err(format!("无法解析数字: {}", num_str))
    }

    fn parse_identifier_or_function(&mut self) -> Result<Expr, String> {
        let start = self.pos;
        while let Some(ch) = self.peek() {
            if ch.is_alphanumeric() || ch == '_' {
                self.next();
            } else {
                break;
            }
        }
        let ident: String = self.input[start..self.pos].iter().collect();

        self.skip_whitespace();

        // 检查是否是函数调用
        if self.peek() == Some('(') {
            let func_name = ident.clone();
            self.next(); // 跳过 '('

            // 解析逗号分隔的参数列表（目前仅 log 用两参，其余函数单参）
            let mut args: Vec<Expr> = Vec::new();
            loop {
                let arg = self.parse_expression()?;
                args.push(arg);
                self.skip_whitespace();
                match self.peek() {
                    Some(',') => {
                        self.next();
                    }
                    Some(')') => {
                        self.next();
                        break;
                    }
                    _ => return Err(format!("函数 '{}' 缺少右括号", func_name)),
                }
            }

            // 验证函数名
            if FUNCTIONS.contains(&func_name.as_str()) {
                return Ok(Expr::Function(func_name, args));
            } else {
                return Err(format!("未知函数: {}", func_name));
            }
        }

        // 内置物理常量（查表 ⇒ 返回其十进制字面量解析出的数值）。
        // 放在 match **之前**：表里全是 2 字母以上的名字，与 pi/e/tau/phi/i 无冲突。
        if let Some((_, _, text)) = PHYS_CONSTANTS
            .iter()
            .find(|(s, l, _)| *s == ident || *l == ident)
        {
            let mut np = Parser::new(text);
            return np.parse_number();
        }

        // 常量或变量
        match ident.as_str() {
            "pi" | "π" => Ok(Expr::Number(Number::from_pi_times(BigRational::one()))),
            "e" => Ok(Expr::Number(Number::from_e_times(BigRational::one()))),
            // tau = 2π（精确）；phi/φ = 黄金比 (1+√5)/2（精确）
            "tau" => Ok(Expr::Number(Number::from_pi_times(
                BigRational::from_integer(BigInt::from(2u32)),
            ))),
            "phi" | "φ" => Ok(Expr::Number(Number::phi())),
            // 虚数单位 i（与 pi/e 同级；小写 i 已从 VALID_VARIABLES 移除，不再作为未知数）
            "i" => Ok(Expr::Number(Number::Complex(Box::new(
                hipercalc_core::complex::ComplexNum::i_unit(),
            )))),
            "ans" => Ok(Expr::Variable("ans".to_string())),
            // 无穷（极限点 / 积分限用）。它是**伪变量**：由 calculus 模块在自己的参数位置上识别，
            // 不允许参与普通求值（那样会报"未定义变量: inf"，误导用户）。
            "inf" | "∞" => Ok(Expr::Variable("inf".to_string())),
            _ => {
                // 单字母变量
                if ident.len() == 1 {
                    let ch = ident.chars().next().unwrap();
                    if VALID_VARIABLES.contains(ch) {
                        return Ok(Expr::Variable(ident));
                    }
                }
                // 全大写标识符（可含下划线）视为单个存储变量名（如 X、AB、PI_VAR），不做拆分。
                // 单大写 X/Y/Z 不在 VALID_VARIABLES 中（那边大写段只到 W），必须在此处接收。
                let all_upper = ident.chars().all(|c| c.is_ascii_uppercase() || c == '_');
                if all_upper {
                    return Ok(Expr::Variable(ident));
                }
                // 以 e 开头的连续变量串（如 2ex → 2*e*x、exy → e*x*y）：
                // 先取常数 e，其余字符交回隐式乘法逐层解析。
                // 旧实现直接报"未知标识符: ex"，导致 e 无法作为隐式乘法因子。
                if ident.len() > 1
                    && ident.starts_with('e')
                    && !FUNCTIONS.contains(&ident.as_str())
                    && ident.chars().skip(1).all(|c| VALID_VARIABLES.contains(c))
                {
                    self.pos = start + 1;
                    return Ok(Expr::Number(Number::from_e_times(BigRational::one())));
                }
                // 省略乘号的多字母变量序列（如 xy → x*y）。
                // 只消费第一个字符作为独立变量，其余字符回退给隐式乘法/幂逐层解析，
                // 这样才能保证 xy^2 = x*(y^2) 而非 (x*y)^2。
                if !FUNCTIONS.contains(&ident.as_str())
                    && ident.chars().all(|c| VALID_VARIABLES.contains(c))
                {
                    let first = ident.chars().next().unwrap().to_string();
                    self.pos = start + 1;
                    return Ok(Expr::Variable(first));
                }
                Err(format!("未知标识符: {}", ident))
            }
        }
    }

    fn try_parse_identifier(&mut self, s: &str) -> bool {
        let saved = self.pos;
        self.skip_whitespace();
        let start = self.pos;
        while let Some(ch) = self.peek() {
            if ch.is_alphanumeric() || ch == '_' {
                self.next();
            } else {
                break;
            }
        }
        let ident: String = self.input[start..self.pos].iter().collect();
        if ident == s {
            true
        } else {
            self.pos = saved;
            false
        }
    }
}

/// 表达式求值器
pub struct Evaluator {
    pub display_mode: DisplayMode,
    pub angle_mode: AngleMode,
    pub ans: Number,
    /// 用户存储变量（/let 指令，名称全大写）
    pub vars: std::collections::BTreeMap<String, Number>,
}

/// 黎曼 ζ 的**数值路径**（Euler-Maclaurin）。
///
/// 朴素 Dirichlet 级数 Σ 1/n^s 的误差是 O(N^{1-s})：s=2 时累 10⁶ 项也只有 6 位正确。
/// Euler-Maclaurin 把尾巴用「积分 + 伯努利数修正」一次补掉：
///
/// ```text
/// ζ(s) ≈ Σ_{k=1}^{N-1} k^{-s} + N^{1-s}/(s-1) + ½·N^{-s}
///        + Σ_{r=1}^{K} B_{2r}/(2r)! · s(s+1)…(s+2r-2) · N^{-s-2r+1}
/// ```
///
/// N=40、K=10（复用偶数点那张 B₂…B₂₀ 表）足以把误差压到 10^-40 量级。
/// 上升阶乘与 N 的指数都按 r 递推，不重复算幂。
fn zeta_numeric(s: &Number) -> Result<Number, String> {
    let pr = bigfloat::precision();
    let work = pr + 20; // 多算几位余量，最后统一截到显示精度
    let sv = s.to_approx();
    let one = BigFloat::from_u64(1);
    // s 必须 > 1：ζ 的级数只在 s>1 收敛，s≤1 需要解析延拓（本实现不做）
    let diff = BigFloat::sub(&sv, &one, work);
    if diff.value.is_negative() || diff.value.is_zero() {
        return Err("zeta 的实部必须大于 1（s=1 是发散的调和级数）".to_string());
    }
    const N: u64 = 40;
    const K: u32 = 10;
    let mut sum = BigFloat::from_u64(0);
    for k in 1..N {
        let term = BigFloat::from_u64(k)
            .pow(&sv.neg(), work)
            .map_err(|e| format!("zeta 数值计算失败: {e}"))?;
        sum = BigFloat::add(&sum, &term, work);
    }
    let nf = BigFloat::from_u64(N);
    let n_neg_s = nf
        .pow(&sv.neg(), work)
        .map_err(|e| format!("zeta 数值计算失败: {e}"))?;
    let n_inv = BigFloat::div(&one, &nf, work);
    // N^{1-s}/(s-1)
    let tail = BigFloat::mul(
        &nf.pow(&BigFloat::sub(&one, &sv, work), work)
            .map_err(|e| format!("zeta 数值计算失败: {e}"))?,
        &BigFloat::div(&one, &diff, work),
        work,
    );
    sum = BigFloat::add(&sum, &tail, work);
    // ½·N^{-s}
    let half = BigFloat::from_big_rational(&BigRational::new(BigInt::from(1), BigInt::from(2)));
    sum = BigFloat::add(&sum, &BigFloat::mul(&half, &n_neg_s, work), work);
    // 修正项（K=10 ⇒ 用到 B₂…B₂₀）
    let mut rising = sv.clone();
    let mut n_exp = BigFloat::mul(&n_neg_s, &n_inv, work); // r=1：N^{-s-1}
    let mut factorial = BigInt::from(2); // r=1：(2r)! = 2
    let n_inv2 = BigFloat::mul(&n_inv, &n_inv, work);
    for r in 1..=K {
        if r > 1 {
            // 把 s(s+1)…(s+2r-2) 补齐（每轮多两个因子）
            for j in [2 * r - 3, 2 * r - 2] {
                rising = BigFloat::mul(
                    &rising,
                    &BigFloat::add(&sv, &BigFloat::from_u64(j as u64), work),
                    work,
                );
            }
        }
        let b = bernoulli_even(r).expect("K ≤ 10，表内有");
        let coeff = BigFloat::div(
            &BigFloat::from_big_rational(&b),
            &BigFloat::from_int(&factorial),
            work,
        );
        let term = BigFloat::mul(&BigFloat::mul(&coeff, &rising, work), &n_exp, work);
        sum = BigFloat::add(&sum, &term, work);
        factorial *= BigInt::from(2 * r + 1) * BigInt::from(2 * r + 2);
        n_exp = BigFloat::mul(&n_exp, &n_inv2, work);
    }
    Ok(Number::Approx(sum))
}

/// 偶数点 ζ 闭式要用的伯努利数 B_2 … B_20（都是小有理数，直接查表比现算省事且不会错）
fn bernoulli_even(n: u32) -> Option<BigRational> {
    let (p, q) = match n {
        1 => (1, 6),
        2 => (-1, 30),
        3 => (1, 42),
        4 => (-1, 30),
        5 => (5, 66),
        6 => (-691, 2730),
        7 => (7, 6),
        8 => (-3617, 510),
        9 => (43867, 798),
        10 => (-174611, 330),
        _ => return None,
    };
    Some(BigRational::new(BigInt::from(p), BigInt::from(q)))
}

/// 整数序列在快速模式下的统一上限提示
const SEQUENCE_LIMIT: &str = "参数在快速模式下超出上限（/mode deep 可取消限制）";

/// 斐波那契：**快速倍增法**，O(log n)。用朴素递推的话 fib(10^6) 要迭代百万次，
/// 而倍增法只要约 20 轮大整数乘法（结果有 20 万位，代价主要在乘法本身）。
fn fibonacci(n: u64) -> BigInt {
    fn go(n: u64) -> (BigInt, BigInt) {
        if n == 0 {
            return (BigInt::from(0), BigInt::from(1));
        }
        let (a, b) = go(n / 2);
        let two_b = &b * 2;
        let c = &a * (&two_b - &a);
        let d = &a * &a + &b * &b;
        if n.is_multiple_of(2) {
            (c, d)
        } else {
            (d.clone(), &c + &d)
        }
    }
    go(n).0
}

/// 卡塔兰数 C_n = (2n)! / (n! (n+1)!)
///
/// 实现成 ∏_{k=2}^{n}(n+k) ÷ n! —— **分子分母各自累乘、最后只除一次**。
/// 曾经写成每步 "acc*(n+k)/k" 的逐步整除，结果 n=5 时第一步 1×7/2 就被截断成 3，
/// 算出 36（正确是 42）。逐步整除在这里不成立：单独看每一步都不保证整除。
fn catalan(n: u64) -> BigInt {
    let mut num = BigInt::from(1);
    let mut den = BigInt::from(1);
    for k in 2..=n {
        num *= BigInt::from(n + k);
        den *= BigInt::from(k);
    }
    num / den
}

/// 双阶乘 n!! = n(n-2)(n-4)…（偶数到 2、奇数到 1）；0!! = 1
fn double_factorial(n: u64) -> BigInt {
    let mut acc = BigInt::from(1);
    let mut k = n;
    while k > 1 {
        acc *= BigInt::from(k);
        k -= 2;
    }
    acc
}

/// 判断 n 是否 2 的整数次幂，返回指数（如 8 → Some(3)）
fn power_of_two(n: &BigInt) -> Option<u64> {
    if n <= &BigInt::from(0) {
        return None;
    }
    // 2^k = 1 后跟 k 个 0：位移出最高位的 1，再验证剩余部分为 0
    let bits = n.bits();
    if bits == 0 {
        return None;
    }
    let k = bits - 1;
    let pow = BigInt::from(1) << k;
    if n == &pow { Some(k) } else { None }
}

/// 判断 n 是否 10 的整数次幂，返回指数（仅处理位数不超大的情况，避免纯大数逐位除法卡顿）
fn power_of_ten(n: &BigInt) -> Option<u64> {
    // 判定方式是"反复除 10 直到不再整除"，每次除法代价 O(n 的 limb 数)，
    // 且指数越大循环越长（约 3.32 bit/位）；这里限到 2^10 bit（约 308 位十进制，
    // 即 u64 能表示的十进制指数）以内——超出就放弃精确识别，退回数值 ln 路径（结果一致）
    const MAX_BITS_FOR_TEN_POW: u64 = 1024;
    if n <= &BigInt::from(0) {
        return None;
    }
    if n.bits() > MAX_BITS_FOR_TEN_POW {
        return None;
    }
    let mut x = n.clone();
    let mut k = 0u64;
    while &x % BigInt::from(10) == BigInt::from(0) {
        x = &x / BigInt::from(10);
        k += 1;
    }
    if x == BigInt::from(1) { Some(k) } else { None }
}

/// 对正有理数做小质数分解：返回（质数 → 指数），分母贡献负指数。
/// 仅试除 10^6 以内的质数，剩余部分作为整体因子（指数 ±1）；
/// 超过约 100 位十进制的大数直接放弃（返回 None），避免试除卡顿。
fn factorize_rational(r: &BigRational) -> Option<std::collections::BTreeMap<BigInt, i64>> {
    let mut map = std::collections::BTreeMap::<BigInt, i64>::new();
    const LIMIT: i64 = 1_000_000;
    for (abs_n, sign) in [(r.numer().abs(), 1i64), (r.denom().abs(), -1i64)] {
        if abs_n <= BigInt::from(1) {
            continue;
        }
        if abs_n.bits() > 330 {
            return None;
        }
        let mut m = abs_n.clone();
        // 2、3、5
        for p in [2i64, 3, 5] {
            let pb = BigInt::from(p);
            while &m % &pb == BigInt::from(0) {
                *map.entry(BigInt::from(p)).or_insert(0) += sign;
                m /= &pb;
            }
        }
        // 6k±1 序列试除（7、11、13、17、…），到 10^6 或 p² > m
        let mut p = 7i64;
        let mut step = 4i64;
        loop {
            if p > LIMIT || BigInt::from(p) * BigInt::from(p) > m {
                break;
            }
            let pb = BigInt::from(p);
            if &m % &pb == BigInt::from(0) {
                while &m % &pb == BigInt::from(0) {
                    *map.entry(BigInt::from(p)).or_insert(0) += sign;
                    m /= &pb;
                }
            }
            p += step;
            step = 6 - step;
        }
        if m > BigInt::from(1) {
            *map.entry(m).or_insert(0) += sign;
        }
    }
    Some(map)
}

/// 若 base 与 x 同为某数 α 的整数次幂（base = α^m、x = α^k），
/// 返回精确值 log_base(x) = k/m（有理数）；否则返回 None 走数值路径。
fn rational_log_exact(base: &BigRational, x: &BigRational) -> Option<BigRational> {
    if x.is_zero() {
        return None;
    }
    // log_b(1) = 0（底数非 1 由调用方拦截）
    if x.is_one() {
        return Some(BigRational::zero());
    }
    let bf = factorize_rational(base)?;
    let xf = factorize_rational(x)?;
    if bf.is_empty() || xf.is_empty() {
        return None; // base 或 x 为 1（此处 base=1 已在调用方排除）
    }
    if bf.len() != xf.len() {
        return None; // 素因子集合不一致，不可能同底幂
    }
    let first = bf.keys().next().unwrap().clone();
    let a0 = *bf.get(&first)?;
    let c0 = *xf.get(&first)?;
    // 全部素因子的指数比必须一致：c_i / a_i = c0 / a0（交叉相乘精确比较）
    for (key, &a) in &bf {
        let c = *xf.get(key)?;
        if c * a0 != a * c0 {
            return None;
        }
    }
    Some(BigRational::new(BigInt::from(c0), BigInt::from(a0)))
}

impl Evaluator {
    pub fn new() -> Self {
        Evaluator {
            display_mode: DisplayMode::LineIO,
            angle_mode: AngleMode::Radian,
            ans: Number::from_int(0),
            vars: std::collections::BTreeMap::new(),
        }
    }

    pub fn evaluate(&mut self, expr: &Expr) -> Result<Number, String> {
        hipercalc_core::cancel::check()?;
        match expr {
            Expr::Sd(inner) => {
                let result = self.evaluate(inner)?;
                Ok(result)
            }
            Expr::Factor(_) => Err("factor 必须作为最外层函数使用".to_string()),
            Expr::Equation(_, _) => Err("等式需要在求解模式下处理，不应直接计算".to_string()),
            _ => Ok(self.eval_node(expr, &[])?),
        }
    }

    /// 带单变量替换的求值（兼容接口）
    pub fn evaluate_with_var(
        &self,
        expr: &Expr,
        var: &str,
        value: &Number,
    ) -> Result<Number, String> {
        // 中断检查点：长循环（数值求和/积分、DK 求根…）每次迭代至少走一次求值入口
        hipercalc_core::cancel::check()?;
        self.eval_node(expr, &[(var.to_string(), value)])
    }

    /// 带多变量替换的求值（非线性方程组牛顿法使用）
    pub fn evaluate_with_vars(
        &self,
        expr: &Expr,
        substs: &[(String, &Number)],
    ) -> Result<Number, String> {
        self.eval_node(expr, substs)
    }

    fn eval_node(&self, expr: &Expr, substs: &[(String, &Number)]) -> Result<Number, String> {
        match expr {
            Expr::Number(n) => Ok(n.clone()),
            Expr::Variable(name) => {
                // 优先级：显式替换（方程未知数/子式）> ans > 存储变量 > 未定义
                if let Some((_, val)) = substs.iter().find(|(v, _)| v == name) {
                    return Ok((*val).clone());
                }
                if name == "ans" {
                    return Ok(self.ans.clone());
                }
                if let Some(val) = self.vars.get(name) {
                    return Ok(val.clone());
                }
                Err(format!("未定义变量: {}", name))
            }
            Expr::Binary(left, op, right) => {
                let l = self.eval_node(left, substs)?;
                let r = self.eval_node(right, substs)?;
                // 矩阵预检：`Number::add/mul` 的签名没有 `Result`，非法组合必须在这里拦下，
                // 否则会走到 core 的不变式断言（那是 debug 断言，Release 下会给出错答案）。
                if l.is_matrix() || r.is_matrix() {
                    check_matrix_op(&l, op, &r)?;
                }
                match op {
                    BinOp::Add => Ok(l.add(&r)),
                    BinOp::Sub => Ok(l.sub(&r)),
                    BinOp::Mul => Ok(l.mul(&r)),
                    BinOp::Div => {
                        if r.is_zero() {
                            Err("除以零错误".to_string())
                        } else {
                            Ok(l.div(&r))
                        }
                    }
                }
            }
            Expr::Unary(op, expr) => {
                let val = self.eval_node(expr, substs)?;
                match op {
                    UnaryOp::Pos => Ok(val),
                    UnaryOp::Neg => Ok(val.neg()),
                }
            }
            Expr::Pow(base, exp) => {
                let b = self.eval_node(base, substs)?;
                let e = self.eval_node(exp, substs)?;
                Ok(b.pow(&e)?)
            }
            Expr::Function(name, args) => {
                if name == "factor" || name == "fac" {
                    return Err("fac/factor 必须作为最外层函数使用".to_string());
                }
                // 逐参求值后交给 eval_function（内部校验参数个数）
                let mut vals = Vec::with_capacity(args.len());
                for a in args {
                    vals.push(self.eval_node(a, substs)?);
                }
                self.eval_function(name, &vals)
            }
            Expr::Sd(_) => unreachable!(),
            Expr::Factor(_) => Err("factor 必须作为最外层函数使用".to_string()),
            Expr::Equation(_, _) => Err("等式不能在求值中使用".to_string()),
            Expr::System(_) => Err("方程组不能在求值中使用".to_string()),
        }
    }

    /// 复数参数的函数分派：
    /// - 支持的：`abs`（取模）、`sqr`/`sqrt`、`exp`、`ln`、`sin`/`cos`/`tan`、
    ///   以及新加的 `re`/`im`/`conj`/`arg`；
    /// - 加/减/乘/除与整数幂已由 `Number` 层直接处理（不经过这里）；
    /// - 其余函数（`floor`/`mod`/`gcd`/`nCr`/反三角…）在实数域才有定义，直接报错。
    fn eval_complex_function(&self, name: &str, args: &[Number]) -> Result<Number, String> {
        use hipercalc_core::complex::ComplexNum;
        let z = args[0].to_complex();
        match name {
            // |z| 是实数
            "abs" => Ok(z.abs()),
            "sqr" | "sqrt" => Ok(Number::from_complex(z.sqrt()?)),
            "exp" => Ok(Number::from_complex(z.exp()?)),
            "ln" => Ok(Number::from_complex(z.ln()?)),
            "sin" => Ok(Number::from_complex(z.sin()?)),
            "cos" => Ok(Number::from_complex(z.cos()?)),
            "tan" => Ok(Number::from_complex(z.tan()?)),
            // 实部 / 虚部 / 共轭 / 辐角
            "re" => Ok(z.re),
            "im" => Ok(z.im),
            "conj" => Ok(Number::from_complex(ComplexNum::new(z.re, z.im.neg()))),
            "arg" => z.arg(),
            _ => Err(format!("复数不支持该函数: {}", name)),
        }
    }

    fn eval_function(&self, name: &str, args: &[Number]) -> Result<Number, String> {
        // ── 矩阵：构造与线性代数 ───────────────────────────────────────────
        // `[a, b, c]`（元素全标量）⇒ **行向量**（1×n）。这与字面量的写法一致；
        // `[[..], [..]]`（元素是单行）⇒ 组装成矩阵。
        if name == "list" {
            if args.iter().all(|a| !a.is_matrix()) {
                return Number::from_matrix(vec![args.to_vec()]);
            }
            let mut rows: Vec<Vec<Number>> = Vec::new();
            for a in args {
                match a {
                    Number::Matrix(m) if m.len() == 1 => rows.push(m[0].clone()),
                    _ => return Err("矩阵字面量里每层只能是标量，或单行的 [..]".to_string()),
                }
            }
            return Number::from_matrix(rows);
        }
        // 特征值：core 给特征多项式系数，这里用**现成的多项式求根**解它
        if name == "eigen" {
            let m = args
                .first()
                .and_then(|a| a.as_matrix())
                .ok_or_else(|| "eigen 需要矩阵参数".to_string())?;
            let coeffs = hipercalc_core::matrix::char_poly_coeffs(m)?;
            let sols = crate::solver_poly::solve_poly_full(&coeffs)?;
            let mut vals: Vec<Number> = Vec::new();
            for s in sols {
                match s {
                    crate::solver_poly::PolySolution::Real(n) => vals.push(n),
                    crate::solver_poly::PolySolution::Complex(re, im) => vals.push(
                        Number::from_complex(hipercalc_core::complex::ComplexNum::new(re, im)),
                    ),
                }
            }
            // 特征值按行向量返回（复数特征值也能表示）
            return Number::from_matrix(vec![vals]);
        }
        // 解方程组 A x = b
        if name == "linsolve" {
            if args.len() != 2 {
                return Err("linsolve 用法: linsolve(A, b)".to_string());
            }
            let (Some(a), Some(b)) = (args[0].as_matrix(), args[1].as_matrix()) else {
                return Err("linsolve 的两个参数都要是矩阵（b 可写 [..] 行向量）".to_string());
            };
            return hipercalc_core::matrix::solve(a, b);
        }
        if let Some(m) = args.first().and_then(|a| a.as_matrix()) {
            use hipercalc_core::matrix as mx;
            return match name {
                "det" => mx::det(m),
                "inv" => mx::inv(m),
                "trace" => mx::trace(m),
                "transpose" => Ok(Number::Matrix(Box::new(mx::transpose(m)))),
                "rank" => Ok(Number::from_int(mx::rank(m) as i64)),
                "eigen" | "linsolve" => unreachable!("上面已单独处理"),
                _ => Err(format!("函数 {name} 不支持矩阵参数")),
            };
        }
        // 高等数学函数是**解析期**由 `calculus::expand_calculus` 就地展开的，正常流程到不了这里。
        // 这条拦截是兜底：万一将来新增了绕过重写的解析路径，也要给出明确提示而不是"未知函数"。
        if crate::calculus::is_calculus_name(name) {
            return Err(crate::calculus::ERROR_CALC_IN_WRAPPER.replace("{0}", name));
        }
        // 参数个数校验：log 有专门的提示（写出参数含义），其余两参函数给统一提示
        if name == "log" && args.len() != 2 {
            return Err("log 需要两个参数: log(底数, 真数)".to_string());
        }
        if name != "log" && name != "list" {
            let want = if TWO_ARG_FUNCTIONS.contains(&name) {
                2
            } else {
                1
            };
            if args.len() != want {
                return Err(if want == 2 {
                    format!("函数 {} 需要两个参数", name)
                } else {
                    format!("函数 {} 需要一个参数", name)
                });
            }
        }
        let arg = args
            .first()
            .ok_or_else(|| format!("函数 {} 缺少参数", name))?;
        // 复数分流：只有一部分函数支持复数，其余给明确报错（不再落到"取实部"的数值路径上）。
        // 四个**分量操作**例外——实部/虚部/共轭/辐角对实数同样有定义（实数 = 虚部为 0 的复数），
        // 所以无论实参是不是复数都走复数路径：否则 `re(2)`、`arg(-1)` 这类合法输入会报"未知函数"。
        if args.iter().any(|a| a.is_complex()) || COMPONENT_FUNCTIONS.contains(&name) {
            return self.eval_complex_function(name, args);
        }
        match name {
            "cbrt" => {
                // 立方根：允许负数（奇次），完全立方给精确值
                let neg = arg.is_negative();
                let a = if neg { arg.neg() } else { arg.clone() };
                let out = a.cbrt()?;
                Ok(if neg { out.neg() } else { out })
            }
            "nroot" => {
                // nroot(x, n)：n 次根，n 必须是 ≥ 2 的整数；负数只在 n 为奇数时有定义
                let x = &args[0];
                let k = as_nonneg_int(&args[1], "nroot 的次数必须是 >= 2 的整数")?;
                let kv = match k.to_u32() {
                    Some(v) if (2..=1_000_000).contains(&v) => v,
                    _ => return Err("nroot 的次数超出支持范围（2 ~ 1000000）".to_string()),
                };
                let neg = x.is_negative();
                if neg && kv % 2 == 0 {
                    return Err("负数的偶次根在实数范围内无定义".to_string());
                }
                let a = if neg { x.neg() } else { x.clone() };
                let out = a.nth_root(kv)?;
                Ok(if neg { out.neg() } else { out })
            }
            // 位运算：只对**非负整数**定义。负数的"位表示"依赖位宽（补码位数一说就变），
            // 是另一套语义，这里明确拒绝而不是猜一个。
            "and" | "or" | "xor" => {
                let err = "位运算只支持非负整数";
                let a = as_nonneg_int(&args[0], err)?;
                let b = as_nonneg_int(&args[1], err)?;
                let out = match name {
                    "and" => a & b,
                    "or" => a | b,
                    _ => a ^ b,
                };
                Ok(Number::from_bigint(out))
            }
            // 与 Python 一致：无限宽补码下 ~a = -a - 1（所以结果是负数，输入仍须非负）
            "not" => {
                let a = as_nonneg_int(&args[0], "位运算只支持非负整数")?;
                Ok(Number::from_bigint(-(a + BigInt::from(1))))
            }
            "shl" | "shr" => {
                let err = "移位只能作用于非负整数";
                let a = as_nonneg_int(&args[0], err)?;
                let n = as_nonneg_int(&args[1], err)?;
                const MAX_SHIFT: u32 = 1_000_000;
                let nv = match n.to_u32() {
                    Some(v) if v <= MAX_SHIFT => v,
                    _ => return Err(format!("移位位数必须在 0 到 {MAX_SHIFT} 之间")),
                };
                let out = if name == "shl" { a << nv } else { a >> nv };
                Ok(Number::from_bigint(out))
            }
            // 整数序列：快速模式给上限（否则会算出几十万位的数），/mode deep 可取消
            "fib" => {
                let n = as_nonneg_int(&args[0], "fib 需要非负整数参数")?;
                let nv = match n.to_u32() {
                    Some(v) if v <= 1_000_000 || hipercalc_core::calc_mode::is_deep() => v as u64,
                    _ => return Err(SEQUENCE_LIMIT.to_string()),
                };
                Ok(Number::from_bigint(fibonacci(nv)))
            }
            "catalan" => {
                let n = as_nonneg_int(&args[0], "catalan 需要非负整数参数")?;
                let nv = match n.to_u32() {
                    Some(v) if v <= 20_000 || hipercalc_core::calc_mode::is_deep() => v as u64,
                    _ => return Err(SEQUENCE_LIMIT.to_string()),
                };
                Ok(Number::from_bigint(catalan(nv)))
            }
            "doublefac" => {
                let n = as_nonneg_int(&args[0], "doublefac 需要非负整数参数")?;
                let nv = match n.to_u32() {
                    Some(v) if v <= 20_000 || hipercalc_core::calc_mode::is_deep() => v as u64,
                    _ => return Err(SEQUENCE_LIMIT.to_string()),
                };
                Ok(Number::from_bigint(double_factorial(nv)))
            }
            // 伽马函数：只给**精确**结果，不做数值近似（避免"看起来精确"的假象）
            //   Γ(n)     = (n-1)!          —— 正整数
            //   Γ(n+1/2) = (2n)!/(4^n·n!)·√π —— 正半整数，靠 ExactTerm::PiPow（指数 1/2）精确表示
            "gamma" => {
                let two = BigInt::from(2);
                let err = "gamma 目前只支持正整数与正半整数（其余值需要数值近似，暂未开放）";
                let r = args[0].as_rational().ok_or_else(|| err.to_string())?;
                let (num, den) = (r.numer().clone(), r.denom().clone());
                if den == BigInt::one() {
                    // 正整数：Γ(n) = (n-1)!
                    if num <= BigInt::from(0) {
                        return Err("gamma 在 0 与负整数处是极点".to_string());
                    }
                    if num > BigInt::from(100_000) {
                        return Err(SEQUENCE_LIMIT.to_string());
                    }
                    let mut acc = BigInt::from(1);
                    let mut i = BigInt::from(2);
                    while i < num {
                        acc *= &i;
                        i += BigInt::from(1);
                    }
                    return Ok(Number::from_bigint(acc));
                }
                if den == two {
                    // 正半整数 x = m/2（m 为奇数）：Γ((2n+1)/2) = (2n)!/(4^n·n!)·√π，其中 n=(m-1)/2
                    if num <= BigInt::from(0) {
                        return Err(err.to_string());
                    }
                    let n = (&num - BigInt::from(1)) / BigInt::from(2);
                    let n_u32 = match n.to_u32() {
                        Some(v) if v <= 20_000 => v,
                        _ => return Err(SEQUENCE_LIMIT.to_string()),
                    };
                    let mut num_fac = BigInt::from(1); // (2n)!
                    let mut i = BigInt::from(2);
                    let twon = &n * BigInt::from(2);
                    while i <= twon {
                        num_fac *= &i;
                        i += BigInt::from(1);
                    }
                    let mut n_fac = BigInt::from(1); // n!
                    let mut i = BigInt::from(2);
                    while i <= n {
                        n_fac *= &i;
                        i += BigInt::from(1);
                    }
                    let coeff = BigRational::new(num_fac, BigInt::from(4).pow(n_u32) * n_fac);
                    // Γ(n+1/2) = coeff · π^(1/2)
                    let half = BigRational::new(BigInt::from(1), BigInt::from(2));
                    return Ok(Number::Exact(ExactExpr {
                        terms: vec![ExactTerm::PiPow(coeff, half)],
                        denominator: BigInt::one(),
                    }));
                }
                Err(err.to_string())
            }
            // 误差函数 erf(x) = (2/√π)·∫₀ˣ e^(-t²) dt，以及补误差函数 erfc(x) = 1 - erf(x)。
            // **没有初等闭式** ⇒ 走泰勒级数：
            //   erf(x) = 2/√π · Σ_{n≥0} (-1)^n x^(2n+1) / (n!·(2n+1))
            // 项递推：t_{n+1} = -t_n · x² · (2n+1) / ((n+1)·(2n+3))（不必反复算幂与阶乘）
            // 判据用**相对**判据（与 exp 一致）：绝对判据在大 |x| 下永不收敛。
            "erf" | "erfc" => {
                let p = bigfloat::precision();
                let xf = args[0].to_approx();
                // |x| ≥ 8 时 erf 与 ±1 的差已到 1e-29（20 位显示下就是 ±1），
                // 直接饱和而不是硬算：级数在 x=8 附近要损失约 28 位有效数字。
                // 只在能拿到精确有理数时做这个判断 —— 拿不到（如 π）就照常算级数，那些值本来也不大。
                // 注意：这里返回的是"饱和成 +1 还是 -1"，None 表示**不饱和**。
                // 早先写成 `if let Some(neg) = saturate` 是错的 —— 那个 Option 对所有有理数都是 Some，
                // 于是 erf(1) 这类正常输入也被当成饱和值直接返回 ±1。
                let saturate_sign = args[0].as_rational().and_then(|r| {
                    let eight = BigRational::from_integer(BigInt::from(8));
                    let neg_eight = -eight.clone();
                    if r >= eight {
                        Some(false)
                    } else if r <= neg_eight {
                        Some(true)
                    } else {
                        None
                    }
                });
                if let Some(neg) = saturate_sign {
                    let one = BigFloat::from_u64(1);
                    let mut out = one;
                    if neg {
                        out = out.neg();
                    }
                    if name == "erfc" {
                        out = BigFloat::sub(&BigFloat::from_u64(1), &out, p);
                    }
                    return Ok(Number::Approx(out));
                }

                // BigFloat 是 value / 10^precision 的定点表示 —— 符号就在公开的 value 里
                let neg = xf.value.is_negative();
                let x = if neg { xf.neg() } else { xf };
                let two = BigFloat::from_u64(2);
                let x2 = BigFloat::mul(&x, &x, p);
                let mut term = x.clone(); // n = 0 项：x
                let mut sum = term.clone();
                let limit_rel = BigFloat::from_big_rational(&BigRational::new(
                    BigInt::from(1),
                    BigInt::from(10).pow(p.saturating_sub(2) as u32),
                ));
                for n in 0..400u32 {
                    // t_{n+1} = -t_n · x² · (2n+1) / ((n+1)(2n+3))
                    let num = BigFloat::from_u64((2 * n + 1) as u64);
                    let den = BigFloat::from_u64(((n + 1) as u64) * ((2 * n + 3) as u64));
                    term = BigFloat::mul(&term, &x2, p);
                    term = BigFloat::mul(&term, &num, p);
                    term = BigFloat::div(&term, &den, p);
                    term = term.neg();
                    sum = BigFloat::add(&sum, &term, p);
                    // |t_{n+1}| ≤ |sum|·10^-(p-2) 即认为收敛
                    let scaled = BigFloat::mul(&sum, &limit_rel, p);
                    let abs_term = if term.value.is_negative() {
                        term.neg()
                    } else {
                        term.clone()
                    };
                    // 同样看定点尾数的符号：diff = |t| - |sum|·10^-(p-2)，为负即已收敛
                    let diff = BigFloat::sub(&abs_term, &scaled, p);
                    if diff.value.is_negative() {
                        break;
                    }
                }
                // erf = 2/√π · sum
                let two_over_sqrt_pi = BigFloat::div(&two, &BigFloat::pi(p).sqrt(p), p);
                let mut out = BigFloat::mul(&sum, &two_over_sqrt_pi, p);
                if neg {
                    out = out.neg();
                }
                if name == "erfc" {
                    out = BigFloat::sub(&BigFloat::from_u64(1), &out, p);
                }
                Ok(Number::Approx(out))
            }
            // 黎曼 ζ 函数。**偶数点有精确闭式**：
            //   ζ(2n) = (-1)^(n+1) · B_{2n} · (2π)^{2n} / (2·(2n)!)   ⇒  系数 · π^(2n)
            // 需要精确类型能表示 π 的幂，所以上一轮先把 SqrtPi 泛化成了 PiPow。
            // 奇数点没有已知闭式（ζ(3) 至今只有数值），留给数值路径。
            "zeta" => {
                // 拿不到精确有理数（如 π）就直接走数值路径
                let s_arg = match args[0].as_rational() {
                    Some(r) => r,
                    None => return zeta_numeric(&args[0]),
                };
                if !s_arg.is_integer() {
                    // 非整数 s 没有闭式 ⇒ 数值路径
                    return zeta_numeric(&args[0]);
                }
                let sv = s_arg.to_integer();
                if sv <= BigInt::from(1) {
                    return Err("zeta 的实部必须大于 1（s=1 是发散的调和级数）".to_string());
                }
                if sv.clone() % BigInt::from(2) != BigInt::zero() {
                    // 奇数点没有已知闭式（ζ(3) 等）⇒ 数值路径
                    return zeta_numeric(&args[0]);
                }
                let n = &sv / BigInt::from(2);
                let n_u32 = n
                    .to_u32()
                    .filter(|v| *v <= 10)
                    .ok_or_else(|| "zeta 目前只支持到 ζ(20)（伯努利数表到此为止）".to_string())?;
                let b = bernoulli_even(n_u32).expect("已按范围校验");
                // 系数 = (-1)^(n+1) · B_{2n} · 2^(2n) / (2·(2n)!)
                // (-1)^(n+1)：n 为奇数时是 +1（ζ(2)、ζ(6)…），n 为偶数时是 -1
                // —— 之前写成 (n+1)%2==1 恰好反了，ζ(2) 会输出成 -pi^2/6
                let sign = if n_u32 % 2 == 1 { 1 } else { -1 };
                let two_pow = BigInt::from(2).pow(2 * n_u32);
                let mut factorial = BigInt::from(1);
                let mut i = BigInt::from(2);
                let two_n = BigInt::from(2 * n_u32);
                while i <= two_n {
                    factorial *= &i;
                    i += BigInt::one();
                }
                let coeff = BigRational::new(
                    BigInt::from(sign) * b.numer() * &two_pow,
                    b.denom() * BigInt::from(2) * factorial,
                );
                let exp = BigRational::from_integer(BigInt::from(2 * n_u32));
                Ok(Number::Exact(ExactExpr {
                    terms: vec![ExactTerm::PiPow(coeff, exp)],
                    denominator: BigInt::one(),
                }))
            }
            "mod" => args[0].modulo(&args[1]),
            "idiv" => args[0].idiv(&args[1]),
            "nCr" | "nPr" => {
                let is_c = name == "nCr";
                let err = if is_c {
                    "组合数需要非负整数参数"
                } else {
                    "排列数需要非负整数参数"
                };
                let n = as_nonneg_int(&args[0], err)?;
                let r = as_nonneg_int(&args[1], err)?;
                if !hipercalc_core::calc_mode::is_deep() && n > BigInt::from(10_000u32) {
                    return Err(if is_c {
                        "组合数参数过大（上限 10000，/mode deep 可取消限制）".to_string()
                    } else {
                        "排列数参数过大（上限 10000，/mode deep 可取消限制）".to_string()
                    });
                }
                // r > n 时组合数与排列数都取 0
                if r > n {
                    return Ok(Number::from_int(0));
                }
                if is_c {
                    // C(n,r) = C(n,n-r)：取较小的那个，乘除次数最少
                    let r_neg = &n - &r;
                    let r_use = if r_neg < r { r_neg } else { r };
                    let steps = r_use.to_u32().ok_or_else(|| "组合数参数过大".to_string())?;
                    let mut acc = BigInt::one();
                    for i in 1..=steps {
                        // 每步都能整除（部分积恰为 C(n-r+i, i)），无需整体约分
                        acc = acc * (&n - &r_use + i) / i;
                    }
                    Ok(Number::from_bigint(acc))
                } else {
                    // P(n,r) = n·(n-1)·…·(n-r+1)
                    let steps = r.to_u32().ok_or_else(|| "排列数参数过大".to_string())?;
                    let mut acc = BigInt::one();
                    for i in 0..steps {
                        acc *= &n - i;
                    }
                    Ok(Number::from_bigint(acc))
                }
            }
            "gcd" | "lcm" => {
                use num_integer::Integer;
                let a = as_int(&args[0], "gcd/lcm 需要整数参数")?;
                let b = as_int(&args[1], "gcd/lcm 需要整数参数")?;
                // 参数须为**非零**整数：0 会让 gcd 退化成"另一个数"、lcm 恒为 0，按约定直接拒绝。
                // 结果统一取绝对值 ⇒ 无论两个参数的正负，返回值**恒为非负**。
                if a.is_zero() || b.is_zero() {
                    return Err("gcd/lcm 的参数不能为 0".to_string());
                }
                Ok(Number::from_bigint(if name == "gcd" {
                    a.gcd(&b).abs()
                } else {
                    a.lcm(&b).abs()
                }))
            }
            "isprime" => {
                let n = as_int(arg, "isprime 需要整数参数")?;
                if !hipercalc_core::calc_mode::is_deep() && n > BigInt::from(10u32).pow(24) {
                    return Err("素性判定参数过大（上限 10^24，/mode deep 可取消限制）".to_string());
                }
                Ok(Number::from_int(
                    if hipercalc_core::bigint_ext::is_prime(&n) {
                        1
                    } else {
                        0
                    },
                ))
            }
            "nextprime" => {
                let n = as_int(arg, "nextprime 需要整数参数")?;
                if !hipercalc_core::calc_mode::is_deep() && n > BigInt::from(10u32).pow(24) {
                    return Err("素性判定参数过大（上限 10^24，/mode deep 可取消限制）".to_string());
                }
                Ok(Number::from_bigint(hipercalc_core::bigint_ext::next_prime(
                    &n,
                )))
            }
            "sqr" | "sqrt" => {
                if arg.is_negative() {
                    // 负实数：实数域无定义，但复数域是纯虚数（sqrt(-4) = 2i、sqrt(-2) = sqrt(2)i）
                    let pos = arg.neg().sqrt();
                    return Ok(Number::from_complex(
                        hipercalc_core::complex::ComplexNum::new(Number::from_int(0), pos),
                    ));
                }
                Ok(arg.sqrt())
            }
            "abs" => Ok(arg.abs()),
            "floor" => Ok(arg.floor()),
            "ceil" => Ok(arg.ceil()),
            "round" => Ok(arg.round()),
            "frac" => Ok(arg.frac()),
            "sign" => Ok(arg.sign()),
            "log" => {
                let base = &args[0];
                let x = &args[1];
                if base.is_negative() || base.is_zero() {
                    return Err("log 的底数必须 > 0".to_string());
                }
                // 底数为 1（或与 1 的偏差小于 1e-40）时 log_1(x) 无定义。
                // 旧实现用 `rounded(0) == 1` 判断，会把 [0.5, 1.5) 的底数（如 0.5、0.9、1.2）
                // 全部误判为 1 ⇒ log(0.5,8) 这类合法输入被错误拒绝。
                let base_is_one = match base.as_rational() {
                    Some(r) => r.is_one(),
                    None => {
                        let b = base.to_approx();
                        let one = bigfloat::BigFloat::from_u64(1);
                        bigfloat::BigFloat::sub(&b, &one, bigfloat::precision())
                            .value
                            .abs()
                            <= BigInt::from(10).pow((bigfloat::precision() / 2) as u32)
                    }
                };
                if base_is_one {
                    return Err("log 的底数不能为 1".to_string());
                }
                if x.is_negative() || x.is_zero() {
                    return Err("log 的真数必须 > 0".to_string());
                }
                // log_b(b) = 1：底数与真数同一数（含无理数 e、π 等）时精确返回
                let bx = base.to_approx();
                let xx = x.to_approx();
                if bx.value == xx.value && bx.precision == xx.precision {
                    return Ok(Number::from_int(1));
                }
                // 精确识别：底数与真数同为某数 α 的整数（可负）次幂时，log_b(x) = k/m
                if let (Some(b), Some(v)) = (base.as_rational(), x.as_rational())
                    && let Some(t) = rational_log_exact(&b, &v)
                {
                    return Ok(Number::from_rational(t));
                }
                // 一般情形：log_b(x) = ln(x) / ln(b)
                let ln_x = xx.ln(bigfloat::precision());
                let ln_b = bx.ln(bigfloat::precision());
                Ok(Number::Approx(bigfloat::BigFloat::div(
                    &ln_x,
                    &ln_b,
                    bigfloat::precision(),
                )))
            }
            "ln" => {
                if arg.is_zero() {
                    return Err("ln 的定义域为 x>0".to_string());
                }
                if arg.is_negative() {
                    // 负实数：实数域无定义，复数域给 ln(-1) = iπ（ln(-x) = ln x + iπ）
                    let z =
                        hipercalc_core::complex::ComplexNum::new(arg.clone(), Number::from_int(0));
                    return Ok(Number::from_complex(z.ln()?));
                }
                Ok(Number::Approx(arg.to_approx().ln(bigfloat::precision())))
            }
            // exp 带参数上限保护（超限时 BigFloat::exp 会返回错误，避免卡死/结果失控）
            "exp" => {
                check_exp_range(arg)?;
                Ok(Number::Approx(arg.to_approx().exp(bigfloat::precision())?))
            }
            "log10" => {
                if arg.is_negative() || arg.is_zero() {
                    return Err("log10 的定义域为 x>0".to_string());
                }
                // 若 x = p/q 且 p、q 均为 10 的幂（即 x = 10^k），给出精确整数结果
                if let Some(r) = arg.as_rational() {
                    let (p, q) = (r.numer(), r.denom());
                    if let (Some(a), Some(b)) = (power_of_ten(p), power_of_ten(q)) {
                        return Ok(Number::from_bigint(
                            BigInt::from(a as i64) - BigInt::from(b as i64),
                        ));
                    }
                }
                let ln_x = arg.to_approx().ln(bigfloat::precision());
                let ln_10 = bigfloat::BigFloat::ln(
                    &bigfloat::BigFloat::from_i64(10),
                    bigfloat::precision(),
                );
                Ok(Number::Approx(bigfloat::BigFloat::div(
                    &ln_x,
                    &ln_10,
                    bigfloat::precision(),
                )))
            }
            "log2" => {
                if arg.is_negative() || arg.is_zero() {
                    return Err("log2 的定义域为 x>0".to_string());
                }
                // 若 x = p/q 且 p、q 均为 2 的幂（即 x = 2^k），给出精确整数结果
                if let Some(r) = arg.as_rational() {
                    let (p, q) = (r.numer(), r.denom());
                    if let (Some(a), Some(b)) = (power_of_two(p), power_of_two(q)) {
                        return Ok(Number::from_bigint(
                            BigInt::from(a as i64) - BigInt::from(b as i64),
                        ));
                    }
                }
                let ln_x = arg.to_approx().ln(bigfloat::precision());
                let ln_2 =
                    bigfloat::BigFloat::ln(&bigfloat::BigFloat::from_i64(2), bigfloat::precision());
                Ok(Number::Approx(bigfloat::BigFloat::div(
                    &ln_x,
                    &ln_2,
                    bigfloat::precision(),
                )))
            }
            "fact" => {
                // 由后缀 !（如 5!、(2+3)!）生成的内部函数，白名单不暴露 fact(...)
                let r = arg
                    .as_rational()
                    .ok_or_else(|| "阶乘需要整数参数".to_string())?;
                if !r.is_integer() {
                    return Err("阶乘需要整数参数".to_string());
                }
                let n = r.to_integer();
                if n.is_negative() {
                    return Err("阶乘需要非负整数".to_string());
                }
                if !hipercalc_core::calc_mode::is_deep() && n > BigInt::from(10000) {
                    return Err("阶乘参数过大（上限 10000，/mode deep 可取消限制）".to_string());
                }
                let mut acc = num_bigint::BigInt::one();
                let nu = n
                    .to_u32()
                    .ok_or_else(|| "阶乘参数超出可表示范围".to_string())?;
                for i in 2..=nu {
                    acc *= num_bigint::BigInt::from(i);
                }
                Ok(Number::from_bigint(acc))
            }
            "sinh" => {
                check_exp_range(arg)?;
                let b = arg.to_approx();
                let ep = b.exp(bigfloat::precision())?;
                let en = bigfloat::BigFloat::neg(&b).exp(bigfloat::precision())?;
                let d = bigfloat::BigFloat::sub(&ep, &en, bigfloat::precision());
                Ok(Number::Approx(bigfloat::BigFloat::div(
                    &d,
                    &bigfloat::BigFloat::from_i64(2),
                    bigfloat::precision(),
                )))
            }
            "cosh" => {
                check_exp_range(arg)?;
                let b = arg.to_approx();
                let ep = b.exp(bigfloat::precision())?;
                let en = bigfloat::BigFloat::neg(&b).exp(bigfloat::precision())?;
                let d = bigfloat::BigFloat::add(&ep, &en, bigfloat::precision());
                Ok(Number::Approx(bigfloat::BigFloat::div(
                    &d,
                    &bigfloat::BigFloat::from_i64(2),
                    bigfloat::precision(),
                )))
            }
            "coth" => {
                // coth(x) = (e^{2x}+1)/(e^{2x}-1)，x=0 处未定义
                if arg.is_zero() {
                    return Err("coth 在 x=0 处未定义".to_string());
                }
                check_exp_range(arg)?;
                let prec = bigfloat::precision();
                let two = bigfloat::BigFloat::from_u64(2);
                let t = bigfloat::BigFloat::mul(&two, &arg.to_approx(), prec).exp(prec)?;
                let one = bigfloat::BigFloat::from_u64(1);
                let num = bigfloat::BigFloat::add(&t, &one, prec);
                let den = bigfloat::BigFloat::sub(&t, &one, prec);
                if den.is_zero() {
                    return Err("coth 在 x=0 处未定义".to_string());
                }
                Ok(Number::Approx(bigfloat::BigFloat::div(&num, &den, prec)))
            }
            "sech" => {
                // sech(x) = 2/(e^x + e^{-x})
                check_exp_range(arg)?;
                let prec = bigfloat::precision();
                let b = arg.to_approx();
                let ep = b.exp(prec)?;
                let en = bigfloat::BigFloat::neg(&b).exp(prec)?;
                let den = bigfloat::BigFloat::add(&ep, &en, prec);
                let two = bigfloat::BigFloat::from_u64(2);
                Ok(Number::Approx(bigfloat::BigFloat::div(&two, &den, prec)))
            }
            "csch" => {
                // csch(x) = 2/(e^x - e^{-x})，x=0 处未定义
                if arg.is_zero() {
                    return Err("csch 在 x=0 处未定义".to_string());
                }
                check_exp_range(arg)?;
                let prec = bigfloat::precision();
                let b = arg.to_approx();
                let ep = b.exp(prec)?;
                let en = bigfloat::BigFloat::neg(&b).exp(prec)?;
                let den = bigfloat::BigFloat::sub(&ep, &en, prec);
                let two = bigfloat::BigFloat::from_u64(2);
                Ok(Number::Approx(bigfloat::BigFloat::div(&two, &den, prec)))
            }
            "arcsinh" => {
                // arcsinh(x) = sign(x)·ln(|x| + sqrt(x²+1))：用 |x| 入 ln，负数走取反
                if arg.is_zero() {
                    return Ok(Number::from_int(0));
                }
                let prec = bigfloat::precision();
                let b = arg.to_approx();
                let neg = b.value.is_negative();
                let abs_b = bigfloat::BigFloat {
                    value: b.value.abs(),
                    precision: b.precision,
                };
                let sq = bigfloat::BigFloat::mul(&abs_b, &abs_b, prec);
                let inner = bigfloat::BigFloat::add(&sq, &bigfloat::BigFloat::from_u64(1), prec);
                let sum = bigfloat::BigFloat::add(&abs_b, &inner.sqrt(prec), prec);
                let v = sum.ln(prec);
                Ok(Number::Approx(if neg {
                    bigfloat::BigFloat::neg(&v)
                } else {
                    v
                }))
            }
            "arccosh" => {
                // arccosh(x) = ln(x + sqrt(x²-1))，定义域 x ≥ 1
                // x = 1 时结果为精确的 0（与 arcsinh(0) 一致；否则会返回 Approx(0)、被标成 `≈ 0`）
                if arg.sub(&Number::from_int(1)).is_zero() {
                    return Ok(Number::from_int(0));
                }
                let b = arg.to_approx();
                let one_scaled = BigInt::from(10).pow(b.precision as u32);
                if b.value < one_scaled {
                    return Err("arccosh 的定义域为 x >= 1".to_string());
                }
                let prec = bigfloat::precision();
                let sq = bigfloat::BigFloat::mul(&b, &b, prec);
                let inner = bigfloat::BigFloat::sub(&sq, &bigfloat::BigFloat::from_u64(1), prec);
                let sum = bigfloat::BigFloat::add(&b, &inner.sqrt(prec), prec);
                Ok(Number::Approx(sum.ln(prec)))
            }
            "arctanh" => {
                // arctanh(x) = ½·ln((1+x)/(1-x))，定义域 |x| < 1
                // x = 0 时结果为精确的 0（同上：否则会被标成 `≈ 0`）
                if arg.is_zero() {
                    return Ok(Number::from_int(0));
                }
                let b = arg.to_approx();
                let one_scaled = BigInt::from(10).pow(b.precision as u32);
                if b.value.abs() >= one_scaled {
                    return Err("arctanh 的定义域为 |x| < 1".to_string());
                }
                let prec = bigfloat::precision();
                let one = bigfloat::BigFloat::from_u64(1);
                let num = bigfloat::BigFloat::add(&one, &b, prec);
                let den = bigfloat::BigFloat::sub(&one, &b, prec);
                // 除法多留 10 位，避免 |x|→1 时的相消误差直接吃掉目标精度
                let q = bigfloat::BigFloat::div(&num, &den, prec + 10);
                let half = bigfloat::BigFloat::div(
                    &bigfloat::BigFloat::from_u64(1),
                    &bigfloat::BigFloat::from_u64(2),
                    prec,
                );
                Ok(Number::Approx(bigfloat::BigFloat::mul(
                    &half,
                    &q.ln(prec),
                    prec,
                )))
            }
            "tanh" => {
                // |x| > 100 时双曲正切在 80 位精度下已经饱和：
                //   1 - tanh(x) = 2e^{-2x}/(1+e^{-2x}) < 2e^{-200} < 1e-80
                // 直接返回极限值 ±1（数学上就是 80 位精度下的精确结果）。
                // 旧实现阈值取 10^10：5·10^5 < |x| < 10^10 之间会先撞上 exp 的
                // 参数上限而误报"exp 参数过大"（如 tanh(1e9)），|x| > 10^10 却
                // 正常返回 1，行为不一致；Deep 模式下更会让 exp(2x) 真的去算
                // 10^9 位十进制。阈值取 100 后所有 |x| 都能给出稳定结果。
                // 阈值按精度派生：1 - tanh(x) ≈ 2e^{-2x} < 10^-precision ⇔ |x| > (precision·ln10 + ln2)/2。
                // 低精度时取 max(2.0)（即 |x| > 100）保证不会把远未饱和的值当成 ±1。
                let limit_x = (bigfloat::precision() as f64) * std::f64::consts::LN_10 / 2.0 + 0.5;
                let limit_log10 = limit_x.log10().max(2.0);
                let b = arg.to_approx();
                if b.magnitude_log10() > limit_log10 {
                    let one = bigfloat::BigFloat::from_i64(1);
                    return Ok(Number::Approx(if b.value.is_negative() {
                        bigfloat::BigFloat::neg(&one)
                    } else {
                        one
                    }));
                }
                let two = bigfloat::BigFloat::from_i64(2);
                let t = bigfloat::BigFloat::mul(&two, &b, bigfloat::precision())
                    .exp(bigfloat::precision())?;
                let one = bigfloat::BigFloat::from_i64(1);
                let num = bigfloat::BigFloat::sub(&t, &one, bigfloat::precision());
                let den = bigfloat::BigFloat::add(&t, &one, bigfloat::precision());
                if den.is_zero() {
                    // x→∞ 的极限情形，双曲正切 ≈ 1
                    return Ok(Number::Approx(one));
                }
                Ok(Number::Approx(bigfloat::BigFloat::div(
                    &num,
                    &den,
                    bigfloat::precision(),
                )))
            }
            "sin" => {
                check_trig_range(arg, self.angle_mode)?;
                Ok(trig::sin_number(arg, self.angle_mode))
            }
            "cos" => {
                check_trig_range(arg, self.angle_mode)?;
                Ok(trig::cos_number(arg, self.angle_mode))
            }
            "tan" => {
                check_trig_range(arg, self.angle_mode)?;
                trig::tan_number(arg, self.angle_mode)
            }
            "cot" => {
                check_trig_range(arg, self.angle_mode)?;
                trig::cot_number(arg, self.angle_mode)
            }
            "sec" => {
                check_trig_range(arg, self.angle_mode)?;
                trig::sec_number(arg, self.angle_mode)
            }
            "csc" => {
                check_trig_range(arg, self.angle_mode)?;
                trig::csc_number(arg, self.angle_mode)
            }
            "arcsin" => {
                let approx = arg.to_approx();
                if approx.value.abs() > BigInt::from(10).pow(approx.precision as u32) {
                    return Err("arcsin 参数必须在 [-1, 1] 范围内".to_string());
                }
                if let Some(exact) = trig::try_exact_arcsin(arg) {
                    return Ok(to_degrees_if_needed(exact, self.angle_mode));
                }
                Ok(to_degrees_if_needed(
                    Number::Approx(approx.asin(bigfloat::precision())),
                    self.angle_mode,
                ))
            }
            "arccos" => {
                let approx = arg.to_approx();
                if approx.value.abs() > BigInt::from(10).pow(approx.precision as u32) {
                    return Err("arccos 参数必须在 [-1, 1] 范围内".to_string());
                }
                if let Some(exact) = trig::try_exact_arccos(arg) {
                    return Ok(to_degrees_if_needed(exact, self.angle_mode));
                }
                Ok(to_degrees_if_needed(
                    Number::Approx(approx.acos(bigfloat::precision())),
                    self.angle_mode,
                ))
            }
            "arctan" => {
                if let Some(exact) = trig::try_exact_arctan(arg) {
                    return Ok(to_degrees_if_needed(exact, self.angle_mode));
                }
                let approx = arg.to_approx();
                Ok(to_degrees_if_needed(
                    Number::Approx(approx.atan(bigfloat::precision())),
                    self.angle_mode,
                ))
            }
            "arccot" => {
                let approx = arg.to_approx();
                Ok(to_degrees_if_needed(
                    Number::Approx(approx.acot(bigfloat::precision())),
                    self.angle_mode,
                ))
            }
            "arcsec" => {
                let approx = arg.to_approx();
                let one = BigInt::from(10).pow(approx.precision as u32);
                if approx.value.abs() < one {
                    return Err("arcsec 参数必须满足 |x| >= 1".to_string());
                }
                Ok(to_degrees_if_needed(
                    Number::Approx(approx.asec(bigfloat::precision())),
                    self.angle_mode,
                ))
            }
            "arccsc" => {
                let approx = arg.to_approx();
                let one = BigInt::from(10).pow(approx.precision as u32);
                if approx.value.abs() < one {
                    return Err("arccsc 参数必须满足 |x| >= 1".to_string());
                }
                let approx_inner = arg.to_approx();
                Ok(to_degrees_if_needed(
                    Number::Approx(approx_inner.acsc(bigfloat::precision())),
                    self.angle_mode,
                ))
            }
            "sd" => Err("sd 只能作为最外层函数使用".to_string()),
            "factor" | "fac" => Err("fac/factor 必须作为最外层函数使用".to_string()),
            _ => Err(format!("未知函数: {}", name)),
        }
    }
}

/// exp / 双曲函数的参数范围保护：|x| 超过上限时报错。
/// 超限时 exp 的结果位数会失控（|x|·log10(e) 位十进制），也会拖慢整机。
/// 死算模式 (/mode deep) 下不拦截。
fn check_exp_range(arg: &Number) -> Result<(), String> {
    if hipercalc_core::calc_mode::is_deep() {
        return Ok(());
    }
    let log10 = arg.to_approx().magnitude_log10();
    if log10 > bigfloat::EXP_ARG_LIMIT_LOG10 {
        return Err(format!(
            "参数过大（约 10^{:.0}），指数运算结果将超出支持范围",
            log10
        ));
    }
    Ok(())
}

/// 三角函数参数范围保护。
/// 内部用 80 位精度的 2π 做参数归约，归约误差约为 |x|·1.6e-101；
/// |弧度参数| ≥ 10^78 时误差已接近 20 位有效数字的显示精度
/// （实测 sin(1e80) 侥幸正确、sin(1e100) 起与真值完全无关），故直接拒绝。
/// 死算模式 (/mode deep) 下不拦截（结果不可靠，由用户自行判断）。
fn check_trig_range(arg: &Number, mode: AngleMode) -> Result<(), String> {
    if hipercalc_core::calc_mode::is_deep() {
        return Ok(());
    }
    let mut log10 = arg.to_approx().magnitude_log10();
    if mode == AngleMode::Degree {
        // 度 → 弧度：×π/180 ⇒ 量级减少 log10(180/π)
        log10 -= 1.758_122_473_127_204_4;
    }
    // 归约误差 ≈ |x|·10^-(precision+21)，限定 |x| ≤ 10^(precision-2) 使其保持在显示精度附近
    // （默认 80 位 → 10^78，与历史阈值一致）
    let limit = bigfloat::precision() as f64 - 2.0;
    if log10 > limit {
        return Err(format!(
            "参数过大（约 10^{0:.0} 弧度），{1} 位精度的 π 归约会失效，三角函数结果不可靠",
            log10,
            bigfloat::precision()
        ));
    }
    Ok(())
}

/// 折叠"不含变量的整数常量表达式"，用于幂指数（`x^(2-1)` → `x^1`）。
///
/// 只处理 Number / 一元 ± / 四则 / 小整数幂这几种可静态求值的形式；
/// 遇到变量、函数、非整数结果一律返回 None，调用方保持原样。
/// 结果过大（> 2^16 位）也不折叠，避免解析阶段构造巨数。
fn fold_const_int(expr: &Expr) -> Option<BigInt> {
    /// 折叠结果的位宽上限（约 2 万位十进制）
    const MAX_FOLD_BITS: u64 = 1 << 16;

    let v = match expr {
        Expr::Number(n) => n
            .as_rational()
            .filter(|r| r.is_integer())
            .map(|r| r.to_integer())?,
        Expr::Unary(op, e) => {
            let v = fold_const_int(e)?;
            match op {
                UnaryOp::Neg => -v,
                UnaryOp::Pos => v,
            }
        }
        Expr::Binary(l, op, r) => {
            let a = fold_const_int(l)?;
            let b = fold_const_int(r)?;
            match op {
                BinOp::Add => a + b,
                BinOp::Sub => a - b,
                BinOp::Mul => a * b,
                BinOp::Div => {
                    // 要求整除，否则保留原式（分数指数交给求值阶段处理）
                    if b.is_zero() || !(&a % &b).is_zero() {
                        return None;
                    }
                    a / b
                }
            }
        }
        Expr::Pow(b, e) => {
            let base = fold_const_int(b)?;
            let exp = fold_const_int(e)?.to_u32()?;
            if exp > 4096 {
                return None;
            }
            base.pow(exp)
        }
        _ => return None,
    };
    if v.bits() > MAX_FOLD_BITS {
        return None;
    }
    Some(v)
}

/// 如果处于角度模式，将弧度结果转为度数
pub(crate) fn to_degrees_if_needed(result: Number, mode: AngleMode) -> Number {
    match mode {
        AngleMode::Radian => result,
        AngleMode::Degree => radians_to_degrees(result),
    }
}

/// 弧度转度数：result * 180 / pi
pub(crate) fn radians_to_degrees(result: Number) -> Number {
    if let Number::Complex(z) = &result {
        // 复数的两个分量各自换算（角度模式只影响用户可见的三角函数结果）
        let re = radians_to_degrees(z.re.clone());
        let im = radians_to_degrees(z.im.clone());
        return Number::from_complex(hipercalc_core::complex::ComplexNum::new(re, im));
    }
    match result {
        // 上面已提前返回复数
        Number::Complex(_) => unreachable!("radians_to_degrees 已在入口处理复数"),
        // 量纲值：角度换算只对三角结果有意义，量纲值原样透传
        Number::Matrix(m) => Number::Matrix(m),
        Number::Quantity(q) => Number::Quantity(q),
        Number::Matrix(m) => Number::Matrix(m), // 矩阵原样透传（角度制只作用于三角标量结果）
        Number::Exact(expr) => {
            // 精确值转换：Pi(coeff) → Rational(coeff * 180)
            let mut new_terms: Vec<hipercalc_core::number::ExactTerm> = Vec::new();
            for term in &expr.terms {
                use hipercalc_core::number::ExactTerm;
                match term {
                    ExactTerm::PiPow(coeff, k) if k.is_one() => {
                        // coeff * pi 弧度 = coeff * 180 度
                        let deg_coeff = coeff * BigRational::from_integer(BigInt::from(180));
                        new_terms.push(ExactTerm::Rational(deg_coeff));
                    }
                    _other => {
                        // 非 pi 项无法精确转换，回退到数值
                        let approx = expr.to_bigfloat();
                        let pi = hipercalc_core::bigfloat::BigFloat::pi(
                            hipercalc_core::bigfloat::precision(),
                        );
                        let deg_factor = hipercalc_core::bigfloat::BigFloat::div(
                            &hipercalc_core::bigfloat::BigFloat::from_u64(180),
                            &pi,
                            hipercalc_core::bigfloat::precision(),
                        );
                        let converted = hipercalc_core::bigfloat::BigFloat::mul(
                            &approx,
                            &deg_factor,
                            hipercalc_core::bigfloat::precision(),
                        );
                        return Number::Approx(converted);
                    }
                }
            }
            Number::Exact(ExactExpr {
                terms: new_terms,
                denominator: expr.denominator,
            })
        }
        Number::Approx(approx) => {
            let pi = hipercalc_core::bigfloat::BigFloat::pi(hipercalc_core::bigfloat::precision());
            let deg_factor = hipercalc_core::bigfloat::BigFloat::div(
                &hipercalc_core::bigfloat::BigFloat::from_u64(180),
                &pi,
                hipercalc_core::bigfloat::precision(),
            );
            Number::Approx(hipercalc_core::bigfloat::BigFloat::mul(
                &approx,
                &deg_factor,
                hipercalc_core::bigfloat::precision(),
            ))
        }
    }
}

/// 解析并求值的结果
pub enum EvalResult {
    /// 普通数值结果
    Value(Number),
    /// sd 函数的数值结果
    SdValue(Number),
    /// factor 函数的因式分解表达式（由 REPL 按显示模式分解）
    Factor(Box<Expr>),
    /// 单个方程（需要求解）
    Equation(Box<Expr>, Box<Expr>),
    /// 方程组（需要求解）
    System(Vec<(Box<Expr>, Box<Expr>)>),
    /// 不等式（组）：每项是 (移项成 `f op 0` 后的 f, 运算符编码)。
    /// 由 `< <= > >= !=` 脱糖而来（见 `parse_equation` 里的 `ineq` 函数）。
    Inequality(Vec<(Expr, u8)>),
    /// 多项式拟合输入：坐标（含顶点标记）+ 可选解析式模板
    Fit(Box<crate::solver_fit::FitInput>),
    /// 三角形求解输入：空白分隔的赋值（`a=3 b=4 c=5`、`A=30 b=5 C=60`、`hA=4 a=3 b=4`）
    Triangle(Box<crate::solver_triangle::TriangleInput>),
    /// `primefac(非零整数)`：参数已在解析期求值并校验（由 REPL 格式化成 `12 = 2^2 * 3`）
    PrimeFac(Number),
    /// 高等数学的**符号结果**（含自由变量）：交给 REPL 按显示模式渲染。
    /// 注意它**不写入 `ans`**（`ans` 只能装 `Number`）。
    Symbolic(Box<Expr>),
}

/// 判断一行是否"看起来是多项式拟合输入"（坐标 + 可选解析式）。
///
/// 只在**现有文法必然报错**的空白区认领：
/// - 行首是 `(`，或 `P(` / `p(`（`P`/`p` 后允许空白）；
/// - 第一个括号组内 depth==1 层必须出现过逗号（普通括号分组里出现逗号是非法语法）；
/// - 与该 `)` 匹配之后的第一个非空白字符只能是：行尾 / `(` / ASCII 字母。
///
/// 因此 `(1+2)*3`（无逗号）、`f(1,2)`/`log(1,2)`（首字符是字母）、
/// `P(2)`（括号内无逗号）、`(1,2)+3`（尾字符是 `+`）、`P+1`、`2P` 都**不会**被认领，行为保持不变。
pub fn looks_like_polynomial_fit(input: &str) -> bool {
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    if i >= chars.len() {
        return false;
    }
    if chars[i] == 'P' || chars[i] == 'p' {
        let mut j = i + 1;
        while j < chars.len() && chars[j].is_whitespace() {
            j += 1;
        }
        if chars.get(j) != Some(&'(') {
            return false;
        }
        i = j;
    }
    if chars.get(i) != Some(&'(') {
        return false;
    }
    let mut depth = 0i32;
    let mut saw_comma_at_top = false;
    let mut k = i;
    while k < chars.len() {
        match chars[k] {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            ',' if depth == 1 => saw_comma_at_top = true,
            _ => {}
        }
        k += 1;
    }
    if depth != 0 || !saw_comma_at_top {
        return false;
    }
    let mut m = k + 1;
    while m < chars.len() && chars[m].is_whitespace() {
        m += 1;
    }
    if m >= chars.len() {
        return true; // 行尾：只有坐标、没有解析式
    }
    let c = chars[m];
    c == '(' || c.is_ascii_alphabetic()
}

/// 找到顶层（括号/绝对值之外）的 `=`，返回其后的文本
fn split_top_level_assign(s: &str) -> Option<&str> {
    let mut depth: i32 = 0;
    let mut in_abs = false;
    for (idx, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            '|' => in_abs = !in_abs,
            '=' if depth == 0 && !in_abs => return Some(&s[idx + 1..]),
            _ => {}
        }
    }
    None
}

/// 解析拟合行尾部的"解析式"：按顶层 `=` 切开，**左值整体忽略**（输出恒为 `y = …`）
fn parse_fit_template(tail: &str) -> Result<Option<crate::solver_fit::FitTemplate>, String> {
    let trimmed = tail.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let rhs_src = split_top_level_assign(trimmed).unwrap_or(trimmed).trim();
    if rhs_src.is_empty() {
        return Err("坐标格式错误（应为 (x,y) 或 P(x,y)）".to_string());
    }
    let mut p = Parser::new(rhs_src);
    let rhs = p.parse_expression()?;
    p.skip_whitespace();
    if p.pos != p.input.len() {
        return Err(format!("位置 {} 处多余的字符", p.pos));
    }
    Ok(Some(crate::solver_fit::FitTemplate { rhs, var: 'x' }))
}

/// 三角形求解入口：`triangle(a=3 b=4 c=5)` / `triangle(a=3, b=4, c=5)`。
///
/// 与 `sd` / `fac` 同一约定：**必须是整个表达式的最外层函数**，不能参与其它运算。
/// 括号内是按空白**或**逗号分隔的 `<记号>=<表达式>`（逗号在括号内，如 `log(2,8)`，不会被当分隔符）。
///
/// - `Ok(Some(..))`：认领，交给求解阶段；
/// - `Ok(None)`：这条输入与 `triangle` 无关，继续走常规文法；
/// - `Err(..)`：用户显然想用 `triangle` 但写法不对（不是最外层 / 缺括号 / 内容非法），直接报错不回落。
///
/// 说明：旧版允许裸写 `a=3 b=4 c=5`，那与"方程组/隐式乘法"的边界很容易混淆
/// （`a=3 b=4 c=5` 本会被读成 `a = 3*b`），故改为显式函数调用。
pub fn parse_triangle_call(
    input: &str,
) -> Result<Option<crate::solver_triangle::TriangleInput>, String> {
    const OUTERMOST: &str = "triangle() 必须是整个表达式的最外层函数，不能参与其它运算";
    let trimmed = input.trim();
    // 是否以独立的 `triangle` 标识符开头（后面不能紧跟标识符字符）
    let starts_with_ident = trimmed.as_bytes().starts_with(b"triangle")
        && trimmed
            .as_bytes()
            .get("triangle".len())
            .is_none_or(|b| !is_ident_byte(*b));
    if !starts_with_ident {
        // 别处出现了 triangle ⇒ 用在了非最外层
        if contains_ident(trimmed, "triangle") {
            return Err(OUTERMOST.to_string());
        }
        return Ok(None);
    }
    let after = trimmed["triangle".len()..].trim_start();
    if !after.starts_with('(') {
        return Err("triangle 函数需要参数: triangle(a=3 b=4 c=5)".to_string());
    }
    let (body, tail) = match split_paren_group(after) {
        Some(v) => v,
        None => return Err("缺少右括号 ')'".to_string()),
    };
    if !tail.trim().is_empty() {
        return Err(OUTERMOST.to_string());
    }
    let parts = parse_triangle_items(body)?;
    Ok(Some(crate::solver_triangle::TriangleInput { parts }))
}

/// 标识符字符（字母/数字/下划线）
fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// 串里是否出现**独立的** `ident` 标识符（前后都不是标识符字符）。
/// 用于判断某个"必须最外层"的函数是否被用在了别处——词边界匹配可避免
/// `primefac` 里的 `fac`、或变量名里的同形子串被误判。
fn contains_ident(s: &str, ident: &str) -> bool {
    let bytes = s.as_bytes();
    let needle = ident.as_bytes();
    (0..bytes.len()).any(|i| {
        bytes[i..].starts_with(needle)
            && (i == 0 || !is_ident_byte(bytes[i - 1]))
            && (i + needle.len() >= bytes.len() || !is_ident_byte(bytes[i + needle.len()]))
    })
}

/// 从形如 `( … )尾巴` 的串里切出括号内容与尾巴；深度感知，支持嵌套括号。
/// 字节下标只在 ASCII 括号处切分，因此多字节 UTF-8 内容安全。
fn split_paren_group(s: &str) -> Option<(&str, &str)> {
    let b = s.as_bytes();
    if b.first() != Some(&b'(') {
        return None;
    }
    let mut depth = 0usize;
    for (i, &c) in b.iter().enumerate() {
        match c {
            b'(' => depth += 1,
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some((&s[1..i], &s[i + 1..]));
                }
            }
            _ => {}
        }
    }
    None
}

/// 按"顶层逗号**或**空白"切分括号内内容：括号里的逗号/空白（如 `log(2,8)`）不切分
fn split_triangle_items(body: &str) -> Vec<&str> {
    let b = body.as_bytes();
    let mut out: Vec<&str> = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for i in 0..b.len() {
        match b[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b',' | b' ' | b'\t' | b'\n' | b'\r' if depth == 0 => {
                if start < i {
                    out.push(&body[start..i]);
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    if start < b.len() {
        out.push(&body[start..]);
    }
    out
}

/// 解析 `triangle(...)` 括号内的各项 `<记号>=<表达式>`。
/// 每个右端表达式用独立 `Parser` 解析（支持 `1/2`、`sqrt(2)`、`pi/6`），求值延迟到求解阶段。
fn parse_triangle_items(
    body: &str,
) -> Result<Vec<(crate::solver_triangle::TriPart, Expr)>, String> {
    let mut parts: Vec<(crate::solver_triangle::TriPart, Expr)> = Vec::new();
    for item in split_triangle_items(body) {
        let eq = match item.find('=') {
            Some(i) => i,
            None => return Err(format!("三角形记号格式错误: {0}", item)),
        };
        let (lhs, rhs) = (&item[..eq], &item[eq + 1..]);
        let part = match crate::solver_triangle::tri_part_from_name(lhs) {
            Some(p) => p,
            None => return Err(format!("未知的三角形记号: {0}", lhs)),
        };
        if rhs.is_empty() {
            return Err("三角形赋值缺少右端表达式".to_string());
        }
        let mut p = Parser::new(rhs);
        let expr = p.parse_expression()?;
        p.skip_whitespace();
        if p.pos != p.input.len() {
            return Err(format!("位置 {} 处多余的字符", p.pos));
        }
        parts.push((part, expr));
    }
    if parts.is_empty() {
        return Err("三角形记号格式错误".to_string());
    }
    Ok(parts)
}

/// `primefac(非零整数)` 的入口：与 `sd` / `fac` / `triangle` 同一约定——**只能是整个表达式的最外层函数**。
///
/// - `Ok(Some(n))`：认领，`n` 是求值并校验过的**非零整数**；
/// - `Ok(None)`：与本函数无关，继续走常规文法；
/// - `Err(..)`：想用但写法不对（不是最外层 / 缺括号 / 参数不是非零整数）——直接报错、**不回落**。
///
/// 与 `fac`/`factor` 的分工：`fac` 的整数入参也走素因数分解（见 `main::handle_factor`），
/// 但那条路径是"非零整数 ⇒ 分解，否则回落多项式分解"；本函数是**严格入口**，
/// 小数、0 一律报错（含自由变量的表达式则由求值错误报出，如 `未定义变量: x`）。
pub fn parse_primefac(input: &str, evaluator: &mut Evaluator) -> Result<Option<Number>, String> {
    const IDENT: &str = "primefac";
    const OUTERMOST: &str = "primefac() 必须是整个表达式的最外层函数，不能参与其它运算";
    const NEED_INT: &str = "primefac 的参数必须是非零整数";
    let trimmed = input.trim();
    let starts_with_ident = trimmed.as_bytes().starts_with(IDENT.as_bytes())
        && trimmed
            .as_bytes()
            .get(IDENT.len())
            .is_none_or(|b| !is_ident_byte(*b));
    if !starts_with_ident {
        // 别处出现了 primefac ⇒ 用在了非最外层
        if contains_ident(trimmed, IDENT) {
            return Err(OUTERMOST.to_string());
        }
        return Ok(None);
    }
    let after = trimmed[IDENT.len()..].trim_start();
    if !after.starts_with('(') {
        return Err("primefac 函数需要参数: primefac(12)".to_string());
    }
    let (body, tail) = match split_paren_group(after) {
        Some(v) => v,
        None => return Err("缺少右括号 ')'".to_string()),
    };
    if !tail.trim().is_empty() {
        return Err(OUTERMOST.to_string());
    }
    // 括号内是可求值的表达式（`primefac(2+3)`、`primefac(-2^2)` 都合法）
    let mut p = Parser::new(body.trim());
    let expr = p.parse_expression()?;
    p.skip_whitespace();
    if p.pos != p.input.len() {
        return Err(format!("位置 {} 处多余的字符", p.pos));
    }
    let value = evaluator.evaluate(&expr)?;
    let n = as_int(&value, NEED_INT)?;
    if n == BigInt::from(0u32) {
        return Err(NEED_INT.to_string());
    }
    Ok(Some(Number::from_bigint(n)))
}

/// 矩阵与标量组合的合法性预检（`+ - *` 的矩阵语义只允许这几种组合）。
/// 除法对矩阵一律拒绝，提示改用 `inv(A)*B`。
fn check_matrix_op(l: &Number, op: &BinOp, r: &Number) -> Result<(), String> {
    let (lm, rm) = (l.as_matrix(), r.as_matrix());
    match op {
        BinOp::Add | BinOp::Sub => match (lm, rm) {
            (Some(a), Some(b)) if a.len() == b.len() && a[0].len() == b[0].len() => Ok(()),
            (Some(_), Some(_)) => Err("矩阵加减要求两边同形".to_string()),
            _ => Err("矩阵不能与标量相加减（不做广播）".to_string()),
        },
        BinOp::Mul => match (lm, rm) {
            (Some(a), Some(b)) if a[0].len() == b.len() => Ok(()),
            (Some(_), Some(_)) => Err("矩阵乘法要求左矩阵的列数等于右矩阵的行数".to_string()),
            _ => Ok(()), // 数 × 矩阵 / 矩阵 × 数
        },
        BinOp::Div => Err("矩阵不支持除法（可用 inv(A)*B）".to_string()),
    }
}

/// 把若干 `ineq(左, 右, 运算符编码)` 调用组装成 `EvalResult::Inequality`：
/// 左-右 移项成多项式 `f`，并取出运算符与自变量。
fn ineq_result(items: Vec<(String, Vec<Expr>)>) -> Result<EvalResult, String> {
    let mut out: Vec<(Expr, u8)> = Vec::new();
    for (_, mut args) in items {
        if args.len() != 3 {
            return Err("不等式的内部表示异常（应为 3 个参数）".to_string());
        }
        let op = args.pop().expect("已判长度");
        let right = args.pop().expect("已判长度");
        let left = args.pop().expect("已判长度");
        let code = match &op {
            Expr::Number(n) => n
                .as_rational()
                .map(|r| r.to_integer().to_u8())
                .flatten()
                .unwrap_or(0),
            _ => return Err("不等式的运算符编码异常".to_string()),
        };
        // 移项：f = 左 - 右
        out.push((
            Expr::Binary(Box::new(left), BinOp::Sub, Box::new(right)),
            code,
        ));
    }
    Ok(EvalResult::Inequality(out))
}

/// 解析并求值顶层输入
pub fn parse_and_eval(input: &str, evaluator: &mut Evaluator) -> Result<EvalResult, String> {
    let mut parser = Parser::new(input);
    parser.skip_whitespace();

    if parser.pos >= parser.input.len() {
        return Err("空表达式".to_string());
    }

    // 三角形求解：triangle(已知量)，与 sd/fac 同属"必须最外层"的顶层函数。
    // 放在最前面：① 与拟合/方程组无冲突（都以 `t` 开头）；② 用错位置时能立刻给出明确提示。
    if let Some(tri) = parse_triangle_call(input)? {
        return Ok(EvalResult::Triangle(Box::new(tri)));
    }

    // 素因数分解：primefac(非零整数)，同样必须最外层。
    // 必须在 `parse_factor` 之前——虽然 `primefac` 与 `fac`/`factor` 是不同的标识符，
    // 但两者都要在 `parse_system` 之前拦截，否则 `primefac(12)` 会被当普通函数调用。
    if let Some(n) = parse_primefac(input, evaluator)? {
        return Ok(EvalResult::PrimeFac(n));
    }

    // 先尝试解析为"多项式拟合"（坐标 + 可选解析式）：
    // 只在现有文法必然报错的空白区认领（见 looks_like_polynomial_fit），认领后失败即报错、不回落
    if looks_like_polynomial_fit(input) {
        let mut fit_parser = Parser::new(input);
        let fit = fit_parser.parse_fit()?;
        fit_parser.skip_whitespace();
        if fit_parser.pos != fit_parser.input.len() {
            return Err(format!("位置 {} 处多余的字符", fit_parser.pos));
        }
        return Ok(EvalResult::Fit(Box::new(fit)));
    }

    // 先尝试解析为 sd 表达式
    if let Ok(sd_expr) = parser.parse_sd() {
        // 高等数学函数也允许出现在 sd(...) 里；若它出现在 sd/fac 内部，展开时会明确报错
        let sd_expr = crate::calculus::expand_calculus(evaluator, sd_expr)?;
        if let Expr::Sd(inner) = &sd_expr
            && let Expr::Equation(left, right) = inner.as_ref()
        {
            return Ok(EvalResult::Equation(left.clone(), right.clone()));
        }
        let result = evaluator.evaluate(&sd_expr)?;
        return Ok(EvalResult::SdValue(result));
    }

    // 再尝试解析为 factor 表达式
    if let Ok(factor_expr) = parser.parse_factor()
        && let Expr::Factor(inner) = &factor_expr
    {
        // 高等数学函数不允许出现在 fac/factor 内部；走一次重写通路即可给出明确报错
        // （否则会落到"无法因式分解：仅支持有理数系数的多项式"，看不出真正原因）
        let _ = crate::calculus::expand_calculus(evaluator, factor_expr.clone())?;
        return Ok(EvalResult::Factor(inner.clone()));
    }

    // 解析为方程组、等式或表达式
    let mut parser = Parser::new(input);
    let expr = parser.parse_system()?;
    parser.skip_whitespace();
    if parser.pos != parser.input.len() {
        return Err(format!("位置 {} 处多余的字符", parser.pos));
    }

    // 高等数学：**解析完成后、分类/求值前**把 `diff(...)` 之类就地展开成等价普通表达式。
    // 这样它们既能参与运算（`diff(x^2,x)+1`），又能复用既有的求值/方程/显示链路。
    // 预扫描未命中时原样返回 ⇒ 既有输入的行为完全不变（这是本改动最重要的不变量）。
    // 统计函数：同样在解析完成后、求值前就地展开（`[1,2,3]` 已在解析期脱糖成 list）
    // —— 必须放在 calculus 判断**之前**：否则没有 `diff` 之类的纯统计表达式不会被展开。
    let expr = crate::stats::expand_stats(evaluator, &expr)?;
    // 统计展开会**生成 `sum(...)` 节点**，而那个节点要靠 calculus 那一遍来展开 ⇒
    // 预扫必须把"输入里有统计函数"也算进来，否则 `mean(k, k, 1, 4)` 会残留一个未展开的 sum。
    let had_calculus = crate::calculus::input_may_have_calculus(input)
        || crate::stats::input_may_have_stats(input);
    let expr = if had_calculus {
        crate::calculus::expand_calculus(evaluator, expr)?
    } else {
        expr
    };

    match expr {
        // 不等式（组）：元素是脱糖出的 `ineq(f, 0, op)` 调用
        Expr::Function(name, args) if name == "ineq" => {
            return crate::parser::ineq_result(vec![(name, args)]);
        }
        Expr::System(eqs) => {
            if eqs
                .iter()
                .all(|e| matches!(e, Expr::Function(n, _) if n == "ineq"))
            {
                let items: Vec<(String, Vec<Expr>)> = eqs
                    .into_iter()
                    .map(|e| match e {
                        Expr::Function(n, a) => (n, a),
                        _ => unreachable!(),
                    })
                    .collect();
                return crate::parser::ineq_result(items);
            }
            let mut pairs = Vec::new();
            for eq in eqs {
                match eq {
                    Expr::Equation(left, right) => {
                        pairs.push((left, right));
                    }
                    _ => return Err("方程组中每一项都必须是等式".to_string()),
                }
            }
            Ok(EvalResult::System(pairs))
        }
        Expr::Equation(left, right) => Ok(EvalResult::Equation(left, right)),
        _ => {
            // 含自由变量的**符号结果**（如 `diff(x^2,x)` → `2*x`）交给 REPL 渲染。
            // 只在输入里真的用了高等数学函数时才走这条路：否则 `x+1` 这类输入
            // 仍应照旧报"未定义变量: x"，不能因为本功能把既有行为改掉。
            if had_calculus && crate::calculus::has_free_variables(evaluator, &expr) {
                Ok(EvalResult::Symbolic(Box::new(expr)))
            } else {
                let result = evaluator.evaluate(&expr)?;
                Ok(EvalResult::Value(result))
            }
        }
    }
}

#[cfg(test)]
mod func_tests {
    use super::*;
    use hipercalc_core::display;

    /// 求值并返回 LineIO 显示串（与命令行输出同一套格式化）
    fn eval_lineio(expr: &str) -> Result<String, String> {
        let mut ev = Evaluator::new();
        match parse_and_eval(expr, &mut ev) {
            Ok(EvalResult::Value(v)) | Ok(EvalResult::SdValue(v)) => Ok(display::format_lineio(&v)),
            Ok(_) => Ok("<non-value>".to_string()),
            Err(e) => Err(e),
        }
    }

    fn eval_mathio(expr: &str) -> Result<String, String> {
        let mut ev = Evaluator::new();
        match parse_and_eval(expr, &mut ev) {
            Ok(EvalResult::Value(v)) | Ok(EvalResult::SdValue(v)) => Ok(display::format_mathio(&v)),
            Ok(_) => Ok("<non-value>".to_string()),
            Err(e) => Err(e),
        }
    }

    #[test]
    fn cube_root_is_exact_on_perfect_cubes() {
        assert_eq!(eval_mathio("cbrt(27)").unwrap(), "3");
        assert_eq!(eval_mathio("cbrt(-27)").unwrap(), "-3");
        assert_eq!(eval_mathio("cbrt(1/8)").unwrap(), "1 / 2");
        assert_eq!(eval_mathio("cbrt(0)").unwrap(), "0");
        // 非完全立方走数值：2 的立方根 1.2599210498948731648…
        let v = eval_lineio("cbrt(2)").unwrap();
        assert!(
            v.starts_with("1.2599210498948731647") || v.starts_with("1.2599210498948731648"),
            "{v}"
        );
        // 回验：(cbrt(2))^3 在 20 位显示精度下就是 2
        let back = eval_lineio("cbrt(2)^3").unwrap();
        assert!(back.starts_with("2"), "{back}");
    }

    #[test]
    fn inverse_hyperbolic_values_and_domains() {
        assert_eq!(eval_mathio("arcsinh(0)").unwrap(), "0");
        assert_eq!(eval_mathio("arccosh(1)").unwrap(), "0");
        assert_eq!(eval_mathio("arctanh(0)").unwrap(), "0");
        // arcsinh(1) = ln(1+√2) ≈ 0.88137358701954302523…
        let v = eval_lineio("arcsinh(1)").unwrap();
        assert!(v.starts_with("0.8813735870195430252"), "{v}");
        // 奇对称
        let a = eval_lineio("arcsinh(-1)").unwrap();
        assert!(a.starts_with("-0.8813735870195430252"), "{a}");
        // 定义域报错
        assert!(eval_lineio("arccosh(0.5)").is_err());
        assert!(eval_lineio("arctanh(2)").is_err());
        assert!(eval_lineio("arctanh(-1)").is_err());
    }

    #[test]
    fn hyperbolic_reciprocals() {
        // coth(1) = 1/tanh(1) ≈ 1.3130352854993313036
        let v = eval_lineio("coth(1)").unwrap();
        assert!(v.starts_with("1.313035285499331303"), "{v}");
        // sech(0) = 1
        assert_eq!(eval_mathio("sech(0)").unwrap(), "1");
        assert!(eval_lineio("csch(0)").is_err());
        assert!(eval_lineio("coth(0)").is_err());
        // 恒等式 coth² - csch² = 1（数值误差在显示精度内不可见）
        let v = eval_lineio("coth(2)^2 - csch(2)^2").unwrap();
        assert!(v.starts_with("1"), "{v}");
    }

    #[test]
    fn constants_tau_and_phi() {
        // tau = 2π（MathIO 显示为 2*pi）
        assert_eq!(eval_mathio("tau").unwrap(), "2*pi");
        assert!(
            eval_lineio("tau")
                .unwrap()
                .starts_with("6.2831853071795864769")
        );
        // phi = (1+√5)/2 ≈ 1.6180339887…
        assert!(
            eval_lineio("phi")
                .unwrap()
                .starts_with("1.6180339887498948482")
        );
        // 黄金比满足 φ² = φ + 1（精确）
        assert_eq!(eval_mathio("phi^2 - phi - 1").unwrap(), "0");
        // 与显式写法一致
        let a = eval_lineio("phi").unwrap();
        let b = eval_lineio("(1+sqr(5))/2").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn mod_and_idiv_are_euclidean() {
        // 余数恒非负、随 |除数| 归一（与 bigint_ext::div 的截断语义不同）
        assert_eq!(eval_mathio("mod(7,3)").unwrap(), "1");
        assert_eq!(eval_mathio("mod(-7,3)").unwrap(), "2");
        assert_eq!(eval_mathio("mod(7,-3)").unwrap(), "1");
        assert_eq!(eval_mathio("mod(-7,-3)").unwrap(), "2");
        assert_eq!(eval_mathio("idiv(7,3)").unwrap(), "2");
        assert_eq!(eval_mathio("idiv(-7,3)").unwrap(), "-3");
        assert_eq!(eval_mathio("idiv(7,-3)").unwrap(), "-2"); // div_euclid：7 = (-3)(-2) + 1
        // 有理数：mod(7/2, 1) = 1/2
        assert_eq!(eval_mathio("mod(7/2,1)").unwrap(), "1 / 2");
        // 恒等式 idiv(a,b)·b + mod(a,b) = a（多个组合）
        for (a, b) in [
            ("17", "5"),
            ("-17", "5"),
            ("17", "-5"),
            ("-17", "-5"),
            ("7", "1/3"),
        ] {
            let lhs = eval_lineio(&format!("idiv({a},{b})*{b} + mod({a},{b})")).unwrap();
            let rhs = eval_lineio(a).unwrap();
            assert_eq!(lhs, rhs, "恒等式在 a={a}, b={b} 下不成立");
        }
        // 错误：除数为 0 / 非精确参数
        assert!(eval_lineio("mod(5,0)").is_err());
        assert!(eval_lineio("idiv(5,0)").is_err());
        assert!(eval_lineio("mod(pi,2)").is_err());
    }

    #[test]
    fn nth_root_exact_and_numeric() {
        // 完全幂给精确值
        assert_eq!(eval_mathio("nroot(27,3)").unwrap(), "3");
        assert_eq!(eval_mathio("nroot(-32,5)").unwrap(), "-2");
        assert_eq!(eval_mathio("nroot(16,4)").unwrap(), "2");
        assert_eq!(eval_mathio("nroot(1/16,4)").unwrap(), "1 / 2");
        assert_eq!(eval_mathio("nroot(8,3)").unwrap(), "2");
        // 非完全幂走数值：2^(1/5) ≈ 1.1486983549970350068
        let v = eval_lineio("nroot(2,5)").unwrap();
        assert!(v.starts_with("1.148698354997035006"), "{v}");
        // 定义域与参数校验
        assert!(eval_lineio("nroot(-4,2)").is_err());
        assert!(eval_lineio("nroot(8,1)").is_err());
        assert!(eval_lineio("nroot(8,0)").is_err());
        assert!(eval_lineio("nroot(8,2.5)").is_err());
    }

    #[test]
    fn combinations_and_gcd() {
        assert_eq!(eval_mathio("nCr(5,2)").unwrap(), "10");
        assert_eq!(eval_mathio("nCr(10,0)").unwrap(), "1");
        assert_eq!(eval_mathio("nCr(5,7)").unwrap(), "0");
        assert_eq!(eval_mathio("nPr(10,3)").unwrap(), "720");
        assert_eq!(eval_mathio("nPr(5,7)").unwrap(), "0");
        // C(100,50) = 100891344545564193334812497256
        assert_eq!(
            eval_mathio("nCr(100,50)").unwrap(),
            "100891344545564193334812497256"
        );
        // 递推恒等式 C(n,r) = C(n-1,r-1) + C(n-1,r)
        let l = eval_lineio("nCr(60,20)").unwrap();
        let r = eval_lineio("nCr(59,19) + nCr(59,20)").unwrap();
        assert_eq!(l, r);
        // 参数校验
        assert!(eval_lineio("nCr(-1,2)").is_err());
        assert!(eval_lineio("nCr(5,1.5)").is_err());
        assert!(eval_lineio("nCr(20000,3)").is_err()); // Fast 上限 10000
        // gcd / lcm
        assert_eq!(eval_mathio("gcd(12,18)").unwrap(), "6");
        assert_eq!(eval_mathio("gcd(-12,18)").unwrap(), "6");
        assert_eq!(eval_mathio("lcm(4,6)").unwrap(), "12");
        let g = eval_lineio("gcd(123456,7890)").unwrap();
        let l2 = eval_lineio("lcm(123456,7890)").unwrap();
        assert_eq!(g, "6");
        assert_eq!(l2, "162344640"); // 123456·7890/6
        // gcd·lcm = |a·b|
        assert_eq!(
            eval_lineio("gcd(123456,7890)*lcm(123456,7890)").unwrap(),
            eval_lineio("123456*7890").unwrap()
        );
        assert!(eval_lineio("gcd(1.5,2)").is_err());
    }

    #[test]
    fn primality_functions() {
        assert_eq!(eval_mathio("isprime(97)").unwrap(), "1");
        assert_eq!(eval_mathio("isprime(2)").unwrap(), "1");
        assert_eq!(eval_mathio("isprime(1)").unwrap(), "0");
        assert_eq!(eval_mathio("isprime(0)").unwrap(), "0");
        assert_eq!(eval_mathio("isprime(-7)").unwrap(), "0");
        // Carmichael 数（纯试除会误判）
        assert_eq!(eval_mathio("isprime(561)").unwrap(), "0");
        assert_eq!(eval_mathio("isprime(1729)").unwrap(), "0");
        assert_eq!(eval_mathio("nextprime(100)").unwrap(), "101");
        assert_eq!(eval_mathio("nextprime(101)").unwrap(), "103");
        assert_eq!(eval_mathio("nextprime(0)").unwrap(), "2");
        // 大素数 2^61-1 判定为素数、nextprime 接着它
        assert_eq!(eval_mathio("isprime(2^61-1)").unwrap(), "1");
        assert_eq!(
            eval_mathio("nextprime(2^61-1)").unwrap(),
            "2305843009213693967"
        );
        // Fast 规模护栏
        assert!(eval_lineio("isprime(10^25)").is_err());
        assert!(eval_lineio("nextprime(10^25)").is_err());
        // 参数校验
        assert!(eval_lineio("isprime(1.5)").is_err());
    }

    #[test]
    fn complex_syntax_and_dispatch() {
        // 单位后缀：解析期折成 SI（精确有理数相除/相乘）
        for (input, want) in [
            ("3 km", "3000"),
            ("3km", "3000"),        // 连写也可以
            ("2 mile", "3218.688"), // 2 × 1609.344（精确）
            ("3 km + 500 meter", "3500"),
            ("2 hour * 60", "432000"),
        ] {
            let got = eval_lineio(input).unwrap();
            assert_eq!(got, want, "{input} → {got}，期望 {want}");
        }
        // 单位系数查询（/unit 指令用它）
        assert_eq!(
            unit_factor("km").and_then(|n| n.as_rational()),
            Some(BigRational::from_integer(BigInt::from(1000)))
        );
        assert!(unit_factor("xyz").is_none());
        // 关键非回归：`2 pi` 仍是隐式乘法，不能被单位表吃掉
        assert!(eval_lineio("2 pi").unwrap().starts_with("6.2831853"));
        // 内置物理常量：值取 CODATA 2022（SI 定义值逐位精确），并验证只读登记表
        for (name, want) in [
            ("C0", "299792458"),
            ("LIGHT_SPEED", "299792458"), // 长名别名
            ("GACC", "9.80665"),          // 精确值 196133/20000，LineIO 显示为小数
        ] {
            let got = eval_lineio(name).unwrap();
            assert!(got.contains(want), "{name} → {got}；期望含 {want}");
        }
        // 只读名单：短名与长名都要认出来
        assert!(is_phys_constant("C0") && is_phys_constant("LIGHT_SPEED"));
        // pi / e 不受影响（常量表里没有它们的名字）
        // pi 仍按数值输出（LineIO 显示小数，不是字符串 pi）—— 与既有测试同一写法
        assert!(eval_lineio("pi").unwrap().starts_with("3.14159"));
        // i 是常数：2i 走隐式乘法、i^2 = -1
        assert_eq!(eval_mathio("i").unwrap(), "i");
        assert_eq!(eval_mathio("2i").unwrap(), "2i");
        assert_eq!(eval_mathio("i^2").unwrap(), "-1");
        assert_eq!(eval_mathio("i^100").unwrap(), "1");
        assert_eq!(eval_mathio("(2+3i)*(1-i)").unwrap(), "5 + i");
        // 负实数的开方/对数自动进复数域（精确/半精确）
        assert_eq!(eval_mathio("sqrt(-4)").unwrap(), "2i");
        assert_eq!(eval_mathio("cbrt(-8)").unwrap(), "-2"); // 奇次根仍在实数域
        assert!(eval_lineio("ln(-1)").unwrap().starts_with("3.14159"));
        // 实部/虚部/共轭/辐角
        assert_eq!(eval_mathio("re(2+3i)").unwrap(), "2");
        assert_eq!(eval_mathio("im(2+3i)").unwrap(), "3");
        assert_eq!(eval_mathio("conj(2-3i)").unwrap(), "2 + 3i");
        assert!(eval_lineio("arg(1+i)").unwrap().starts_with("0.7853981"));
        assert_eq!(eval_mathio("abs(3+4i)").unwrap(), "5");
        // 复数非整数次幂（曾报错"暂不支持"，现已实现主值 √i = e^(iπ/4) = (1+i)/√2）
        assert!(
            eval_lineio("i^(1/2)")
                .unwrap()
                .starts_with("0.7071067811865475244")
        );
        // 仍不支持的情形给明确报错

        assert!(eval_lineio("floor(i)").unwrap_err().contains("复数"));
        assert!(eval_lineio("mod(1+i,2)").unwrap_err().contains("复数"));
        // i 不再是单字母变量
        assert!(!VALID_VARIABLES.contains('i'));
        assert!(eval_lineio("i2").is_err());
    }

    /// 取 `parse_and_eval` 的报错文案（`EvalResult` 未实现 `Debug`，不能直接用 `unwrap_err`）
    fn perr(input: &str, ev: &mut Evaluator) -> String {
        match parse_and_eval(input, ev) {
            Err(e) => e,
            Ok(_) => panic!("{input} 应当报错"),
        }
    }

    #[test]
    fn triangle_call_is_parsed_and_outermost_only() {
        let mut ev = Evaluator::new();
        // 会被认领：空白分隔与逗号分隔都支持
        for good in [
            "triangle(a=3 b=4 c=5)",
            "triangle(a=3, b=4, c=5)",
            "triangle(A=30 a=3 b=4)",
            "triangle(hA=4 a=3 b=4)",
            "triangle(a=1/2 b=sqrt(2) c=1)",
            "triangle( a=3   b=4 )",
            "triangle(a=log(2,8) b=4 c=5)", // 括号内的逗号是函数参数，不是项分隔符
        ] {
            assert!(
                matches!(parse_triangle_call(good), Ok(Some(_))),
                "{good} 应被认领"
            );
        }
        // 绝不能认领：这些在现有文法里有别的含义（合法或另有报错）
        for bad in [
            "a=3",           // 合法：解方程
            "a=3, b=4, c=5", // 合法：解方程组
            "x=1 y=2",       // 名字不在记号表
            "h=1 a=2 b=3",   // h 不是合法高度名（只认 hA/hB/hC）
            "hD=1 a=2 b=3",  // 同上
            "sinA=0.5 B=30", // sinA 不在记号表
            "a = 3 b = 4",   // 只支持紧凑写法
            "(1,2) (3,4)",   // 拟合输入
            "3+4",           // 普通表达式
            "",              // 空
            "a=3 b=",        // RHS 为空
            "a=3 b=4+",      // 尾随运算符：语法上认领，但解析会报错（见下）
        ] {
            if bad == "a=3 b=4+" {
                continue; // 这一条属于"空白区认领后解析报错"，单独验证
            }
            assert!(
                !matches!(parse_triangle_call(bad), Ok(Some(_))),
                "{bad} 不应被认领"
            );
        }

        // 只能作用于最外层：参与其它运算一律明确报错（不回落、不给误导性提示）
        for bad in [
            "2*triangle(a=3 b=4 c=5)",
            "triangle(a=3 b=4 c=5)+1",
            "triangle(a=3 b=4 c=5)*2",
            "-triangle(a=3 b=4 c=5)",
            "sin(triangle(a=3 b=4 c=5))",
        ] {
            let err = perr(bad, &mut ev);
            assert!(err.contains("最外层"), "{bad} → {err}");
        }

        // 缺括号 / 内容非法
        assert!(perr("triangle a=3 b=4", &mut ev).contains("需要参数"));
        assert!(perr("triangle(a=3 b=4", &mut ev).contains("右括号"));
        assert!(perr("triangle()", &mut ev).contains("记号格式错误"));
        assert!(perr("triangle(x=1 a=2)", &mut ev).contains("未知的三角形记号"));
        assert!(parse_and_eval("triangle(a=3 b=4+)", &mut ev).is_err());

        // 认领成功后确实得到三角形变体
        assert!(matches!(
            parse_and_eval("triangle(a=3 b=4 c=5)", &mut ev),
            Ok(EvalResult::Triangle(_))
        ));
    }

    #[test]
    fn bare_triangle_assignments_are_gone() {
        // 旧版裸写 `a=3 b=4 c=5` 已移除：行为回到原有语义（这里是"多余字符"报错），不再被当作三角形
        let mut ev = Evaluator::new();
        assert!(parse_and_eval("a=3 b=4 c=5", &mut ev).is_err());
        // 既有语义不受影响：单个 `a=3` 仍是方程，逗号分隔仍是方程组
        assert!(matches!(
            parse_and_eval("a=3", &mut ev),
            Ok(EvalResult::Equation(_, _))
        ));
        assert!(matches!(
            parse_and_eval("a=3, b=4, c=5", &mut ev),
            Ok(EvalResult::System(_))
        ));
    }

    #[test]
    fn gcd_lcm_require_nonzero_integers() {
        assert_eq!(eval_mathio("gcd(12,18)").unwrap(), "6");
        assert_eq!(eval_mathio("lcm(4,6)").unwrap(), "12");
        // 结果**恒为非负**：负数入参也一样
        for (input, want) in [
            ("gcd(-4,-6)", "2"),
            ("gcd(-4,6)", "2"),
            ("gcd(4,-6)", "2"),
            ("lcm(-4,-6)", "12"),
            ("lcm(-4,6)", "12"),
            ("lcm(4,-6)", "12"),
        ] {
            assert_eq!(eval_mathio(input).unwrap(), want, "{input}");
        }
        // 0 与小数都报错
        assert!(eval_lineio("gcd(0,6)").unwrap_err().contains("不能为 0"));
        assert!(eval_lineio("lcm(6,0)").unwrap_err().contains("不能为 0"));
        assert!(eval_lineio("gcd(0,0)").unwrap_err().contains("不能为 0"));
        assert!(eval_lineio("gcd(1.5,2)").unwrap_err().contains("整数"));
        assert!(eval_lineio("lcm(1/2,2)").unwrap_err().contains("整数"));
        // **不要求最外层**：可以参与运算
        assert_eq!(eval_mathio("2*gcd(12,18)").unwrap(), "12");
        assert_eq!(eval_mathio("lcm(3,5)*2").unwrap(), "30");
        assert_eq!(eval_mathio("gcd(12,18)+lcm(4,6)").unwrap(), "18");
    }

    #[test]
    fn primefac_is_outermost_only_and_strict() {
        let mut ev = Evaluator::new();
        // 认领（求值在解析期完成）：字面量、含运算的表达式、负号
        for good in [
            "primefac(12)",
            "primefac(-12)",
            "primefac( 2+3 )",
            "primefac(-2^2)",
        ] {
            assert!(
                matches!(parse_and_eval(good, &mut ev), Ok(EvalResult::PrimeFac(_))),
                "{good} 应被认领"
            );
        }
        // 只能作用于最外层
        for bad in [
            "2*primefac(12)",
            "primefac(12)+1",
            "-primefac(12)",
            "sin(primefac(12))",
        ] {
            let err = perr(bad, &mut ev);
            assert!(err.contains("最外层"), "{bad} → {err}");
        }
        // 严格参数：小数 / 0 / 非整数一律拒绝
        for bad in [
            "primefac(0)",
            "primefac(2.5)",
            "primefac(1/2)",
            "primefac(1e-3)",
        ] {
            assert!(perr(bad, &mut ev).contains("非零整数"), "{bad}");
        }
        // 缺括号 / 缺参数
        assert!(perr("primefac 12", &mut ev).contains("需要参数"));
        assert!(perr("primefac(12", &mut ev).contains("右括号"));
        // fac / factor 的整数入参也走素因数分解，但**仍解析为 Factor**（由 handle_factor 分流）
        assert!(matches!(
            parse_and_eval("fac(12)", &mut ev),
            Ok(EvalResult::Factor(_))
        ));
        assert!(matches!(
            parse_and_eval("factor(-12)", &mut ev),
            Ok(EvalResult::Factor(_))
        ));
    }

    #[test]
    fn zero_power_is_not_rejected() {
        // 回归：check_pow_scale 曾对零底数估出 inf 位数而误报错
        assert_eq!(eval_mathio("0^2").unwrap(), "0");
        assert_eq!(eval_mathio("0^5").unwrap(), "0");
        assert_eq!(eval_mathio("0^0").unwrap(), "1");
        assert_eq!(eval_mathio("0*3^2").unwrap(), "0");
    }

    #[test]
    fn component_functions_accept_real_arguments() {
        // `re/im/conj/arg` 是"分量操作"：实数 = 虚部为 0 的复数，必须同样可用。
        // 回归：此前只有实参含复数才走复数路径，`re(2)`、`arg(-1)` 会误报"未知函数: re"。
        assert_eq!(eval_mathio("re(2)").unwrap(), "2");
        assert_eq!(eval_mathio("im(2)").unwrap(), "0");
        assert_eq!(eval_mathio("conj(2)").unwrap(), "2");
        assert_eq!(eval_mathio("re(-5)").unwrap(), "-5");
        assert_eq!(eval_mathio("im(pi)").unwrap(), "0");
        assert_eq!(eval_mathio("re(i)").unwrap(), "0");
        assert!(eval_lineio("arg(2)").unwrap().starts_with("0"));
        assert!(
            eval_lineio("arg(-1)")
                .unwrap()
                .starts_with("3.141592653589793238")
        );
        assert!(eval_lineio("arg(0)").unwrap_err().contains("辐角"));
        // 复数路径不受影响
        assert_eq!(eval_mathio("re(2+3i)").unwrap(), "2");
        assert_eq!(eval_mathio("im(2+3i)").unwrap(), "3");
        assert_eq!(eval_mathio("conj(2-3i)").unwrap(), "2 + 3i");
        assert!(eval_lineio("floor(i)").unwrap_err().contains("复数不支持"));
    }

    #[test]
    fn inverse_hyperbolic_zeros_are_exact() {
        // README：arcsinh(0)、arccosh(1)、arctanh(0) 这类零点给**精确**的 0（前缀是 `=` 而不是 `≈`）
        assert_eq!(eval_mathio("arcsinh(0)").unwrap(), "0");
        assert_eq!(eval_mathio("arccosh(1)").unwrap(), "0");
        assert_eq!(eval_mathio("arctanh(0)").unwrap(), "0");
        // 非零点仍走数值
        assert!(
            eval_lineio("arccosh(2)")
                .unwrap()
                .starts_with("1.3169578969248167086")
        );
        assert!(
            eval_lineio("arctanh(1/2)")
                .unwrap()
                .starts_with("0.5493061443340548457")
        );
        // 定义域错误不能被零点短路吞掉
        assert!(eval_lineio("arccosh(1/2)").unwrap_err().contains("定义域"));
        assert!(eval_lineio("arctanh(1)").unwrap_err().contains("定义域"));
    }
}
