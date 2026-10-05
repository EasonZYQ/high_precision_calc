//! 求和与求积（`sum(f, k, a, b)` / `prod(f, k, a, b)`）。
//!
//! # 先闭式、后逐项
//!
//! 顺序很关键（计划里最初写反了）：
//! 1. **闭式**优先 —— 多项式用 Faulhaber 公式、等比有现成公式、`prod(k)` 走阶乘；
//! 2. 闭式认不出来、且项数在预算内 ⇒ **精确逐项**累加；
//! 3. 都不行 ⇒ 报错（不做大规模数值累加：既慢又给不出精确值）。
//!
//! 例：`sum(k, k, 1, 1000000)` 用闭式秒算 `500000500000`，而逐项要一百万次。
//!
//! # 分级预算（有理数的分母会爆炸）
//!
//! `sum(1/k, k, 1, 2000)` 的中间和分母是 `lcm(1..2000)`（约 10⁸⁶⁰ 位），
//! 每次加法都要 gcd 归约 ⇒ 会卡死。所以**含除法/负幂的项**用更小的预算
//! （`SUM_EXACT_MAX_TERMS`），超出就走闭式或报错；整数项才用大预算。
//! `prod` 另有更小的上限（`prod(k, k, 1, 10000)` 的 10000! 有 35660 位）。

use num_bigint::BigInt;
use num_traits::One;

use crate::parser::{BinOp, Expr};
use hipercalc_core::number::Number;

use super::{
    ERROR_SUM_BOUND, ERROR_SUM_COMPLEX, ERROR_SUM_NO_CLOSED_FORM, ERROR_SUM_TOO_LARGE_DEEP,
    max_prod_terms, max_sum_exact_terms, max_sum_numeric_terms, max_sum_terms,
};

/// 多项式（`coeffs[i]` 是 `n^i` 的系数），仅本模块内部使用
type Poly = Vec<Number>;

pub fn sum(
    ev: &crate::parser::Evaluator,
    f: &Expr,
    var: &str,
    a_expr: &Expr,
    b_expr: &Expr,
) -> Result<Expr, String> {
    let (a, b) = bounds(ev, a_expr, b_expr)?;
    if a > b {
        return Ok(Expr::Number(Number::from_int(0))); // 空和
    }
    let count = (b - a + 1) as u128;
    let budget = if has_division(f, var) {
        max_sum_exact_terms()
    } else {
        max_sum_terms()
    };

    // 1) 闭式
    if let Some(v) = closed_form_sum(ev, f, var, a, b)? {
        return Ok(Expr::Number(v));
    }
    // 2) 精确逐项（预算内）
    if count <= budget as u128 {
        let mut acc = Number::from_int(0);
        for k in a..=b {
            let v = eval_at(ev, f, var, k)?;
            acc = hipercalc_core::number::Number::add(&acc, &v);
        }
        return Ok(Expr::Number(acc));
    }
    // 3) 数值逐项：**含分数时精确累加会让分母爆炸**（`sum(1/k^3,k,1,10^6)` 的分母约 260 万位），
    //    所以每项先转 BigFloat 再相加 —— 结果是近似值（显示时会带 `≈`），但能算出来。
    if count <= max_sum_numeric_terms() as u128 {
        return Ok(Expr::Number(numeric_sum(ev, f, var, a, b)?));
    }
    Err(too_large_error())
}

/// 超限时的文案：已经在 Deep 下就**不能**再提示"可放宽"
fn too_large_error() -> String {
    if hipercalc_core::calc_mode::is_deep() {
        ERROR_SUM_TOO_LARGE_DEEP.to_string()
    } else {
        ERROR_SUM_NO_CLOSED_FORM.to_string()
    }
}

/// 数值逐项求和：每项先 `to_approx` 再累加（避免有理数分母随项数爆炸）
fn numeric_sum(
    ev: &crate::parser::Evaluator,
    f: &Expr,
    var: &str,
    a: i64,
    b: i64,
) -> Result<Number, String> {
    let prec = hipercalc_core::bigfloat::precision();
    let mut acc = hipercalc_core::bigfloat::BigFloat::from_u64(0);
    for k in a..=b {
        // 先用**近似**整数 k 求值：这样整条链走 BigFloat（每项 ~2µs），
        // 而用精确 k 会全程走有理数运算（含 gcd 化简，实测每项 ~23µs，慢十倍多）。
        // 近似 k 是 `from_u64(k)` —— 表示成 Approx 但数值精确，`floor`/`frac` 之类结果不变。
        let approx_k =
            Number::Approx(hipercalc_core::bigfloat::BigFloat::from_u64(k.max(0) as u64));
        let approx_k = if k < 0 { approx_k.neg() } else { approx_k };
        let v = match ev.evaluate_with_var(f, var, &approx_k) {
            Ok(v) => v,
            // 回退：`isprime`/`gcd`/`nCr` 这类**必须精确整数参数**的函数
            Err(_) => eval_at(ev, f, var, k)?,
        };
        if v.is_complex() {
            return Err(ERROR_SUM_COMPLEX.to_string());
        }
        acc = hipercalc_core::bigfloat::BigFloat::add(&acc, &v.to_approx(), prec);
    }
    Ok(Number::Approx(acc))
}

