//! Taylor / Maclaurin 展开（`taylor(f, x, a, n)`）。
//!
//! 系数 `c_k = f⁽ᵏ⁾(a) / k!`，逐阶求导后在 `a` 处取值。`n` 是**最高次数**（不是项数），
//! `a = 0` 即麦克劳林展开。**不写余项** `O(x^n)`（写了就没法继续参与运算），
//! 输出是 n 阶泰勒**多项式**。
//!
//! # 关键一步：把"近似系数"吸附回精确有理数
//!
//! 求值器对 `cos(0)`、`sin(0)`、`exp(0)` 这类值返回的是 **`Approx`**（幂级数算出来的），
//! 直接用会导致 `taylor(sin(x), x, 0, 5)` 输出 `1.0000000000000000000*x - 0.1666…*x^3 + …`
//! —— 把整件事的意义毁掉了。所以每个系数都要过一遍 [`snap_exact`]：
//! 把近似值按十进制展开还原成有理数并约分，**分母足够小才认作精确值**（否则保持近似）。
//! 这样 `cos(0)`→`1`、`sin(0)`→`0`、`cos(π/2)`→`0` 都能还原，而 `cos(1)` 这种真无理值仍是近似。

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::One;

use crate::parser::{BinOp, Expr, UnaryOp};
use hipercalc_core::number::Number;

use super::{
    ERROR_TAYLOR_ORDER, ERROR_TAYLOR_POINT, ERROR_TAYLOR_SINGULAR, ERROR_TAYLOR_TOO_LARGE,
    ERROR_TOO_MANY_TERMS, max_taylor_degree,
};

/// 吸附时允许的最大分母：超过就认为是真无理/超越值，保持近似
const SNAP_DEN_MAX: i64 = 1_000_000;

pub fn taylor(
    ev: &crate::parser::Evaluator,
    f: &Expr,
    var: &str,
    a_expr: &Expr,
    n: usize,
) -> Result<Expr, String> {
    if super::has_free_var_named(a_expr, var) {
        return Err(ERROR_TAYLOR_POINT.replace("{0}", var));
    }
    let limit = max_taylor_degree();
    if n > limit {
        return Err(ERROR_TAYLOR_ORDER.replace("{0}", &limit.to_string()));
    }
    let a = ev
        .evaluate_with_vars(a_expr, &[])
        .map_err(|_| ERROR_TAYLOR_POINT.replace("{0}", var))?;
    if a.is_complex() {
        return Err(ERROR_TAYLOR_POINT.replace("{0}", var));
    }
    let a = snap_exact(&a);

    let mut terms: Vec<(Number, usize)> = Vec::new();
    let mut deriv = f.clone();
    let mut kfact = Number::from_int(1);
    for k in 0..=n {
        let value = ev
            .evaluate_with_var(&deriv, var, &a)
            .map_err(|_| ERROR_TAYLOR_SINGULAR.to_string())?;
        if value.is_complex() {
            return Err(ERROR_TAYLOR_SINGULAR.to_string());
        }
        let value = snap_exact(&value);
        let coeff = hipercalc_core::number::Number::div(&value, &kfact);
        if !coeff.is_zero() {
            terms.push((coeff, k));
        }
        if k == n {
            break;
        }
        deriv = match super::diff::diff(ev, &deriv, var) {
            Ok(d) => d,
            Err(e)
                if e == ERROR_TOO_MANY_TERMS
                    || e.contains("规模过大")
                    || e.contains("嵌套过深") =>
            {
                // 高阶导数的表达式会指数膨胀：给泰勒专用文案，不要复用"求导结果规模过大"
                return Err(ERROR_TAYLOR_TOO_LARGE.to_string());
            }
            Err(e) => return Err(e),
        };
        kfact = hipercalc_core::number::Number::mul(&kfact, &Number::from_int((k + 1) as i64));
    }
    if terms.is_empty() {
        return Ok(Expr::Number(Number::from_int(0)));
    }
    // 按**升幂**顺序拼接：泰勒展开的惯例是 `x - x^3/6 + x^5/120`，
    // 而 normalize 的排序是"次数降序"（多项式/因式分解的惯例：`x^2 + 3*x + 2`）。
    // 所以这里不走整体化简 —— 各项本身就是规范形式（系数 × 幂），不需要再合并。
    let mut acc: Option<Expr> = None;
    for (coeff, k) in terms {
        let negative = coeff.is_negative();
        let abs_coeff = if negative { coeff.neg() } else { coeff };
        let body = term(&abs_coeff, &a, var, k);
        acc = Some(match acc {
            None => {
                if negative {
                    Expr::Unary(UnaryOp::Neg, Box::new(body))
                } else {
                    body
                }
            }
            Some(prev) => Expr::Binary(
                Box::new(prev),
                if negative { BinOp::Sub } else { BinOp::Add },
                Box::new(body),
            ),
        });
    }
    let _ = ev;
    Ok(acc.unwrap())
}

