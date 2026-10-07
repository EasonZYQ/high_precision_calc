use crate::bigfloat;
use crate::number::{ExactExpr, ExactTerm, Number};
use crate::settings::{LATEX, RESULT_BASE};
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, Zero};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// 数学显示模式（mathio）：尽可能使用符号表示
pub fn format_mathio(num: &Number) -> String {
    let (scaled, suffix) = unit_scaled(num);
    format!("{}{}", format_mathio_raw(&scaled), suffix)
}

fn format_mathio_raw(num: &Number) -> String {
    // 矩阵：逐元素用**同一个 raw 格式化器**（避免重复套单位/缩放）
    if let Number::Matrix(m) = num {
        return crate::matrix::render(m, &format_mathio_raw);
    }
    if let Some(s) = format_radix(num) {
        return s;
    }
    // LaTeX 模式下精确式走平行渲染器（不套千分位：LaTeX 里不需要分隔符）
    if latex()
        && let Number::Exact(expr) = num
    {
        return format_exact_expr_latex(expr);
    }
    match num {
        // 千分位只对"纯十进制"结果生效（分数/根式/含 π 的符号串原样返回）
        Number::Exact(expr) => bigfloat::group_integer_part(&format_exact_expr(expr)),
        Number::Approx(f) => {
            let s = f.to_significant_string(bigfloat::display_digits());
            if s == "-0" { "0".to_string() } else { s }
        }
        Number::Complex(z) => format_complex(z, &format_mathio_raw),
        Number::Matrix(m) => crate::matrix::render(m, &format_mathio_raw),
    }
}

/// 复数显示：`a + bi` / `a - bi` / `bi` / `i` / `-i`（实部为 0 省略，虚部为 ±1 省略系数；
/// 分数/根式这类含空格或除号的虚部加括号，避免 `1 / 2i` 的歧义）。
/// `part` 是各分量自己的格式化函数（MathIO 走符号、LineIO 走小数）。
fn format_complex(z: &crate::complex::ComplexNum, part: &dyn Fn(&Number) -> String) -> String {
    if z.is_real() {
        return part(&z.re);
    }
    let im_neg = z.im.is_negative();
    let im_abs = if im_neg { z.im.neg() } else { z.im.clone() };
    // 虚部为 ±1 时省略系数（精确的 1 与"数值上恰好显示为 1"都算）
    let im_body = {
        let s = part(&im_abs);
        if s == "1" {
            "i".to_string()
        } else if s.contains(' ') || s.contains('/') {
            format!("({})i", s)
        } else {
            format!("{}i", s)
        }
    };
    let negative_body = format!("-{}", im_body);
    let im_str = if im_neg {
        negative_body.clone()
    } else {
        im_body.clone()
    };
    if z.re.is_zero() {
        return im_str;
    }
    format!(
        "{} {} {}",
        part(&z.re),
        if im_neg { "-" } else { "+" },
        im_body
    )
}

/// 线性显示模式（lineio）：一律使用小数
/// 结果单位（`/unit km`）：`(标签, 折成 SI 的系数)`。
///
/// 与数制开关同层：**只影响显示**，不动数值本身。做法是把结果**除以**系数再格式化，
/// 于是 `3000` 在 `/unit km` 下显示成 `3 km`（精确值相除仍是精确值）。
/// 单字母单位不收（`m`/`s`/`g`… 与变量命名空间冲突），所以标签一律是多字母的。
/// 设置结果单位；传 `None` 关闭
pub fn set_unit(u: Option<(String, BigRational)>) {
    if let Ok(mut g) = crate::settings::UNIT.lock() {
        *g = u;
    }
}

fn unit_scaled(num: &Number) -> (Number, String) {
    let g = match crate::settings::UNIT.lock() {
        Ok(g) => g,
        Err(_) => return (num.clone(), String::new()),
    };
    match g.as_ref() {
        Some((label, factor)) if !factor.is_zero() => {
            let f = Number::from_rational(factor.clone());
            (Number::div(num, &f), format!(" {label}"))
        }
        _ => (num.clone(), String::new()),
    }
}