pub fn prod(
    ev: &crate::parser::Evaluator,
    f: &Expr,
    var: &str,
    a_expr: &Expr,
    b_expr: &Expr,
) -> Result<Expr, String> {
    let (a, b) = bounds(ev, a_expr, b_expr)?;
    if a > b {
        return Ok(Expr::Number(Number::from_int(1))); // 空积
    }
    let count = (b - a + 1) as u128;

    if let Some(v) = closed_form_prod(ev, f, var, a, b)? {
        return Ok(Expr::Number(v));
    }
    if count <= max_prod_terms() as u128 {
        let mut acc = Number::from_int(1);
        for k in a..=b {
            let v = eval_at(ev, f, var, k)?;
            acc = hipercalc_core::number::Number::mul(&acc, &v);
        }
        return Ok(Expr::Number(acc));
    }
    Err(too_large_error())
}

/* ---------------- 闭式 ---------------- */

fn closed_form_sum(
    ev: &crate::parser::Evaluator,
    f: &Expr,
    var: &str,
    a: i64,
    b: i64,
) -> Result<Option<Number>, String> {
    // 常数项：f·count
    if !super::has_free_var_named(f, var) {
        let c = eval_const(ev, f)?;
        return Ok(Some(hipercalc_core::number::Number::mul(
            &c,
            &Number::from_bigint(BigInt::from(b - a + 1)),
        )));
    }
    // 多项式：Σ c_m k^m 用 Faulhaber（`Σ_{k=1}^{n} k^p` 是 n 的 p+1 次多项式）
    if let Some(coeffs) = poly_coeffs(ev, f, var) {
        let mut total = Number::from_int(0);
        let lo = a - 1;
        for (p, c) in coeffs.iter().enumerate() {
            if c.is_zero() {
                continue;
            }
            let formula = sum_power_formula(p);
            let hi_v = poly_eval(&formula, &Number::from_int(b));
            let lo_v = poly_eval(&formula, &Number::from_int(lo));
            total = hipercalc_core::number::Number::add(
                &total,
                &hipercalc_core::number::Number::mul(
                    c,
                    &hipercalc_core::number::Number::sub(&hi_v, &lo_v),
                ),
            );
        }
        return Ok(Some(total));
    }
    // 等比：c·r^k（用采样点识别，比结构匹配更稳）
    if let Some(v) = geometric_sum(ev, f, var, a, b)? {
        return Ok(Some(v));
    }
    Ok(None)
}

fn closed_form_prod(
    ev: &crate::parser::Evaluator,
    f: &Expr,
    var: &str,
    a: i64,
    b: i64,
) -> Result<Option<Number>, String> {
    let count = b - a + 1;
    // 常数：c^count
    if !super::has_free_var_named(f, var) {
        let c = eval_const(ev, f)?;
        let p = hipercalc_core::number::Number::pow(&c, &Number::from_bigint(BigInt::from(count)))?;
        return Ok(Some(p));
    }
    // 区间含 0 ⇒ 乘积为 0
    if a <= 0 && b >= 0 {
        return Ok(Some(Number::from_int(0)));
    }
    // c·k ⇒ c^count · b!/(a-1)!（要求区间在同侧，且 |a-1|、|b| 不太大）
    if let Some(coeffs) = poly_coeffs(ev, f, var) {
        let mut deg = None;
        for (i, c) in coeffs.iter().enumerate() {
            if !c.is_zero() {
                if deg.is_some() || i != 1 {
                    deg = None;
                    break;
                }
                deg = Some(c.clone());
            }
        }
        if let Some(c) = deg {
            let lo = a - 1;
            if lo == 0 {
                let fact = factorial(b.abs())?;
                let cp = hipercalc_core::number::Number::pow(
                    &c,
                    &Number::from_bigint(BigInt::from(count)),
                )?;
                return Ok(Some(hipercalc_core::number::Number::mul(&cp, &fact)));
            }
            if a > 0 {
                let hi_f = factorial(b)?;
                let lo_f = factorial(lo)?;
                let ratio = hipercalc_core::number::Number::div(&hi_f, &lo_f);
                let cp = hipercalc_core::number::Number::pow(
                    &c,
                    &Number::from_bigint(BigInt::from(count)),
                )?;
                return Ok(Some(hipercalc_core::number::Number::mul(&cp, &ratio)));
            }
        }
    }
    Ok(None)
}