/// 构造单项 `c·(x − a)^k`（`a = 0` 且 `k = 1` 时退化成 `c·x`）
fn term(coeff: &Number, a: &Number, var: &str, k: usize) -> Expr {
    if k == 0 {
        // 常数项就是系数本身（不要写成 `(x - 1)^0`）
        return Expr::Number(coeff.clone());
    }
    let x = Expr::Variable(var.to_string());
    let base = if a.is_zero() {
        x
    } else {
        Expr::Binary(Box::new(x), BinOp::Sub, Box::new(Expr::Number(a.clone())))
    };
    let mut body = base;
    if k != 1 {
        body = Expr::Pow(
            Box::new(body),
            Box::new(Expr::Number(Number::from_int(k as i64))),
        );
    }
    if is_one(coeff) {
        body
    } else {
        Expr::Binary(
            Box::new(Expr::Number(coeff.clone())),
            BinOp::Mul,
            Box::new(body),
        )
    }
}

/// 近似值 → 精确有理数（分母足够小才认）。
///
/// 做法：取 40 位有效数字的十进制串，还原成 `p / 10^m` 再约分；分母 ≤ `SNAP_DEN_MAX` 才采用。
/// 科学计数法串（大数）直接放弃。
pub fn snap_exact(v: &Number) -> Number {
    if !matches!(v, Number::Approx(_)) {
        return v.clone();
    }
    let s = v.to_approx().to_significant_string(40);
    let Some(r) = decimal_to_rational(&s) else {
        return v.clone();
    };
    if r.denom() > &BigInt::from(SNAP_DEN_MAX) {
        return v.clone();
    }
    // 用近似值再确认一次（防止还原出错）
    let back = Number::from_rational(r);
    let d = hipercalc_core::bigfloat::BigFloat::sub(
        &back.to_approx(),
        &v.to_approx(),
        hipercalc_core::bigfloat::precision(),
    );
    if !d.is_zero() && d.magnitude_log10() > -((hipercalc_core::bigfloat::precision() / 2) as f64) {
        return v.clone();
    }
    back
}