/// LaTeX 输出开关（`/mode latex`）。
///
/// 用**显示层开关**而不是给 `DisplayMode` 加枚举分支：`DisplayMode::` 在全仓有 87 处引用，
/// 加一个变体会牵动一大片 match；而 LaTeX 只影响"结果怎么排"，本质是显示层的事。

/// 开关 LaTeX 输出（仅影响结果的符号渲染，数值不变）
pub fn set_latex(on: bool) {
    LATEX.store(on, Ordering::Relaxed);
}

/// 当前是否 LaTeX 输出
pub fn latex() -> bool {
    LATEX.load(Ordering::Relaxed)
}

/// 结果数制（10 = 十进制；16/8/2 = 十六/八/二进制）。
///
/// 与 `bigfloat::display_digits()` 同层：**显示层**的全局开关，不动数值本身。
/// 只对**整数**生效 —— "0.5 的十六进制"没有标准答案，所以小数与根式一律仍按十进制输出。

/// 设置结果数制（只接受 2 / 8 / 10 / 16，其余忽略）
pub fn set_base(base: u32) {
    if matches!(base, 2 | 8 | 10 | 16) {
        RESULT_BASE.store(base, Ordering::Relaxed);
    }
}

/// 当前结果数制
pub fn base() -> u32 {
    RESULT_BASE.load(Ordering::Relaxed)
}

/// 非十进制时把**整数**渲染成带前缀的形式（`0xFF` / `0o17` / `0b1010`）。
/// 带前缀是为了能直接粘回计算器（输入侧 `0x`/`0o`/`0b` 已支持）。
/// 返回 `None` 表示"这一项不归数制管"（十进制模式，或不是整数）。
fn format_radix(num: &Number) -> Option<String> {
    let b = base();
    if b == 10 {
        return None;
    }
    // 只处理整数：as_rational 是 Number 上的现成接口（与 parser::as_int 同一套）
    let n = match num.as_rational() {
        Some(r) if r.is_integer() => r.to_integer(),
        _ => return None,
    };
    Some(radix_string(&n, b))
}

/// 把整数渲染成**带前缀**的目标进制串（纯函数，便于单测且不依赖全局开关）。
/// 带前缀是为了能直接粘回计算器（输入侧 `0x`/`0o`/`0b` 已支持）。
fn radix_string(n: &BigInt, base: u32) -> String {
    let digits = n.to_str_radix(base);
    let prefix = match base {
        16 => "0x",
        8 => "0o",
        _ => "0b",
    };
    match digits.strip_prefix('-') {
        Some(rest) => format!("-{prefix}{rest}"),
        None => format!("{prefix}{digits}"),
    }
}

pub fn format_lineio(num: &Number) -> String {
    let (scaled, suffix) = unit_scaled(num);
    format!("{}{}", format_lineio_raw(&scaled), suffix)
}

fn format_lineio_raw(num: &Number) -> String {
    if let Number::Matrix(m) = num {
        return crate::matrix::render(m, &format_lineio_raw);
    }
    if let Number::Complex(z) = num {
        return format_complex(z, &format_lineio_raw);
    }
    if let Some(s) = format_radix(num) {
        return s;
    }
    let bf = num.to_approx();
    let s = bf.to_significant_string(bigfloat::display_digits());
    if s == "-0" { "0".to_string() } else { s }
}

/// 小数格式（用于 sd 函数在 mathio 模式下的输出）
pub fn format_decimal(num: &Number) -> String {
    // 这个入口是 sd() 之类的内部显示，**不套单位**
    format_decimal_raw(num)
}

fn format_decimal_raw(num: &Number) -> String {
    if let Number::Complex(z) = num {
        return format_complex(z, &format_decimal);
    }
    let bf = num.to_approx();
    let s = bf.to_significant_string(bigfloat::display_digits());
    if s == "-0" { "0".to_string() } else { s }
}