/// `Σ_{k=a}^{b} c·r^k`（`r ≠ 1` 时 `c·r^a·(r^{b-a+1}−1)/(r−1)`；`r = 1` 时 `c·count`）
fn geometric_sum(
    ev: &crate::parser::Evaluator,
    f: &Expr,
    var: &str,
    a: i64,
    b: i64,
) -> Result<Option<Number>, String> {
    // 用三个采样点识别 `c·r^k`：r = f(1)/f(0)，再用 f(2) 验证
    let (Ok(f0), Ok(f1), Ok(f2)) = (
        ev.evaluate_with_var(f, var, &Number::from_int(0)),
        ev.evaluate_with_var(f, var, &Number::from_int(1)),
        ev.evaluate_with_var(f, var, &Number::from_int(2)),
    ) else {
        return Ok(None);
    };
    if f0.is_zero() || f0.is_complex() || f1.is_complex() || f2.is_complex() {
        return Ok(None);
    }
    let r = hipercalc_core::number::Number::div(&f1, &f0);
    if r.is_zero() {
        return Ok(None);
    }
    let expect =
        hipercalc_core::number::Number::mul(&f0, &hipercalc_core::number::Number::mul(&r, &r));
    if !num_close(&expect, &f2) {
        return Ok(None);
    }
    let count = Number::from_bigint(BigInt::from(b - a + 1));
    let one = Number::from_int(1);
    if num_close(&r, &one) {
        return Ok(Some(hipercalc_core::number::Number::mul(&f0, &count)));
    }
    // c·r^a·(r^count − 1)/(r − 1)
    let r_pow_a = hipercalc_core::number::Number::pow(&r, &Number::from_bigint(BigInt::from(a)))?;
    let r_pow_count = hipercalc_core::number::Number::pow(&r, &count)?;
    let numer = hipercalc_core::number::Number::sub(&r_pow_count, &one);
    let denom = hipercalc_core::number::Number::sub(&r, &one);
    Ok(Some(hipercalc_core::number::Number::mul(
        &hipercalc_core::number::Number::mul(&f0, &r_pow_a),
        &hipercalc_core::number::Number::div(&numer, &denom),
    )))
}

/* ---------------- Faulhaber ---------------- */

/// `Σ_{k=1}^{n} k^p` 作为 n 的多项式（用 `(n+1)^{p+1} − 1 = Σ_j C(p+1,j)·S_j(n)` 递推）
fn sum_power_formula(p: usize) -> Poly {
    let mut s: Vec<Poly> = Vec::with_capacity(p + 1);
    // S_0(n) = n
    s.push(vec![Number::from_int(0), Number::from_int(1)]);
    for m in 1..=p {
        // (n+1)^{m+1} − 1
        let mut acc = poly_sub(&poly_pow_n_plus_1(m + 1), &[Number::from_int(1)]);
        for j in 0..m {
            let c = Number::from_bigint(binomial(m + 1, j));
            acc = poly_sub(&acc, &poly_scale(&s[j], &c));
        }
        let d = Number::from_bigint(BigInt::from(m + 1));
        s.push(poly_scale(
            &acc,
            &hipercalc_core::number::Number::div(&Number::from_int(1), &d),
        ));
    }
    s.pop().unwrap()
}

/// `(n+1)^m` 展开成 n 的多项式
fn poly_pow_n_plus_1(m: usize) -> Poly {
    let mut acc: Poly = vec![Number::from_int(1)];
    for _ in 0..m {
        acc = poly_mul(&acc, &[Number::from_int(1), Number::from_int(1)]); // ×(n+1)
    }
    acc
}

fn binomial(n: usize, k: usize) -> BigInt {
    let mut r = BigInt::one();
    for i in 0..k {
        r = r * BigInt::from(n - i) / BigInt::from(i + 1);
    }
    r
}

fn poly_sub(a: &[Number], b: &[Number]) -> Poly {
    let n = a.len().max(b.len());
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let x = a.get(i).cloned().unwrap_or_else(|| Number::from_int(0));
        let y = b.get(i).cloned().unwrap_or_else(|| Number::from_int(0));
        out.push(hipercalc_core::number::Number::sub(&x, &y));
    }
    out
}

