//! 极限（`lim(f, x, a)`）。
//!
//! # 策略：结构优先 → 洛必达 → 数值双侧
//!
//! 1. **直接代入**：连续函数一步到位（`lim(x^2, x, 2) = 4`）；
//! 2. **结构分析**：
//!    - `a` 为常数且是 `0/0` ⇒ **洛必达**（分子分母分别求导后重估，最多 `LIMIT_LHOPITAL_MAX` 轮）；
//!    - `a` 为 ±inf 且分子分母都是多项式 ⇒ 按**次数**比较（高次胜 / 同次取首项系数比 / 低次为 0）；
//! 3. **数值双侧逼近**：`h ← h/10` 取两侧值，要求两侧一致且相邻步收缩。
//!
//! # 两个容易出错的点
//!
//! - **洛必达要能降级**：`diff` 遇到不支持的函数会报错（`floor`/`abs`…），必须**捕获后转数值路径**，
//!   不能把"函数不支持求导"直接抛给用户；
//! - **三角参数护栏**：`check_trig_range` 在 `|x| > 10^(precision-2)`（默认 10⁷⁸）时报"参数过大"，
//!   所以 `x → ±inf` 的探测只到 `10^LIMIT_INF_K_MAX`（30）。这样既不会撞护栏，
//!   也保证 `lim(sin(x), x, inf)` 报的是"**无法判定极限**"（振荡不收敛）而不是"三角函数参数过大"。
//!
//! # 与角度模式的关系
//!
//! 三角函数的求导本身受角度模式影响，所以 Deg 模式下 `lim(sin(x)/x, x, 0) = π/180` ——
//! 这与本机"Deg 模式下 `sin` 的导数带 π/180"的既有语义一致，不是 bug（文档已说明）。

use hipercalc_core::number::Number;
use crate::parser::{BinOp, Expr, UnaryOp};

use super::{
    max_limit_iters, ERROR_LIMIT_ONE_SIDED, ERROR_LIMIT_POINT, ERROR_LIMIT_UNDECIDED,
    LIMIT_INF_K_MAX, LIMIT_LHOPITAL_MAX,
};

/// 极限结果：有限值，或 ±∞
pub enum LimitValue {
    Finite(Number),
    Infinity(i32),
}

impl LimitValue {
    /// 转成可参与运算/显示的 `Expr`（±∞ 用伪变量 `inf` 表示）
    pub fn into_expr(self) -> Expr {
        match self {
            LimitValue::Finite(n) => Expr::Number(n),
            LimitValue::Infinity(sign) if sign >= 0 => Expr::Variable("inf".to_string()),
            LimitValue::Infinity(_) => Expr::Unary(
                UnaryOp::Neg,
                Box::new(Expr::Variable("inf".to_string())),
            ),
        }
    }
}

/// 极限点
#[derive(Clone, Copy)]
enum Target {
    Finite,
    Inf(i32),
}

pub fn limit(
    ev: &crate::parser::Evaluator,
    f: &Expr,
    var: &str,
    point: &Expr,
) -> Result<Expr, String> {
    let target = match point {
        Expr::Variable(v) if v == "inf" => Target::Inf(1),
        Expr::Unary(UnaryOp::Neg, inner) => match inner.as_ref() {
            Expr::Variable(v) if v == "inf" => Target::Inf(-1),
            _ => Target::Finite,
        },
        _ if super::has_free_var_named(point, var) => {
            return Err(ERROR_LIMIT_POINT.to_string())
        }
        _ => Target::Finite,
    };

    // 1) 有限点先直接代入 —— 但**同样要过数值复核**：`floor`/`sign` 这类在点处不连续，
    //    代入值并不等于极限（`lim(floor(x), x, 0)` 代入得 0，实际左右极限是 -1 与 0）。
    if let Target::Finite = target {
        let a = constant_value(ev, point)
            .ok_or_else(|| ERROR_LIMIT_POINT.to_string())?;
        if let Ok(v) = ev.evaluate_with_var(f, var, &a) {
            if !v.is_complex() {
                let cand = LimitValue::Finite(v.clone());
                if numeric_allows(ev, f, var, &a, &cand) {
                    return Ok(Expr::Number(v));
                }
            }
        }
    }

    // 2) 结构分析
    match target {
        Target::Finite => {
            let a = constant_value(ev, point).ok_or_else(|| ERROR_LIMIT_POINT.to_string())?;
            if let Some(v) = lhopital(ev, f, var, &a)? {
                // **结构解也要过数值复核**：洛必达只保证"导数极限存在"时才成立，
                // 而 `abs(x)/x` 这类在 0 处会给出 0（`sign(0)=0`），实际左右极限是 ∓1。
                // 复核不通过就交给数值路径，由它报"左右极限不相等"。
                if numeric_allows(ev, f, var, &a, &v) {
                    return Ok(v.into_expr());
                }
            }
        }
        Target::Inf(sign) => {
            if let Some(v) = rational_at_infinity(ev, f, var, sign) {
                return Ok(v.into_expr());
            }
        }
    }

    // 3) 数值双侧
    numeric_limit(ev, f, var, target, point)
}