/// `"1.25"` / `"-0.5"` → 精确有理数；含 `×`（科学计数法）或非法字符返回 None
fn decimal_to_rational(s: &str) -> Option<BigRational> {
    let s = s.trim();
    if s.contains('×') || s.contains('e') || s.contains('E') {
        return None;
    }
    let (neg, body) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let (int_part, frac_part) = match body.split_once('.') {
        Some((i, f)) => (i, f),
        None => (body, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    if !int_part.chars().all(|c| c.is_ascii_digit())
        || !frac_part.chars().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let digits = format!("{}{}", int_part, frac_part);
    let numer: BigInt = digits.parse().ok()?;
    let denom = BigInt::from(10u8).pow(frac_part.len() as u32);
    let mut r = BigRational::new(numer, denom);
    if neg {
        r = -r;
    }
    Some(r)
}

fn is_one(n: &Number) -> bool {
    n.as_rational().map(|r| r.is_one()).unwrap_or(false)
}

/// 注册表用的元数（仅测试用于一致性断言）
#[cfg(test)]
pub fn arity() -> &'static [usize] {
    super::CALCULUS_ARITIES
        .iter()
        .find(|(n, _)| *n == "taylor")
        .map(|(_, a)| *a)
        .unwrap_or(&[])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{DisplayMode, Evaluator, Parser};

    fn parse(s: &str) -> Expr {
        let mut p = Parser::new(s);
        p.parse_expression().unwrap()
    }

    fn t(input: &str, n: usize) -> String {
        let ev = Evaluator::new();
        let r = taylor(
            &ev,
            &parse(input),
            "x",
            &Expr::Number(Number::from_int(0)),
            n,
        )
        .unwrap();
        super::super::render::render_expr(&r, DisplayMode::MathIO)
    }

    fn t_at(input: &str, a: i64, n: usize) -> String {
        let ev = Evaluator::new();
        let r = taylor(
            &ev,
            &parse(input),
            "x",
            &Expr::Number(Number::from_int(a)),
            n,
        )
        .unwrap();
        super::super::render::render_expr(&r, DisplayMode::MathIO)
    }

    #[test]
    fn maclaurin_exact_coefficients() {
        // 系数必须是精确有理数（不是 1.0000000000…*x）
        assert_eq!(t("sin(x)", 5), "x - 1 / 6 * x^3 + 1 / 120 * x^5");
        assert_eq!(t("cos(x)", 4), "1 - 1 / 2 * x^2 + 1 / 24 * x^4");
        assert_eq!(
            t("exp(x)", 4),
            "1 + x + 1 / 2 * x^2 + 1 / 6 * x^3 + 1 / 24 * x^4"
        );
        assert_eq!(t("1/(1-x)", 3), "1 + x + x^2 + x^3");
        assert_eq!(t("x^3+2*x", 4), "2*x + x^3");
    }

    #[test]
    fn expansion_at_nonzero_point() {
        // taylor(exp(x), x, 1, 2) = e + e(x-1) + e/2 (x-1)^2
        // 注意 e 本身没有"精确的有理表示"，所以系数是近似值（前缀会是 ≈，这是诚实的）
        let s = t_at("exp(x)", 1, 2);
        assert!(s.contains("(x - 1)"), "{s}");
        assert!(s.starts_with("2.7182818284590452354"), "{s}");
        // 但多项式在整点展开是精确的
        assert_eq!(t_at("x^2", 1, 2), "1 + 2 * (x - 1) + (x - 1)^2");
    }

    #[test]
    fn order_limit_and_singular_point() {
        let ev = Evaluator::new();
        let big = taylor(
            &ev,
            &parse("sin(x)"),
            "x",
            &Expr::Number(Number::from_int(0)),
            max_taylor_degree() + 1,
        );
        assert!(big.is_err());
        assert!(big.unwrap_err().contains("上限"));
        // 1/x 在 0 处展开：导数在 0 处无定义
        let e = taylor(
            &ev,
            &parse("1/x"),
            "x",
            &Expr::Number(Number::from_int(0)),
            3,
        );
        assert!(e.is_err());
    }

    #[test]
    fn snap_only_for_small_denominators() {
        // cos(0) 的近似值应吸附成精确 1
        let ev = Evaluator::new();
        let v = ev
            .evaluate_with_var(&parse("cos(x)"), "x", &Number::from_int(0))
            .unwrap();
        assert!(matches!(v, Number::Approx(_)));
        let snapped = snap_exact(&v);
        assert_eq!(
            snapped
                .as_rational()
                .map(|r| r.to_string())
                .unwrap_or_default(),
            "1"
        );
        // cos(1) 是真无理值 ⇒ 保持近似
        let v2 = ev
            .evaluate_with_var(&parse("cos(x)"), "x", &Number::from_int(1))
            .unwrap();
        assert!(matches!(snap_exact(&v2), Number::Approx(_)));
    }

    #[test]
    fn no_blowup_on_repeated_differentiation() {
        // **回归测试**：`((1-x)^2)^2` 这类嵌套幂若不合并成 `(1-x)^4`，
        // 反复求导时项数会指数膨胀（曾让这条用例报"泰勒展开式过大"）；
        // 同理多顶底的幂不能展开、分子也不能被分配进分母。
        assert_eq!(t("1/(1-x)", 5), "1 + x + x^2 + x^3 + x^4 + x^5");
        assert_eq!(
            t("1/(1-x)", 10),
            "1 + x + x^2 + x^3 + x^4 + x^5 + x^6 + x^7 + x^8 + x^9 + x^10"
        );
        assert_eq!(t("1/(1+x)", 6), "1 - x + x^2 - x^3 + x^4 - x^5 + x^6");
    }

    #[test]
    fn arity_is_registered() {
        assert_eq!(arity(), &[4]);
    }
}