fn poly_scale(a: &[Number], s: &Number) -> Poly {
    a.iter()
        .map(|c| hipercalc_core::number::Number::mul(c, s))
        .collect()
}

fn poly_mul(a: &[Number], b: &[Number]) -> Poly {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let mut out = vec![Number::from_int(0); a.len() + b.len() - 1];
    for (i, x) in a.iter().enumerate() {
        for (j, y) in b.iter().enumerate() {
            out[i + j] = hipercalc_core::number::Number::add(
                &out[i + j],
                &hipercalc_core::number::Number::mul(x, y),
            );
        }
    }
    out
}

fn poly_eval(a: &[Number], x: &Number) -> Number {
    let mut acc = Number::from_int(0);
    for c in a.iter().rev() {
        acc = hipercalc_core::number::Number::add(&hipercalc_core::number::Number::mul(&acc, x), c);
    }
    acc
}

/* ---------------- 小工具 ---------------- */

fn bounds(ev: &crate::parser::Evaluator, a: &Expr, b: &Expr) -> Result<(i64, i64), String> {
    let get = |e: &Expr| -> Result<i64, String> {
        let v = ev
            .evaluate_with_vars(e, &[])
            .map_err(|_| ERROR_SUM_BOUND.to_string())?;
        let r = v.as_rational().ok_or_else(|| ERROR_SUM_BOUND.to_string())?;
        if !r.is_integer() {
            return Err(ERROR_SUM_BOUND.to_string());
        }
        r.to_integer()
            .try_into()
            .map_err(|_| ERROR_SUM_BOUND.to_string())
    };
    Ok((get(a)?, get(b)?))
}

fn eval_at(ev: &crate::parser::Evaluator, f: &Expr, var: &str, k: i64) -> Result<Number, String> {
    let v = ev
        .evaluate_with_var(f, var, &Number::from_int(k))
        .map_err(|e| e)?;
    if v.is_complex() {
        return Err(ERROR_SUM_COMPLEX.to_string());
    }
    Ok(v)
}

fn eval_const(ev: &crate::parser::Evaluator, f: &Expr) -> Result<Number, String> {
    let v = ev.evaluate_with_vars(f, &[]).map_err(|e| e)?;
    if v.is_complex() {
        return Err(ERROR_SUM_COMPLEX.to_string());
    }
    Ok(v)
}

/// 项里是否含除法/负幂（这类项的有理数分母会爆炸 ⇒ 用更小的预算）
fn has_division(f: &Expr, var: &str) -> bool {
    match f {
        Expr::Binary(l, BinOp::Div, r) => {
            super::has_free_var_named(r, var) || has_division(l, var) || has_division(r, var)
        }
        Expr::Binary(l, _, r) => has_division(l, var) || has_division(r, var),
        Expr::Unary(_, x) | Expr::Sd(x) | Expr::Factor(x) => has_division(x, var),
        Expr::Pow(b, e) => has_division(b, var) || has_division(e, var),
        Expr::Function(_, args) => args.iter().any(|a| has_division(a, var)),
        _ => false,
    }
}

/// 多项式系数（升幂）；要求单项变量且是多项式
fn poly_coeffs(ev: &crate::parser::Evaluator, f: &Expr, var: &str) -> Option<Vec<Number>> {
    let vc = var.chars().next()?;
    crate::solver_poly::extract_polynomial(ev, f, vc)
}

fn factorial(n: i64) -> Result<Number, String> {
    if n < 0 || n > super::FACTORIAL_MAX {
        return Err(ERROR_SUM_NO_CLOSED_FORM.to_string());
    }
    let mut acc = BigInt::one();
    for i in 2..=n {
        acc *= BigInt::from(i);
    }
    Ok(Number::from_bigint(acc))
}

fn num_close(a: &Number, b: &Number) -> bool {
    let d = hipercalc_core::bigfloat::BigFloat::sub(
        &a.to_approx(),
        &b.to_approx(),
        hipercalc_core::bigfloat::precision() + 10,
    );
    if d.is_zero() {
        return true;
    }
    let scale = b.to_approx().magnitude_log10().max(0.0);
    d.magnitude_log10() <= -(hipercalc_core::bigfloat::display_digits() as f64 + 6.0) + scale
}