/// 格式化精确表达式
fn format_exact_expr(expr: &ExactExpr) -> String {
    if expr.terms.is_empty() {
        return "0".to_string();
    }

    // 检查是否所有项系数为零
    let all_zero = expr.terms.iter().all(|t| match t {
        ExactTerm::Rational(r) => r.is_zero(),
        ExactTerm::Sqrt(c, _) => c.is_zero(),
        ExactTerm::PiPow(c, _) => c.is_zero(),
        ExactTerm::PiPow(c, k) if k.is_one() => c.is_zero(),
        ExactTerm::E(c) => c.is_zero(),
    });
    if all_zero {
        return "0".to_string();
    }

    let mut numerator_str = String::new();

    for term in expr.terms.iter() {
        if is_term_zero(term) {
            continue;
        }
        let term_str = format_term(term);
        if term_str.is_empty() {
            continue;
        }

        if numerator_str.is_empty() {
            numerator_str = term_str;
        } else if let Some(rest) = term_str.strip_prefix('-') {
            numerator_str.push_str(&format!(" - {rest}"));
        } else {
            numerator_str.push_str(&format!(" + {}", term_str));
        }
    }

    if numerator_str.is_empty() {
        return "0".to_string();
    }

    if expr.denominator == BigInt::one() {
        numerator_str
    } else {
        // 确保分子不带括号也清晰
        if expr.terms.len() > 1 {
            format!("({}) / {}", numerator_str, expr.denominator)
        } else {
            format!("{} / {}", numerator_str, expr.denominator)
        }
    }
}

/// LaTeX 版精确式渲染。
///
/// 与 `format_exact_expr` **平行实现**而不是加参数：`ExactTerm` 只有 4 个变体，
/// 重复这几行比为 `DisplayMode` 加分支（87 处引用）划算得多，也不会让原路径承担新分支的风险。
fn format_exact_expr_latex(expr: &ExactExpr) -> String {
    if expr.terms.is_empty() {
        return "0".to_string();
    }
    let mut written = 0usize;
    let mut out = String::new();
    for term in &expr.terms {
        if is_term_zero(term) {
            continue;
        }
        let t = format_term_latex(term);
        if t.is_empty() {
            continue;
        }
        if written == 0 {
            out = t;
        } else if let Some(rest) = t.strip_prefix('-') {
            out.push_str(&format!(" - {rest}"));
        } else {
            out.push_str(&format!(" + {t}"));
        }
        written += 1;
    }
    if written == 0 {
        return "0".to_string();
    }
    if expr.denominator == BigInt::one() {
        out
    } else {
        format!("\\frac{{{out}}}{{{}}}", expr.denominator)
    }
}

/// 有理数的 LaTeX：整数原样，分数用 `\frac`，负号提到分式外面（`-\frac{1}{2}` 比 `\frac{-1}{2}` 好看）
fn latex_rational(r: &BigRational) -> String {
    if r.denom() == &BigInt::one() {
        return r.numer().to_string();
    }
    if r.numer().is_negative() {
        format!("-\\frac{{{}}}{{{}}}", -r.numer(), r.denom())
    } else {
        format!("\\frac{{{}}}{{{}}}", r.numer(), r.denom())
    }
}

/// 系数（有理数）接在符号量前：系数为 1 省略、为 -1 只留负号、分数系数走 `\frac`
fn latex_coeff(r: &BigRational) -> String {
    if r.is_one() {
        String::new()
    } else if r == &BigRational::from_integer(BigInt::from(-1)) {
        "-".to_string()
    } else {
        latex_rational(r)
    }
}

fn format_term_latex(term: &ExactTerm) -> String {
    match term {
        ExactTerm::Rational(r) => latex_rational(r),
        ExactTerm::Sqrt(c, radicand) => {
            let rad = format!("\\sqrt{{{radicand}}}");
            let c = latex_coeff(c);
            // 负系数要把负号留在最前面（调用方靠 "-" 前缀决定用 + 还是 - 连接）
            if c.is_empty() {
                rad
            } else if c == "-" {
                format!("-{rad}")
            } else {
                format!("{c}{rad}")
            }
        }
        ExactTerm::PiPow(c, k) if k.is_one() => {
            format!("{} \\pi", latex_coeff(c)).trim_start().to_string()
        }
        ExactTerm::PiPow(c, k) => latex_pipow(c, k),
        ExactTerm::E(c) => format!("{}e", latex_coeff(c)),
    }
}

