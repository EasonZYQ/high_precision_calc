use crate::bigfloat;
use crate::number::{ExactExpr, ExactTerm, Number};
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, Zero};
use std::sync::atomic::{AtomicU32, Ordering};

/// 数学显示模式（mathio）：尽可能使用符号表示
pub fn format_mathio(num: &Number) -> String {
    if let Some(s) = format_radix(num) {
        return s;
    }
    match num {
        // 千分位只对"纯十进制"结果生效（分数/根式/含 π 的符号串原样返回）
        Number::Exact(expr) => bigfloat::group_integer_part(&format_exact_expr(expr)),
        Number::Approx(f) => {
            let s = f.to_significant_string(bigfloat::display_digits());
            if s == "-0" { "0".to_string() } else { s }
        }
        Number::Complex(z) => format_complex(z, &format_mathio),
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
/// 结果数制（10 = 十进制；16/8/2 = 十六/八/二进制）。
///
/// 与 `bigfloat::display_digits()` 同层：**显示层**的全局开关，不动数值本身。
/// 只对**整数**生效 —— "0.5 的十六进制"没有标准答案，所以小数与根式一律仍按十进制输出。
static RESULT_BASE: AtomicU32 = AtomicU32::new(10);

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
    if let Number::Complex(z) = num {
        return format_complex(z, &format_lineio);
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
        ExactTerm::Pi(c) => c.is_zero(),
        ExactTerm::E(c) => c.is_zero(),
    });
    if all_zero {
        return "0".to_string();
    }

    let mut numerator_str = String::new();

    for (_i, term) in expr.terms.iter().enumerate() {
        if is_term_zero(term) {
            continue;
        }
        let term_str = format_term(term);
        if term_str.is_empty() {
            continue;
        }

        if numerator_str.is_empty() {
            numerator_str = term_str;
        } else if term_str.starts_with('-') {
            numerator_str.push_str(&format!(" - {}", &term_str[1..]));
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

fn format_term(term: &ExactTerm) -> String {
    match term {
        ExactTerm::Rational(r) => format_rational(r),
        ExactTerm::Sqrt(coeff, rad) => format_sqrt_term(coeff, rad),
        ExactTerm::Pi(coeff) => format_pi_term(coeff),
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
        ExactTerm::Pi(c) => c.is_zero(),
        ExactTerm::E(c) => c.is_zero(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 只测**纯函数**：数制开关是全局状态，测试若去改它会污染并行跑的其它测试
    /// （这个坑在本项目已经踩过一次，见 change_logs/change_log40.md 附近的记录）。
    /// 开关本身的端到端行为靠命令行冒烟核对。
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