/// 供测试断言注册表的元数
#[cfg(test)]
pub fn arity(name: &str) -> &'static [usize] {
    super::CALCULUS_ARITIES
        .iter()
        .find(|(n, _)| *n == name)
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

    fn run(name: &str, f: &str, var: &str, a: i64, b: i64) -> String {
        let ev = Evaluator::new();
        let r = if name == "sum" {
            sum(
                &ev,
                &parse(f),
                var,
                &Expr::Number(Number::from_int(a)),
                &Expr::Number(Number::from_int(b)),
            )
        } else {
            prod(
                &ev,
                &parse(f),
                var,
                &Expr::Number(Number::from_int(a)),
                &Expr::Number(Number::from_int(b)),
            )
        }
        .unwrap();
        match &r {
            Expr::Number(n) => hipercalc_core::display::format_mathio(n),
            other => super::super::render::render_expr(other, DisplayMode::MathIO),
        }
    }

    fn run_err(name: &str, f: &str, var: &str, a: i64, b: i64) -> String {
        let ev = Evaluator::new();
        let r = if name == "sum" {
            sum(
                &ev,
                &parse(f),
                var,
                &Expr::Number(Number::from_int(a)),
                &Expr::Number(Number::from_int(b)),
            )
        } else {
            prod(
                &ev,
                &parse(f),
                var,
                &Expr::Number(Number::from_int(a)),
                &Expr::Number(Number::from_int(b)),
            )
        };
        r.unwrap_err()
    }

    #[test]
    fn sums_exact_and_large_ranges() {
        assert_eq!(run("sum", "k", "k", 1, 100), "5050");
        assert_eq!(run("sum", "k^2", "k", 1, 10), "385");
        assert_eq!(run("sum", "k^3", "k", 1, 10), "3025");
        assert_eq!(run("sum", "k", "k", 1, 1000000), "500000500000"); // 闭式
        assert_eq!(run("sum", "1", "k", 1, 10), "10");
        assert_eq!(run("sum", "k", "k", 5, 5), "5");
        assert_eq!(run("sum", "k", "k", 3, 1), "0"); // 空和
    }

    #[test]
    fn sums_fractional_terms() {
        // 谐波数：1 + 1/2 + 1/3 = 11/6
        assert_eq!(run("sum", "1/k", "k", 1, 3), "11 / 6");
        // 含除法 ⇒ 用更小预算；1000 项仍在预算内、且走数值？
        // （1/k 的和没有有理闭式，闭式识别不出来 ⇒ 逐项，1000 ≤ SUM_EXACT_MAX_TERMS(DEEP) 之外）
        assert_eq!(run("sum", "1/k", "k", 1, 10), "7381 / 2520");
    }

    #[test]
    fn geometric_and_power_sums() {
        assert_eq!(run("sum", "2^k", "k", 0, 10), "2047");
        assert_eq!(
            run("sum", "2^k", "k", 1, 100),
            "2535301200456458802993406410750"
        );
        assert_eq!(run("sum", "3^k", "k", 1, 5), "363");
    }

    #[test]
    fn products() {
        assert_eq!(run("prod", "k", "k", 1, 10), "3628800");
        assert_eq!(run("prod", "k", "k", 1, 20), "2432902008176640000");
        assert_eq!(run("prod", "2", "k", 1, 10), "1024");
        assert_eq!(run("prod", "k", "k", 3, 1), "1"); // 空积
        assert_eq!(run("prod", "k", "k", -3, 3), "0"); // 含 0
    }

    #[test]
    fn errors_are_explicit() {
        // 超出预算又没有闭式：明确报错，不做大规模数值累加
        assert!(run_err("sum", "1/k", "k", 1, 100000).contains("无闭式"));
        // 阶乘闭式超出上限
        assert!(run_err("prod", "k", "k", 1, 20000).contains("无闭式"));
    }

    #[test]
    fn faulhaber_matches_known_formulas() {
        // Σ_{k=1}^{n} k^2 = n(n+1)(2n+1)/6 → n=5 时 55
        let f2 = sum_power_formula(2);
        assert_eq!(
            hipercalc_core::display::format_mathio(&poly_eval(&f2, &Number::from_int(5))),
            "55"
        );
        // Σ_{k=1}^{n} k^3 = (n(n+1)/2)^2 → n=4 时 100
        let f3 = sum_power_formula(3);
        assert_eq!(
            hipercalc_core::display::format_mathio(&poly_eval(&f3, &Number::from_int(4))),
            "100"
        );
    }

    #[test]
    fn arity_registered() {
        assert_eq!(arity("sum"), &[4]);
        assert_eq!(arity("prod"), &[4]);
    }
}