fn format_term(term: &ExactTerm) -> String {
    match term {
        ExactTerm::Rational(r) => format_rational(r),
        ExactTerm::Sqrt(coeff, rad) => format_sqrt_term(coeff, rad),
        ExactTerm::PiPow(coeff, k) if k.is_one() => format_pi_term(coeff),
        ExactTerm::PiPow(coeff, k) => format_pipow_term(coeff, k),
        ExactTerm::E(coeff) => format_e_term(coeff),
    }
}

fn format_rational(r: &BigRational) -> String {
    if r.is_integer() {
        r.to_integer().to_string()
    } else if r.denom() == &BigInt::from(1) {
        r.numer().to_string()
    } else {
        format!("{} / {}", r.numer(), r.denom())
    }
}

/// c*π^k：k=1/2 写成 sqrt(pi)（沿用既有风格），k=1 是 pi，其余写 pi^k
fn format_pipow_term(coeff: &BigRational, k: &BigRational) -> String {
    if coeff.is_zero() {
        return String::new();
    }
    let half = BigRational::new(BigInt::from(1), BigInt::from(2));
    let base = if k == &half {
        "sqrt(pi)".to_string()
    } else if k.is_one() {
        "pi".to_string()
    } else {
        format!("pi^{}", format_rational(k))
    };
    if coeff.is_one() {
        return base;
    }
    if coeff == &BigRational::from_integer(BigInt::from(-1)) {
        return format!("-{base}");
    }
    format!("{}*{base}", format_rational(coeff))
}

/// LaTeX 版：k=1/2 走 \\sqrt{\\pi}，其余走 \\pi^{k}
fn latex_pipow(c: &BigRational, k: &BigRational) -> String {
    let half = BigRational::new(BigInt::from(1), BigInt::from(2));
    let base = if k == &half {
        "\\sqrt{\\pi}".to_string()
    } else if k.is_one() {
        "\\pi".to_string()
    } else {
        format!("\\pi^{{{}}}", format_rational(k))
    };
    format!("{} {base}", latex_coeff(c))
        .trim_start()
        .to_string()
}

fn format_sqrt_term(coeff: &BigRational, rad: &BigInt) -> String {
    if coeff.is_zero() {
        return String::new();
    }

    let coeff_str =
        if coeff.is_integer() && *coeff.numer() == BigInt::from(1) && !coeff.is_negative() {
            String::new()
        } else if coeff.is_integer() && *coeff.numer() == BigInt::from(-1) {
            "-".to_string()
        } else if coeff.is_integer() {
            coeff.to_integer().to_string()
        } else {
            format_rational(coeff)
        };

    if *rad == BigInt::one() {
        if coeff_str.is_empty() {
            "1".to_string()
        } else {
            coeff_str
        }
    } else if coeff_str.is_empty() {
        format!("sqrt({})", rad)
    } else if coeff_str == "-" {
        format!("-sqrt({})", rad)
    } else if coeff.is_integer() {
        format!("{}*sqrt({})", coeff_str, rad)
    } else {
        // 分数系数必须加括号：`1 / 2*sqrt(2)` 会被读成 1/(2√2)（旧实现的实际输出）
        format!("({})*sqrt({})", coeff_str, rad)
    }
}

fn format_pi_term(coeff: &BigRational) -> String {
    if coeff.is_zero() {
        return String::new();
    }
    if coeff.is_integer() {
        let n = coeff.to_integer();
        if n == BigInt::from(1) {
            return "pi".to_string();
        }
        if n == BigInt::from(-1) {
            return "-pi".to_string();
        }
        return format!("{}*pi", n);
    }
    // 分数倍 pi：尽量用 pi/分母 或 (分子/分母)*pi 的形式
    let num = coeff.numer().abs();
    let den = coeff.denom();
    let sign = if coeff.is_negative() { "-" } else { "" };
    if num == BigInt::from(1) {
        format!("{}pi / {}", sign, den)
    } else {
        format!("{}({} / {})*pi", sign, num, den)
    }
}