/* ---------------- 结构分析 ---------------- */

/// `0/0` 用洛必达；分子分母都能求导才继续，`diff` 报错就放弃（转数值）
fn lhopital(
    ev: &crate::parser::Evaluator,
    f: &Expr,
    var: &str,
    a: &Number,
) -> Result<Option<LimitValue>, String> {
    let Expr::Binary(num, BinOp::Div, den) = f else {
        return Ok(None);
    };
    let mut num = num.as_ref().clone();
    let mut den = den.as_ref().clone();
    for _ in 0..LIMIT_LHOPITAL_MAX {
        let n = side_value(ev, &num, var, a);
        let d = side_value(ev, &den, var, a);
        match (n, d) {
            // 0/0 ⇒ 继续洛必达
            (Side::Zero, Side::Zero) => {
                let Ok(dn) = super::diff::diff(ev, &num, var) else {
                    return Ok(None); // 不支持求导 ⇒ 交给数值路径，不把错误抛给用户
                };
                let Ok(dd) = super::diff::diff(ev, &den, var) else {
                    return Ok(None);
                };
                num = dn;
                den = dd;
            }
            // 非不定式：直接算商
            (Side::Finite(nv), Side::Finite(dv)) => {
                if dv.is_zero() {
                    return Ok(None);
                }
                // 分子为 0 而分母不为 0 ⇒ 0
                return Ok(Some(LimitValue::Finite(hipercalc_core::number::Number::div(&nv, &dv))));
            }
            // ∞/const 或 const/0 之类：交给数值路径判定符号
            _ => return Ok(None),
        }
    }
    Ok(None)
}

enum Side {
    Zero,
    Finite(Number),
    Other,
}

fn side_value(ev: &crate::parser::Evaluator, e: &Expr, var: &str, a: &Number) -> Side {
    match ev.evaluate_with_var(e, var, a) {
        Ok(v) if v.is_complex() => Side::Other,
        Ok(v) if v.is_zero() => Side::Zero,
        Ok(v) => Side::Finite(v),
        Err(_) => Side::Other,
    }
}

/// 结构解是否被数值证据支持：在 `a ± h`（h 很小）取样，若与候选值差得远则否定。
/// 采样点求值失败（该侧无定义）就跳过该侧；两侧都取不到样则放行（保守）。
fn numeric_allows(
    ev: &crate::parser::Evaluator,
    f: &Expr,
    var: &str,
    a: &Number,
    candidate: &LimitValue,
) -> bool {
    let LimitValue::Finite(c) = candidate else {
        return true; // ±∞ 不做数值复核（结构判定更可靠）
    };
    let mut checked = false;
    for k in [3i32, 5] {
        let h = pow10_neg(k);
        for delta in [
            hipercalc_core::number::Number::sub(a, &h),
            hipercalc_core::number::Number::add(a, &h),
        ] {
            let Ok(v) = ev.evaluate_with_var(f, var, &delta) else {
                continue;
            };
            if v.is_complex() {
                continue;
            }
            let d = hipercalc_core::bigfloat::BigFloat::sub(
                &v.to_approx(),
                &c.to_approx(),
                hipercalc_core::bigfloat::precision() + 10,
            );
            let scale = c.to_approx().magnitude_log10().max(0.0);
            // 判据放宽到 1e-1：连续函数在 h=1e-5/1e-3 处与极限值的偏差量级是 h·f'，
            // 正常都远小于此；而 `floor` 这类跳跃恰好是 1，仍能可靠抓到。
            // 万一连续但斜率极大被误拒，也会退回数值路径给出同样的值（只是可能标 ≈），不会给错答案。
            if !d.is_zero() && d.magnitude_log10() > -1.0 + scale {
                return false;
            }
            checked = true;
        }
    }
    let _ = checked;
    true
}

/// `x → ±∞` 且分子分母都是多项式：按次数比较
fn rational_at_infinity(
    ev: &crate::parser::Evaluator,
    f: &Expr,
    var: &str,
    sign: i32,
) -> Option<LimitValue> {
    let Expr::Binary(num, BinOp::Div, den) = f else {
        return None;
    };
    let vc = var.chars().next()?;
    let pn = crate::solver_poly::extract_polynomial(ev, num, vc)?;
    let pd = crate::solver_poly::extract_polynomial(ev, den, vc)?;
    let dn = poly_degree(&pn)?;
    let dd = poly_degree(&pd)?;
    let (ln_, ld) = (pn.get(dn)?.clone(), pd.get(dd)?.clone());
    Some(match dn.cmp(&dd) {
        std::cmp::Ordering::Less => LimitValue::Finite(Number::from_int(0)),
        std::cmp::Ordering::Equal => {
            let q = hipercalc_core::number::Number::div(&ln_, &ld);
            // x → -∞ 且次数差为奇数时符号翻转（这里次数相同，不翻）
            LimitValue::Finite(q)
        }
        std::cmp::Ordering::Greater => {
            let dir = if sign < 0 && (dn - dd) % 2 == 1 { -1 } else { 1 };
            let sign_of_q = if hipercalc_core::number::Number::div(&ln_, &ld).is_negative() {
                -1
            } else {
                1
            };
            LimitValue::Infinity(dir * sign_of_q)
        }
    })
}

fn poly_degree(coeffs: &[Number]) -> Option<usize> {
    coeffs.iter().rposition(|c| !c.is_zero())
}

/* ---------------- 数值双侧逼近 ---------------- */

fn numeric_limit(
    ev: &crate::parser::Evaluator,
    f: &Expr,
    var: &str,
    target: Target,
    point: &Expr,
) -> Result<Expr, String> {
    let tol = (hipercalc_core::bigfloat::display_digits() + 6) as f64;
    match target {
        Target::Finite => {
            let a = constant_value(ev, point).ok_or_else(|| ERROR_LIMIT_POINT.to_string())?;
            let iters = max_limit_iters().min(hipercalc_core::bigfloat::display_digits());
            let mut prev_l: Option<Number> = None;
            let mut prev_r: Option<Number> = None;
            let mut last_l: Option<Number> = None;
            let mut last_r: Option<Number> = None;
            // 两侧都取得到值、却持续不一致 ⇒ 极限不存在（`abs(x)/x`、`floor(x)` 在 0 处）
            let mut disagree_streak = 0usize;
            for k in 1..=iters {
                let h = pow10_neg(k as i32);
                let l = ev.evaluate_with_var(
                    f,
                    var,
                    &hipercalc_core::number::Number::sub(&a, &h),
                ).ok();
                let r = ev.evaluate_with_var(
                    f,
                    var,
                    &hipercalc_core::number::Number::add(&a, &h),
                ).ok();
                let l = l.filter(|v| !v.is_complex());
                let r = r.filter(|v| !v.is_complex());
                if let (Some(l), Some(r)) = (&l, &r) {
                    if !close(l, r, tol) {
                        // 单步不一致可能只是 h 还不够小；连续几步都不一致就判定不存在
                        disagree_streak += 1;
                        if disagree_streak >= 3 {
                            return Err(ERROR_LIMIT_ONE_SIDED.to_string());
                        }
                    } else {
                        disagree_streak = 0;
                    }
                    if close(l, r, tol) {
                    if let (Some(pl), Some(pr)) = (&prev_l, &prev_r) {
                        if close(pl, l, tol) && close(pr, r, tol) {
                            let avg = hipercalc_core::number::Number::div(
                                &hipercalc_core::number::Number::add(l, r),
                                &Number::from_int(2),
                            );
                            // 收敛值低于显示精度 ⇒ 按精确 0（`lim(sin(x)/x,x,inf)` 应是 0，
                            // 而不是 `-9.9e-29` 这种把噪声当结果）
                            return Ok(Expr::Number(snap_tiny(avg)));
                        }
                    }
                    }
                }
                if l.is_some() {
                    prev_l = l.clone();
                    last_l = l;
                }
                if r.is_some() {
                    prev_r = r.clone();
                    last_r = r;
                }
            }
            // 只有**单侧**可用时（如 sqrt(x) 在 0 的左邻域无定义）才用可用的一侧；
            // 两侧都取到过值却没收敛 ⇒ 不能挑一侧当答案
            if prev_l.is_none() {
                if let Some(v) = last_r {
                    return Ok(Expr::Number(snap_tiny(v)));
                }
            }
            if prev_r.is_none() {
                if let Some(v) = last_l {
                    return Ok(Expr::Number(snap_tiny(v)));
                }
            }
            Err(ERROR_LIMIT_UNDECIDED.to_string())
        }
        Target::Inf(sign) => {
            let mut prev: Option<Number> = None;
            for k in 2..=LIMIT_INF_K_MAX {
                let x = pow10(k);
                let x = if sign < 0 {
                    hipercalc_core::number::Number::sub(&Number::from_int(0), &x)
                } else {
                    x
                };
                let Ok(v) = ev.evaluate_with_var(f, var, &x) else {
                    continue; // 求值失败（可能触发护栏）⇒ 换下一个探测点
                };
                if v.is_complex() {
                    continue;
                }
                if let Some(p) = &prev {
                    if close(p, &v, tol) {
                        return Ok(Expr::Number(snap_tiny(v)));
                    }
                }
                prev = Some(v);
            }
            Err(ERROR_LIMIT_UNDECIDED.to_string())
        }
    }
}