fn format_e_term(coeff: &BigRational) -> String {
    if coeff.is_zero() {
        return String::new();
    }
    if coeff.is_integer() && *coeff.numer() == BigInt::from(1) {
        "e".to_string()
    } else if coeff.is_integer() && *coeff.numer() == BigInt::from(-1) {
        "-e".to_string()
    } else if coeff.is_integer() {
        format!("{}*e", coeff.to_integer())
    } else {
        format!("({})*e", format_rational(coeff))
    }
}

fn is_term_zero(term: &ExactTerm) -> bool {
    match term {
        ExactTerm::Rational(r) => r.is_zero(),
        ExactTerm::Sqrt(c, _) => c.is_zero(),
        ExactTerm::PiPow(c, _) => c.is_zero(),
        ExactTerm::PiPow(c, k) if k.is_one() => c.is_zero(),
        ExactTerm::E(c) => c.is_zero(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 只测**纯函数**：数制开关是全局状态，测试若去改它会污染并行跑的其它测试
    /// （这个坑在本项目已经踩过一次，见 change_logs/change_log40.md 附近的记录）。
    /// 开关本身的端到端行为靠命令行冒烟核对。
    /// LaTeX 渲染：只测私有纯函数（开关是全局态，改它会污染并行测试，见上面注释）
    #[test]
    fn latex_renders_fractions_roots_and_constants() {
        let r = |n: i64, d: i64| BigRational::new(BigInt::from(n), BigInt::from(d));
        let one = || BigInt::one();
        let term = |t: ExactTerm| ExactExpr {
            terms: vec![t],
            denominator: one(),
        };
        // 1/3 → \frac{1}{3}
        assert_eq!(
            format_exact_expr_latex(&term(ExactTerm::Rational(r(1, 3)))),
            "\\frac{1}{3}"
        );
        // 整数不带分式
        assert_eq!(
            format_exact_expr_latex(&term(ExactTerm::Rational(r(7, 1)))),
            "7"
        );
        // 负分数：负号提到分式外
        assert_eq!(
            format_exact_expr_latex(&term(ExactTerm::Rational(r(-1, 2)))),
            "-\\frac{1}{2}"
        );
        // 3√2 / √2（系数 1 省略）/ -√2
        assert_eq!(
            format_exact_expr_latex(&term(ExactTerm::Sqrt(r(3, 1), BigInt::from(2)))),
            "3\\sqrt{2}"
        );
        assert_eq!(
            format_exact_expr_latex(&term(ExactTerm::Sqrt(r(1, 1), BigInt::from(2)))),
            "\\sqrt{2}"
        );
        assert_eq!(
            format_exact_expr_latex(&term(ExactTerm::Sqrt(r(-1, 1), BigInt::from(2)))),
            "-\\sqrt{2}"
        );
        // π 与 e
        assert_eq!(
            format_exact_expr_latex(&term(ExactTerm::PiPow(r(1, 1), BigRational::one()))),
            "\\pi"
        );
        assert_eq!(
            format_exact_expr_latex(&term(ExactTerm::PiPow(r(2, 1), BigRational::one()))),
            "2 \\pi"
        );
        assert_eq!(format_exact_expr_latex(&term(ExactTerm::E(r(1, 1)))), "e");
        // 多项相加 + 整体分母：2 + 3√2 再除以 3
        let multi = ExactExpr {
            terms: vec![
                ExactTerm::Rational(r(2, 1)),
                ExactTerm::Sqrt(r(3, 1), BigInt::from(2)),
            ],
            denominator: BigInt::from(3),
        };
        assert_eq!(format_exact_expr_latex(&multi), "\\frac{2 + 3\\sqrt{2}}{3}");
    }

    #[test]
    fn radix_string_is_prefixed_and_keeps_sign() {
        let n = |v: i64| BigInt::from(v);
        assert_eq!(radix_string(&n(255), 16), "0xff");
        assert_eq!(radix_string(&n(4095), 16), "0xfff");
        assert_eq!(radix_string(&n(-255), 16), "-0xff");
        assert_eq!(radix_string(&n(511), 8), "0o777");
        assert_eq!(radix_string(&n(10), 2), "0b1010");
        assert_eq!(radix_string(&n(0), 2), "0b0");
        assert_eq!(radix_string(&n(0), 16), "0x0");
    }
}