/// 低于显示精度的值按精确 0 处理（与积分的无穷端点同一处理）
fn snap_tiny(v: Number) -> Number {
    let mag = v.to_approx().magnitude_log10();
    if v.is_zero() || mag < -(hipercalc_core::bigfloat::display_digits() as f64 + 5.0) {
        return Number::from_int(0);
    }
    v
}

/// 10^-k（精确有理数，保证 `a ± h` 不丢有效位）
fn pow10_neg(k: i32) -> Number {
    use num_bigint::BigInt;
    Number::from_rational(num_rational::BigRational::new(
        BigInt::from(1),
        BigInt::from(10u8).pow(k as u32),
    ))
}

fn pow10(k: i32) -> Number {
    use num_bigint::BigInt;
    Number::from_bigint(BigInt::from(10u8).pow(k as u32))
}

/// 相对判据：|a − b| ≤ 10^-tol · max(1, |b|)
fn close(a: &Number, b: &Number, tol: f64) -> bool {
    let d = hipercalc_core::bigfloat::BigFloat::sub(
        &a.to_approx(),
        &b.to_approx(),
        hipercalc_core::bigfloat::precision() + 10,
    );
    if d.is_zero() {
        return true;
    }
    let scale = b.to_approx().magnitude_log10().max(0.0);
    d.magnitude_log10() <= -tol + scale
}

fn constant_value(ev: &crate::parser::Evaluator, e: &Expr) -> Option<Number> {
    let v = ev.evaluate_with_vars(e, &[]).ok()?;
    if v.is_complex() {
        return None;
    }
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{DisplayMode, Evaluator, Parser};

    fn parse(s: &str) -> Expr {
        let mut p = Parser::new(s);
        p.parse_expression().unwrap()
    }

    /// 极限结果渲染成字符串
    fn lim(input: &str, point: &str, deg: bool) -> String {
        let mut ev = Evaluator::new();
        if deg {
            ev.angle_mode = hipercalc_core::trig::AngleMode::Degree;
        }
        let r = limit(&ev, &parse(input), "x", &parse(point)).unwrap();
        super::super::render::render_expr(&r, DisplayMode::MathIO)
    }

    fn lim_err(input: &str, point: &str) -> String {
        let ev = Evaluator::new();
        limit(&ev, &parse(input), "x", &parse(point)).unwrap_err()
    }

    #[test]
    fn direct_substitution_and_indeterminate() {
        assert_eq!(lim("x^2", "2", false), "4");
        // 0/0 ⇒ 洛必达
        assert_eq!(lim("(x^2-1)/(x-1)", "1", false), "2");
        assert_eq!(lim("sin(x)/x", "0", false), "1");
        assert_eq!(lim("(exp(x)-1)/x", "0", false), "1");
        assert_eq!(lim("ln(1+x)/x", "0", false), "1");
        assert_eq!(lim("(1-cos(x))/x^2", "0", false), "0.5");
    }

    #[test]
    fn infinity_targets() {
        assert_eq!(lim("1/x", "inf", false), "0");
        assert_eq!(lim("(2*x^2+1)/(x^2-3)", "inf", false), "2");
        assert_eq!(lim("x^3/(x^2+1)", "inf", false), "inf");
        assert_eq!(lim("(x+1)/(x^2+1)", "inf", false), "0");
        // 数值逼近：sin(x)/x → 0
        assert_eq!(lim("sin(x)/x", "inf", false), "0");
    }

    #[test]
    fn angle_mode_affects_trig_limits() {
        // Radian：sin(x)/x → 1
        assert_eq!(lim("sin(x)/x", "0", false), "1");
        // Degree：导数带 π/180 ⇒ 极限是 π/180（与全局三角语义一致，不是 bug）
        let d = lim("sin(x)/x", "0", true);
        assert!(d.starts_with("0.0174532925"), "{d}");
    }

    #[test]
    fn undecidable_and_missing_limit() {
        // 振荡：sin(x) 在 ∞ 处不收敛
        assert!(lim_err("sin(x)", "inf").contains("无法判定"));
        // 左右不一致：|x|/x 在 0 处
        assert!(lim_err("abs(x)/x", "0").contains("不相等") || lim_err("abs(x)/x", "0").contains("无法判定"));
        // 极限点含变量
        assert!(lim_err("x^2", "y").contains("常数"));
    }

    #[test]
    fn lhopital_degrades_when_diff_unsupported() {
        // floor 不支持求导 ⇒ 洛必达必须降级到数值，而不是抛"不支持求导"
        let e = lim_err("floor(x)", "0");
        assert!(!e.contains("不支持求导"), "{e}");
    }
}
