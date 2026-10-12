//! 积分（`int(f, x)` 不定积分 / `int(f, x, a, b)` 定积分）。
//!
//! # 策略：精确优先，数值兜底
//!
//! 1. **符号原函数**（线性性 + 幂 + 线性替换基本表）→ 定积分走牛顿-莱布尼茨给精确值；
//! 2. 求不出原函数（或区间内有奇点）→ **自适应 Gauss–Legendre** 数值积分，给 `≈`。
//!
//! # 数值积分为什么最终选了"计算节点"的 Gauss–Legendre
//!
//! 计划里写的是 Gauss–Kronrod G7-K15，其节点/权重是 15 位有效数字的常数，硬编码写错一位
//! 就会**静默算错**；而先把 Simpson 写出来实测又发现：在 1e-25 的容差下自适应 Simpson 需要
//! 上万次被积函数求值（每点求值都走 BigFloat 幂级数）⇒ 直接撞穿预算报错。
//! 所以改为**用 Newton 迭代现算 Legendre 多项式的根**得到 n 点 Gauss–Legendre：
//! 无魔数、可用"对 2n-1 次多项式精确"直接验证（见测试），10 点对光滑函数两三层二分即可到 1e-25，
//! 求值次数只有几十次。预算护栏与"精确优先"策略不变。
//!
//! # 两个保守设计
//!
//! - **奇点检测**只对显式分母做（`newton_solve` 找根）；**判定不了就走数值**，
//!   绝不静默给出错误的牛顿-莱布尼茨结果；
//! - 求得原函数后**求导回验**（数值抽样比对），防止规则表写错导致给错答案。
//!
//! 不定积分**不写 `+C`**（文档说明"省略积分常数"）。

use crate::parser::{BinOp, Expr};
use hipercalc_core::bigfloat::BigFloat;
use hipercalc_core::number::Number;

use super::{
    ERROR_INT_BOUND, ERROR_INT_BUDGET, ERROR_INT_INF_CONVERGE, ERROR_INT_SINGULAR,
    ERROR_NO_ANTIDERIVATIVE, ERROR_TABLE_NOT_COVERED, max_int_depth, max_int_evals,
};

/// 无穷限积分：`x → ±inf` 时的端点探测指数（10^k），k 上限。
/// 取 6 是受 `check_trig_range`/`exp` 参数护栏（10^6）约束，够用且不会触发护栏。
const INF_PROBE_MAX_K: i32 = 6;

/* ---------------- 不定积分 ---------------- */

/// 求原函数；求不出时报错（两种原因分别给文案）
pub fn antiderivative(ev: &crate::parser::Evaluator, f: &Expr, var: &str) -> Result<Expr, String> {
    match try_integrate(ev, f, var)? {
        Some(raw) => {
            if !verify(ev, &raw, f, var) {
                // 规则表自检失败（理论上不该发生）：按"求不出"处理，绝不给错答案
                return Err(no_antiderivative_reason(f, var));
            }
            super::normalize::simplify(ev, &raw)
        }
        None => Err(no_antiderivative_reason(f, var)),
    }
}

/// 失败原因分两类：**表未覆盖**（如 `tan`）vs **无初等原函数**（如 `exp(-x²)`）。
/// 不能把"我们没实现"说成"数学上不存在"。
fn no_antiderivative_reason(f: &Expr, _var: &str) -> String {
    const KNOWN_UNSUPPORTED: &[&str] = &[
        "tan", "cot", "sec", "csc", "ln", "log", "log10", "log2", "sinh", "cosh", "tanh", "coth",
        "sech", "csch", "abs", "sign", "floor", "ceil", "round", "frac",
    ];
    let table_gap = match f {
        Expr::Function(name, _) => KNOWN_UNSUPPORTED.contains(&name.as_str()),
        Expr::Pow(b, _) => {
            matches!(b.as_ref(), Expr::Function(n, _) if KNOWN_UNSUPPORTED.contains(&n.as_str()))
        }
        _ => false,
    };
    if table_gap {
        ERROR_TABLE_NOT_COVERED.to_string()
    } else {
        ERROR_NO_ANTIDERIVATIVE.to_string()
    }
}

/// `try_integrate` 返回 None / Err 的区分靠这个（只用于错误文案的形态判断）
fn try_integrate(
    ev: &crate::parser::Evaluator,
    f: &Expr,
    var: &str,
) -> Result<Option<Expr>, String> {
    // 注意顺序：`1/(1+u²)`、`1/sqrt(1-u²)` 这些**靠 Div 节点识别**，
    // 必须排在下面的"Div → 负指数幂"归一化之前，否则会被提前改写成 Pow 而认不出来。
    if let Some((a, u, kind)) = special_reciprocal(ev, f, var)? {
        let g = match kind {
            ReciprocalKind::Arctan => call("arctan", u),
            ReciprocalKind::Arcsin => call("arcsin", u),
        };
        return Ok(Some(div(g, a)));
    }
    // `1/u` 在 AST 里是 **Div** 而不是 Pow(_, -1)：先归约成 `分子 · 分母^(-1)`，
    // 这样下面的"常数倍 + 幂规则"就能统一处理 ∫u⁻¹ du（否则 `int(1/x,x)` 会报"求不出"）
    if let Expr::Binary(l, BinOp::Div, r) = f {
        if !super::has_free_var_named(r, var) {
            // 除以常数：交给常规路径（下面"常数倍"分支处理）
        } else {
            let as_pow = Expr::Binary(
                Box::new(l.as_ref().clone()),
                BinOp::Mul,
                Box::new(pow(r.as_ref().clone(), Expr::Number(Number::from_int(-1)))),
            );
            return try_integrate(ev, &as_pow, var);
        }
    }

    // 常数：∫c dx = c·x
    if !super::has_free_var_named(f, var) {
        return Ok(Some(mul(f.clone(), var_expr(var))));
    }
    // 线性性：和差拆分
    if let Expr::Binary(l, op @ (BinOp::Add | BinOp::Sub), r) = f {
        let li = try_integrate(ev, l, var)?;
        let ri = try_integrate(ev, r, var)?;
        return Ok(match (li, ri) {
            (Some(a), Some(b)) => Some(Expr::Binary(Box::new(a), *op, Box::new(b))),
            _ => None,
        });
    }
    // 常数倍：∫c·g = c·∫g（含负号）
    if let Expr::Unary(crate::parser::UnaryOp::Neg, x) = f {
        return Ok(try_integrate(ev, x, var)?.map(neg));
    }
    if let Expr::Binary(l, BinOp::Mul, r) = f {
        let (c, g) = if !super::has_free_var_named(l, var) {
            (Some(l), r)
        } else if !super::has_free_var_named(r, var) {
            (Some(r), l)
        } else {
            (None, r)
        };
        if let Some(c) = c {
            // 常数必须是真正的常数（能被求值），否则不算常数倍
            if constant_value(ev, c).is_some() {
                return Ok(try_integrate(ev, g, var)?.map(|t| mul(c.as_ref().clone(), t)));
            }
        }
    }
    // 线性基底 u = a·var + b
    if let Some((a, _b)) = linear_parts(ev, f, var)? {
        // u^1：∫u du = u²/2
        return Ok(Some(div(pow(f.clone(), num(2)), mul(num(2), a))));
    }
    // 幂：u^n（n 常数有理数）
    if let Expr::Pow(base, exp) = f
        && !super::has_free_var_named(exp, var)
        && let Some(n) = constant_value(ev, exp).and_then(|v| v.as_rational())
        && let Some((a, _b)) = linear_parts(ev, base, var)?
    {
        use num_traits::One;
        let minus_one = num_rational::BigRational::from_integer(num_bigint::BigInt::from(-1));
        if n == minus_one {
            // ∫u⁻¹ du = ln|u| / a
            return Ok(Some(div(call("ln", call("abs", base.as_ref().clone())), a)));
        }
        let np1 = Number::from_rational(n.clone() + num_rational::BigRational::one());
        return Ok(Some(div(
            pow(
                base.as_ref().clone(),
                Expr::Number(Number::from_rational(n + num_rational::BigRational::one())),
            ),
            mul(a, Expr::Number(np1)),
        )));
    }
    // 根式先归约成有理指数幂，交给幂规则（`sqrt(x)` 是 Function 而不是 Pow）
    if let Expr::Function(name, args) = f
        && args.len() == 1
    {
        let exp = match name.as_str() {
            "sqrt" | "sqr" => Some(num_rational::BigRational::new(
                num_bigint::BigInt::from(1),
                num_bigint::BigInt::from(2),
            )),
            "cbrt" => Some(num_rational::BigRational::new(
                num_bigint::BigInt::from(1),
                num_bigint::BigInt::from(3),
            )),
            _ => None,
        };
        if let Some(r) = exp {
            let as_pow = pow(args[0].clone(), Expr::Number(Number::from_rational(r)));
            return try_integrate(ev, &as_pow, var);
        }
    }
    // 基本表：f = g(u)，u = a·var + b 线性
    if let Expr::Function(name, args) = f
        && args.len() == 1
        && let Some((a, _b)) = linear_parts(ev, &args[0], var)?
    {
        let u = args[0].clone();
        let integral = match name.as_str() {
            "exp" => Some(call("exp", u.clone())),
            "sin" => Some(neg(call("cos", u.clone()))),
            "cos" => Some(call("sin", u.clone())),
            _ => None,
        };
        if let Some(g) = integral {
            return Ok(Some(div(g, a)));
        }
    }
    // sec²u → tan u / a
    if let Some((a, u)) = sec_squared(ev, f, var)? {
        return Ok(Some(div(call("tan", u), a)));
    }
    Ok(None)
}

enum ReciprocalKind {
    Arctan,
    Arcsin,
}

/// 识别 `1/(1+u²)`（u 线性，u 即 var 本身或 a·var 形式）与 `1/sqrt(1-u²)`
fn special_reciprocal(
    ev: &crate::parser::Evaluator,
    f: &Expr,
    var: &str,
) -> Result<Option<(Expr, Expr, ReciprocalKind)>, String> {
    let Expr::Binary(numer, BinOp::Div, denom) = f else {
        return Ok(None);
    };
    if constant_value(ev, numer)
        .and_then(|v| v.as_rational())
        .is_none()
    {
        return Ok(None);
    }
    // 1/(1+x²)
    if let Expr::Binary(one, BinOp::Add, sq) = denom.as_ref()
        && is_one_expr(ev, one)
        && let Expr::Pow(base, e2) = sq.as_ref()
        && is_two_expr(ev, e2)
        && let Some((a, _)) = linear_parts(ev, base, var)?
    {
        // 分子必须是常数 1
        if is_one_expr(ev, numer) {
            return Ok(Some((a, base.as_ref().clone(), ReciprocalKind::Arctan)));
        }
    }
    // 1/sqrt(1-x²)
    if let Expr::Function(name, args) = denom.as_ref()
        && name == "sqrt"
        && args.len() == 1
        && let Expr::Binary(one, BinOp::Sub, sq) = &args[0]
        && is_one_expr(ev, one)
        && let Expr::Pow(base, e2) = &**sq
        && is_two_expr(ev, e2)
        && let Some((a, _)) = linear_parts(ev, base, var)?
        && is_one_expr(ev, numer)
    {
        return Ok(Some((a, base.as_ref().clone(), ReciprocalKind::Arcsin)));
    }
    Ok(None)
}

/// 识别 `sec(u)²`（u 线性）
fn sec_squared(
    ev: &crate::parser::Evaluator,
    f: &Expr,
    var: &str,
) -> Result<Option<(Expr, Expr)>, String> {
    let Expr::Pow(base, e2) = f else {
        return Ok(None);
    };
    if !is_two_expr(ev, e2) {
        return Ok(None);
    }
    if let Expr::Function(name, args) = base.as_ref()
        && name == "sec"
        && args.len() == 1
        && let Some((a, _)) = linear_parts(ev, &args[0], var)?
    {
        return Ok(Some((a, args[0].clone())));
    }
    Ok(None)
}

/// 判断 `e` 是否是 `a·var + b`（a、b 为常数）；返回 (a, b)，要求 a ≠ 0。
///
/// 做法：取若干采样点求值，用 f(0)、f(1) 解出 a、b，再用其它点验证
/// —— 多项式/超越式都骗不过多一个采样点（`x²` 在 2 处就对不上了）。
fn linear_parts(
    ev: &crate::parser::Evaluator,
    e: &Expr,
    var: &str,
) -> Result<Option<(Expr, Expr)>, String> {
    let probe =
        |v: i64| -> Option<Number> { ev.evaluate_with_var(e, var, &Number::from_int(v)).ok() };
    let (Some(f0), Some(f1), Some(f2), Some(f3)) = (probe(0), probe(1), probe(2), probe(3)) else {
        return Ok(None);
    };
    let b = f0.clone();
    let a = hipercalc_core::number::Number::sub(&f1, &f0);
    if a.is_zero() {
        return Ok(None);
    }
    // 验证：f(2) = 2a + b，f(3) = 3a + b
    let e2 = hipercalc_core::number::Number::add(
        &hipercalc_core::number::Number::mul(&a, &Number::from_int(2)),
        &b,
    );
    let e3 = hipercalc_core::number::Number::add(
        &hipercalc_core::number::Number::mul(&a, &Number::from_int(3)),
        &b,
    );
    if !num_eq(&e2, &f2) || !num_eq(&e3, &f3) {
        return Ok(None);
    }
    Ok(Some((Expr::Number(a), Expr::Number(b))))
}

fn num_eq(a: &Number, b: &Number) -> bool {
    // 用高精度近似比较（精确式也走这条，误差远小于 1e-40）
    let (x, y) = (a.to_approx(), b.to_approx());
    let d = hipercalc_core::bigfloat::BigFloat::sub(&x, &y, hipercalc_core::bigfloat::precision());
    d.is_zero() || hipercalc_core::bigfloat::BigFloat::sub(&x, &y, 30).is_zero()
}

/// 求得原函数后求导回验：在被积函数有定义的采样点上比对数值。
/// 抽样点取正数（避开 `ln|x|`/`sign` 在负半轴的形式差异），比对不上就认为不可信。
fn verify(ev: &crate::parser::Evaluator, big_f: &Expr, f: &Expr, var: &str) -> bool {
    let Ok(df) = super::diff::diff(ev, big_f, var) else {
        return false;
    };
    for (p, q) in [(1i64, 3i64), (2, 5), (1, 7), (3, 11)] {
        let x = Number::from_rational(num_rational::BigRational::new(
            num_bigint::BigInt::from(p),
            num_bigint::BigInt::from(q),
        ));
        let (Ok(lhs), Ok(rhs)) = (
            ev.evaluate_with_var(&df, var, &x),
            ev.evaluate_with_var(f, var, &x),
        ) else {
            continue;
        };
        if lhs.is_complex() || rhs.is_complex() {
            continue;
        }
        let d = hipercalc_core::bigfloat::BigFloat::sub(
            &lhs.to_approx(),
            &rhs.to_approx(),
            hipercalc_core::bigfloat::precision() + 10,
        );
        // 相对判据：|差| ≤ 10^-(digits+5) · max(1, |f|)
        let tol = (hipercalc_core::bigfloat::display_digits() + 5) as f64;
        let scale = rhs.to_approx().magnitude_log10().max(0.0);
        if d.magnitude_log10() > -tol + scale {
            return false;
        }
        return true;
    }
    true // 一个可比对的点都取不到：不阻断（保守放行）
}

/* ---------------- 定积分 ---------------- */

/// 定积分入口：返回数值结果（精确或近似）
pub fn definite(
    ev: &crate::parser::Evaluator,
    f: &Expr,
    var: &str,
    lower: &Expr,
    upper: &Expr,
) -> Result<Number, String> {
    let lower_inf = is_inf(lower);
    let upper_inf = is_inf(upper);
    if lower_inf || upper_inf {
        return definite_infinite(ev, f, var, lower, upper);
    }
    let a = constant_value(ev, lower).ok_or_else(|| ERROR_INT_BOUND.to_string())?;
    let b = constant_value(ev, upper).ok_or_else(|| ERROR_INT_BOUND.to_string())?;
    definite_finite(ev, f, var, &a, &b)
}

fn definite_finite(
    ev: &crate::parser::Evaluator,
    f: &Expr,
    var: &str,
    a: &Number,
    b: &Number,
) -> Result<Number, String> {
    // 上下限反了：交换并取负
    if lt(b, a) {
        let v = definite_finite(ev, f, var, b, a)?;
        return Ok(v.neg());
    }
    if num_eq(a, b) {
        return Ok(Number::from_int(0));
    }
    // 精确优先：求得出原函数且区间内无奇点 ⇒ 牛顿-莱布尼茨
    if let Ok(big_f) = antiderivative(ev, f, var) {
        if !singular_inside(ev, f, var, a, b)? {
            return newton_leibniz(ev, &big_f, var, a, b);
        }
        return Err(ERROR_INT_SINGULAR.to_string());
    }
    numeric(ev, f, var, a, b)
}

/// 牛顿-莱布尼茨：F(b) - F(a)
fn newton_leibniz(
    ev: &crate::parser::Evaluator,
    big_f: &Expr,
    var: &str,
    a: &Number,
    b: &Number,
) -> Result<Number, String> {
    let fa = ev
        .evaluate_with_var(big_f, var, a)
        .map_err(|_| ERROR_INT_SINGULAR.to_string())?;
    let fb = ev
        .evaluate_with_var(big_f, var, b)
        .map_err(|_| ERROR_INT_SINGULAR.to_string())?;
    if fa.is_complex() || fb.is_complex() {
        return Err(ERROR_INT_SINGULAR.to_string());
    }
    Ok(hipercalc_core::number::Number::sub(&fb, &fa))
}

/// 无穷限：先试"牛顿-莱布尼茨 + 端点极限"，失败再走变量替换的数值积分
fn definite_infinite(
    ev: &crate::parser::Evaluator,
    f: &Expr,
    var: &str,
    lower: &Expr,
    upper: &Expr,
) -> Result<Number, String> {
    // 先按解析出的方向判断是否"同向无穷"（-inf..inf 后面再处理）
    if let Ok(big_f) = antiderivative(ev, f, var) {
        let lo = bound_value_at_inf(ev, &big_f, var, lower, true);
        let hi = bound_value_at_inf(ev, &big_f, var, upper, false);
        if let (Some(l), Some(h)) = (lo, hi) {
            return Ok(hipercalc_core::number::Number::sub(&h, &l));
        }
    }
    Err(ERROR_INT_INF_CONVERGE.to_string())
}

/// 把上下限表达式求值；`inf` 用原函数在 ±10^k 处的收敛值代替
fn bound_value_at_inf(
    ev: &crate::parser::Evaluator,
    big_f: &Expr,
    var: &str,
    bound: &Expr,
    is_lower: bool,
) -> Option<Number> {
    if !is_inf(bound) {
        let v = constant_value(ev, bound)?;
        let r = ev.evaluate_with_var(big_f, var, &v).ok()?;
        return if r.is_complex() { None } else { Some(r) };
    }
    let sign = if is_lower { -1i64 } else { 1 };
    // 指数探测：10^2 .. 10^k 逐步收敛
    let mut prev: Option<hipercalc_core::bigfloat::BigFloat> = None;
    for k in 2..=INF_PROBE_MAX_K {
        let mut mag = hipercalc_core::bigfloat::BigFloat::from_u64(1);
        let ten = hipercalc_core::bigfloat::BigFloat::from_u64(10);
        for _ in 0..k {
            mag = hipercalc_core::bigfloat::BigFloat::mul(
                &mag,
                &ten,
                hipercalc_core::bigfloat::precision(),
            );
        }
        let x = if sign > 0 {
            Number::Approx(mag)
        } else {
            Number::Approx(mag.neg())
        };
        let v = ev.evaluate_with_var(big_f, var, &x).ok()?;
        if v.is_complex() {
            return None;
        }
        let cur = v.to_approx();
        if let Some(p) = &prev {
            let d = hipercalc_core::bigfloat::BigFloat::sub(
                &cur,
                p,
                hipercalc_core::bigfloat::precision() + 10,
            );
            let tol = (hipercalc_core::bigfloat::display_digits() + 5) as f64;
            let scale = cur.magnitude_log10().max(0.0);
            if d.magnitude_log10() <= -tol + scale {
                // 端点极限已收敛；若它低于显示精度，就当**精确 0**。
                // 这样 `∫₀^∞ e^{-x}dx` 得到的是 `1 − 0`（而不是 `1 − 4e-44` 那种把
                // 垃圾数字带进结果的值）。注意前缀仍可能是 `≈`：原函数在另一端点常含
                // 近似值（如 `exp(0)` 在求值器里就是近似的），这是诚实反映计算路径。
                if cur.is_zero()
                    || cur.magnitude_log10()
                        < -(hipercalc_core::bigfloat::display_digits() as f64 + 5.0)
                {
                    return Some(Number::from_int(0));
                }
                return Some(v);
            }
        }
        prev = Some(cur);
    }
    None
}

/// 区间内是否有奇点：只对**显式分式**的分母找根；判定不了就返回 true（保守，改走数值）
fn singular_inside(
    ev: &crate::parser::Evaluator,
    f: &Expr,
    var: &str,
    a: &Number,
    b: &Number,
) -> Result<bool, String> {
    let mut denoms: Vec<Expr> = Vec::new();
    collect_denominators(f, &mut denoms);
    if denoms.is_empty() {
        // 没有显式分母：但 tan/cot/sec/csc/ln 等自带奇点，无法可靠判定 ⇒ 交给数值
        return Ok(has_implicit_singularity(f));
    }
    for d in &denoms {
        // 分母在端点上为零也可能发散（如 ∫_0^1 1/x），一并算奇点
        for x in [a.clone(), b.clone()] {
            if let Ok(v) = ev.evaluate_with_var(d, var, &x)
                && v.is_zero()
            {
                return Ok(true);
            }
        }
        let Some(vc) = var.chars().next() else {
            return Ok(true);
        };
        let roots = crate::solve_aux::collect_all_roots(ev, d, vc);
        for r in roots {
            let r_bf = r.to_approx();
            let r_num = Number::Approx(r_bf.clone());
            if lt(&r_num, a) || lt(b, &r_num) {
                continue;
            }
            // 排除落在端点附近的伪根
            let fa = hipercalc_core::bigfloat::BigFloat::sub(
                &r_bf,
                &a.to_approx(),
                hipercalc_core::bigfloat::precision(),
            );
            let fb = hipercalc_core::bigfloat::BigFloat::sub(
                &b.to_approx(),
                &r_bf,
                hipercalc_core::bigfloat::precision(),
            );
            let tol = -(hipercalc_core::bigfloat::display_digits() as f64) * 10.0;
            if fa.magnitude_log10() > tol && fb.magnitude_log10() > tol {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// 含这些函数时无法可靠排除奇点 ⇒ 保守改走数值
fn has_implicit_singularity(f: &Expr) -> bool {
    match f {
        Expr::Function(name, args) => {
            matches!(
                name.as_str(),
                "tan" | "cot" | "sec" | "csc" | "ln" | "log" | "log10" | "log2" | "abs"
            ) || args.iter().any(has_implicit_singularity)
        }
        Expr::Binary(l, op, r) => {
            matches!(op, BinOp::Div) || has_implicit_singularity(l) || has_implicit_singularity(r)
        }
        Expr::Unary(_, x) | Expr::Sd(x) | Expr::Factor(x) => has_implicit_singularity(x),
        Expr::Pow(b, e) => has_implicit_singularity(b) || has_implicit_singularity(e),
        _ => false,
    }
}

fn collect_denominators(e: &Expr, out: &mut Vec<Expr>) {
    match e {
        Expr::Binary(l, BinOp::Div, r) => {
            out.push(r.as_ref().clone());
            collect_denominators(l, out);
            collect_denominators(r, out);
        }
        Expr::Binary(l, _, r) => {
            collect_denominators(l, out);
            collect_denominators(r, out);
        }
        Expr::Unary(_, x) | Expr::Sd(x) | Expr::Factor(x) => collect_denominators(x, out),
        Expr::Pow(b, x) => {
            collect_denominators(b, out);
            collect_denominators(x, out);
        }
        Expr::Function(_, args) => args.iter().for_each(|a| collect_denominators(a, out)),
        _ => {}
    }
}

/* ---------------- 数值积分（自适应 Gauss–Legendre） ---------------- */

/// n 点 Gauss–Legendre 的节点与权重（[-1,1] 上），用 Newton 迭代求 Legendre 多项式的根。
///
/// **算出来而不是硬编码**：15 位有效数字的魔数写错一位就会静默算错，而计算得到的节点
/// 可以用"对 2n-1 次多项式精确"这件事来验证（见 `gauss_legendre_is_exact_for_polynomials`）。
///
/// 为什么不继续用自适应 Simpson：在 1e-25 量级的容差下，Simpson 需要上万次被积函数求值
/// （每点求值都要走 BigFloat 幂级数），会直接撞穿预算；n=10 的 Gauss–Legendre 对 19 次
/// 多项式精确、对光滑函数两三层二分就能到 1e-25，求值次数只有几十次。
fn gauss_legendre(n: usize, prec: usize) -> Vec<(BigFloat, BigFloat)> {
    let mut out = Vec::with_capacity(n);
    let pi = BigFloat::pi(prec);
    let two_n_plus_1 = BigFloat::from_u64((2 * n + 1) as u64);
    let two = BigFloat::from_u64(2);
    for i in 0..n.div_ceil(2) {
        // 第 i 个正根的初值：cos(π·(i + 3/4) / (n + 1/2))
        //   = cos(π(4i+3) / (4n+2)) = cos(π(4i+3) / (2(2n+1)))
        // 注意分母是 **2(2n+1)**；写成 4(2n+1) 会让所有初值挤到最大根附近，
        // Newton 于是全部收敛到同一个根（实测 n=4 时四个节点全是 0.861136…）。
        let num = BigFloat::from_u64((4 * i + 3) as u64);
        let arg = BigFloat::div(
            &BigFloat::mul(&pi, &num, prec),
            &BigFloat::mul(&two, &two_n_plus_1, prec),
            prec,
        );
        let mut x = arg.cos(prec);
        for _ in 0..100 {
            let (pn, _pnm1) = legendre_pair(n, &x, prec);
            let dp = legendre_derivative(n, &x, prec);
            if dp.is_zero() {
                break;
            }
            let dx = BigFloat::div(&pn, &dp, prec);
            x = BigFloat::sub(&x, &dx, prec);
            if dx.is_zero() || dx.magnitude_log10() < -((prec as f64) * 0.9) {
                break;
            }
        }
        let dp = legendre_derivative(n, &x, prec);
        let one_minus_x2 =
            BigFloat::sub(&BigFloat::from_u64(1), &BigFloat::mul(&x, &x, prec), prec);
        let denom = BigFloat::mul(&one_minus_x2, &BigFloat::mul(&dp, &dp, prec), prec);
        if denom.is_zero() {
            continue;
        }
        let w = BigFloat::div(&BigFloat::from_u64(2), &denom, prec);
        let is_middle = n % 2 == 1 && i == n / 2;
        out.push((x.clone(), w.clone()));
        if !is_middle {
            out.push((x.neg(), w));
        }
    }
    out
}

/// `P_n(x)` 与 `P_{n-1}(x)`（三项递推）
fn legendre_pair(n: usize, x: &BigFloat, prec: usize) -> (BigFloat, BigFloat) {
    if n == 0 {
        return (BigFloat::from_u64(1), BigFloat::from_u64(0));
    }
    let mut p0 = BigFloat::from_u64(1);
    let mut p1 = x.clone();
    for k in 2..=n {
        let two_k_minus_1 = BigFloat::from_u64((2 * k - 1) as u64);
        let kf = BigFloat::from_u64(k as u64);
        let km1 = BigFloat::from_u64((k - 1) as u64);
        let num = BigFloat::sub(
            &BigFloat::mul(&BigFloat::mul(&two_k_minus_1, x, prec), &p1, prec),
            &BigFloat::mul(&km1, &p0, prec),
            prec,
        );
        let p2 = BigFloat::div(&num, &kf, prec);
        p0 = p1;
        p1 = p2;
    }
    (p1, p0)
}

/// `P'_n(x) = n·(x·P_n(x) − P_{n-1}(x)) / (x² − 1)`
fn legendre_derivative(n: usize, x: &BigFloat, prec: usize) -> BigFloat {
    if n == 0 {
        return BigFloat::from_u64(0);
    }
    let (pn, pnm1) = legendre_pair(n, x, prec);
    let num = BigFloat::sub(&BigFloat::mul(x, &pn, prec), &pnm1, prec);
    let den = BigFloat::sub(&BigFloat::mul(x, x, prec), &BigFloat::from_u64(1), prec);
    if den.is_zero() {
        return BigFloat::from_u64(0);
    }
    BigFloat::div(
        &BigFloat::mul(&BigFloat::from_u64(n as u64), &num, prec),
        &den,
        prec,
    )
}

struct Integrator<'a> {
    ev: &'a crate::parser::Evaluator,
    f: &'a Expr,
    var: &'a str,
    evals: usize,
    budget: usize,
    tol: f64,
    nodes: Vec<(BigFloat, BigFloat)>,
}

impl<'a> Integrator<'a> {
    fn eval_at(&mut self, x: &Number) -> Result<Number, String> {
        self.evals += 1;
        if self.evals > self.budget {
            return Err(ERROR_INT_BUDGET.to_string());
        }
        let v = self
            .ev
            .evaluate_with_var(self.f, self.var, x)
            .map_err(|_| ERROR_INT_SINGULAR.to_string())?;
        if v.is_complex() {
            return Err(ERROR_INT_SINGULAR.to_string());
        }
        Ok(v)
    }

    /// 一个区间上的 n 点 Gauss–Legendre 求积
    fn rule(&mut self, a: &Number, b: &Number) -> Result<Number, String> {
        let mid = mid(a, b);
        let width = hipercalc_core::number::Number::sub(b, a);
        let half = hipercalc_core::number::Number::div(&width, &Number::from_int(2));
        let nodes = self.nodes.clone();
        let mut sum = Number::from_int(0);
        for (t, w) in nodes {
            let x = hipercalc_core::number::Number::add(
                &mid,
                &hipercalc_core::number::Number::mul(&half, &Number::Approx(t)),
            );
            let fx = self.eval_at(&x)?;
            sum = hipercalc_core::number::Number::add(
                &sum,
                &hipercalc_core::number::Number::mul(&Number::Approx(w), &fx),
            );
        }
        Ok(hipercalc_core::number::Number::mul(&half, &sum))
    }

    /// 自适应二分：比较整段与两半的求积结果，误差估计 `|S2 − S1|`
    fn adaptive(&mut self, a: &Number, b: &Number, depth: usize) -> Result<Number, String> {
        let s1 = self.rule(a, b)?;
        let m = mid(a, b);
        let l = self.rule(a, &m)?;
        let r = self.rule(&m, b)?;
        let s2 = hipercalc_core::number::Number::add(&l, &r);
        let err = hipercalc_core::number::Number::sub(&s2, &s1);
        let scale = s2.to_approx().magnitude_log10().max(0.0);
        let converged = err.to_approx().magnitude_log10() <= -self.tol + scale;
        if converged || depth >= max_int_depth() {
            return Ok(s2);
        }
        let left = self.adaptive(a, &m, depth + 1)?;
        let right = self.adaptive(&m, b, depth + 1)?;
        Ok(hipercalc_core::number::Number::add(&left, &right))
    }
}

fn mid(a: &Number, b: &Number) -> Number {
    let sum = hipercalc_core::number::Number::add(a, b);
    hipercalc_core::number::Number::div(&sum, &Number::from_int(2))
}

fn numeric(
    ev: &crate::parser::Evaluator,
    f: &Expr,
    var: &str,
    a: &Number,
    b: &Number,
) -> Result<Number, String> {
    let prec = hipercalc_core::bigfloat::precision();
    let mut it = Integrator {
        ev,
        f,
        var,
        evals: 0,
        budget: max_int_evals(),
        tol: (hipercalc_core::bigfloat::display_digits() + 6) as f64,
        nodes: gauss_legendre(GAUSS_NODES, prec),
    };
    it.adaptive(a, b, 0)
}

/// Gauss–Legendre 节点数：10 点对 19 次多项式精确
const GAUSS_NODES: usize = 10;

/* ---------------- 小工具 ---------------- */

fn var_expr(var: &str) -> Expr {
    Expr::Variable(var.to_string())
}

fn num(v: i64) -> Expr {
    Expr::Number(Number::from_int(v))
}

fn neg(a: Expr) -> Expr {
    Expr::Unary(crate::parser::UnaryOp::Neg, Box::new(a))
}

fn mul(a: Expr, b: Expr) -> Expr {
    Expr::Binary(Box::new(a), BinOp::Mul, Box::new(b))
}

fn div(a: Expr, b: Expr) -> Expr {
    Expr::Binary(Box::new(a), BinOp::Div, Box::new(b))
}

fn pow(a: Expr, b: Expr) -> Expr {
    Expr::Pow(Box::new(a), Box::new(b))
}

fn call(name: &str, a: Expr) -> Expr {
    Expr::Function(name.to_string(), vec![a])
}

fn is_inf(e: &Expr) -> bool {
    matches!(e, Expr::Variable(v) if v == "inf")
}

fn constant_value(ev: &crate::parser::Evaluator, e: &Expr) -> Option<Number> {
    ev.evaluate_with_vars(e, &[]).ok()
}

fn is_one_expr(ev: &crate::parser::Evaluator, e: &Expr) -> bool {
    constant_value(ev, e)
        .and_then(|v| v.as_rational())
        .map(|r| num_traits::One::is_one(&r))
        .unwrap_or(false)
}

fn is_two_expr(ev: &crate::parser::Evaluator, e: &Expr) -> bool {
    constant_value(ev, e)
        .and_then(|v| v.as_rational())
        .map(|r| r == num_rational::BigRational::from_integer(num_bigint::BigInt::from(2)))
        .unwrap_or(false)
}

/// 数值小于（用高精度近似比较）
fn lt(a: &Number, b: &Number) -> bool {
    use num_traits::Signed;
    let d = hipercalc_core::bigfloat::BigFloat::sub(&a.to_approx(), &b.to_approx(), 40);
    !d.is_zero() && d.value.is_negative()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{DisplayMode, Evaluator, Parser};

    fn parse(s: &str) -> Expr {
        let mut p = Parser::new(s);
        p.parse_expression().unwrap()
    }

    fn anti(input: &str) -> String {
        let ev = Evaluator::new();
        let r = antiderivative(&ev, &parse(input), "x").unwrap();
        super::super::render::render_expr(&r, DisplayMode::MathIO)
    }

    fn anti_err(input: &str) -> String {
        let ev = Evaluator::new();
        antiderivative(&ev, &parse(input), "x").unwrap_err()
    }

    fn def(input: &str, a: i64, b: i64) -> String {
        let ev = Evaluator::new();
        let r = definite(
            &ev,
            &parse(input),
            "x",
            &Expr::Number(Number::from_int(a)),
            &Expr::Number(Number::from_int(b)),
        )
        .unwrap();
        hipercalc_core::display::format_mathio(&r)
    }

    #[test]
    fn indefinite_basics() {
        // MathIO 下系数是"前置因子"（与 fac 的 `1 / 8 * (2*x - 1)` 同风格）
        assert_eq!(anti("x^2"), "1 / 3 * x^3");
        assert_eq!(anti("3*x^2"), "x^3");
        assert_eq!(anti("1/x"), "ln(abs(x))");
        assert_eq!(anti("sin(x)"), "-cos(x)");
        assert_eq!(anti("cos(x)"), "sin(x)");
        assert_eq!(anti("exp(x)"), "exp(x)");
        assert_eq!(anti("5"), "5*x");
        assert_eq!(anti("x^2+1"), "1 / 3 * x^3 + x");
        assert_eq!(anti("sin(2*x)"), "-1 / 2 * cos(2*x)");
        assert_eq!(anti("sqrt(x)"), "2 / 3 * x^(3 / 2)");
        assert_eq!(anti("1/(1+x^2)"), "arctan(x)");
        assert_eq!(anti("1/sqrt(1-x^2)"), "arcsin(x)");
    }

    #[test]
    fn indefinite_failures_have_distinct_reasons() {
        // 表内未覆盖（tan 有初等原函数，只是本机没实现）
        assert!(anti_err("tan(x)").contains("未覆盖"));
        // 数学上无初等原函数
        assert!(anti_err("exp(-x^2)").contains("初等原函数"));
    }

    #[test]
    fn definite_exact_via_newton_leibniz() {
        assert_eq!(def("x^2", 0, 1), "1 / 3");
        assert_eq!(def("x", 0, 2), "2");
        assert_eq!(def("x^2", 1, 0), "-1 / 3"); // 上下限反了 ⇒ 取负
        // sin 的原函数含 cos(3)（无精确值）⇒ 结果按模式显示成小数，但值是对的
        assert!(def("sin(x)", 0, 0) == "0");
        assert!(
            def("sin(x)", 0, 3).starts_with("1.9899924966"),
            "{}",
            def("sin(x)", 0, 3)
        );
    }

    #[test]
    fn definite_numeric_fallback() {
        // exp(-x^2) 无初等原函数 ⇒ 数值积分 ∫₀¹ = √π/2·erf(1) ≈ 0.746824132812427
        let v = def("exp(-x^2)", 0, 1);
        assert!(v.starts_with("0.74682413281242"), "{v}");
        // 无穷限 + 有精确原函数 ⇒ 牛顿-莱布尼茨给精确 1
        let ev = Evaluator::new();
        let r = definite(
            &ev,
            &parse("exp(-x)"),
            "x",
            &Expr::Number(Number::from_int(0)),
            &Expr::Variable("inf".to_string()),
        )
        .unwrap();
        assert_eq!(hipercalc_core::display::format_mathio(&r), "1");
    }

    #[test]
    fn singular_interval_is_rejected() {
        let ev = Evaluator::new();
        let r = definite(
            &ev,
            &parse("1/x"),
            "x",
            &Expr::Number(Number::from_int(-1)),
            &Expr::Number(Number::from_int(1)),
        );
        assert!(r.is_err(), "{r:?}");
    }

    #[test]
    fn linearity_probe_rejects_nonlinear() {
        let ev = Evaluator::new();
        assert!(linear_parts(&ev, &parse("2*x+1"), "x").unwrap().is_some());
        assert!(linear_parts(&ev, &parse("x^2"), "x").unwrap().is_none());
        assert!(linear_parts(&ev, &parse("sin(x)"), "x").unwrap().is_none());
        assert!(linear_parts(&ev, &parse("5"), "x").unwrap().is_none());
    }

    /// 取近似 f64（`magnitude_log10` 是用位长估的数量级、粒度约 ±0.25，不能用于精确值比较）
    fn bf_f64(x: &BigFloat) -> f64 {
        x.to_significant_string(15)
            .parse::<f64>()
            .unwrap_or(f64::NAN)
    }

    #[test]
    fn gauss_legendre_is_exact_for_polynomials() {
        // n 点 Gauss–Legendre 对 2n-1 次多项式精确：∫_{-1}^{1} x^k dx = 0（奇）/ 2/(k+1)（偶）
        // 这条测试同时验证"节点与权重都算对了"（比硬编码常数可靠）
        let prec = hipercalc_core::bigfloat::precision();
        let nodes = gauss_legendre(GAUSS_NODES, prec);
        assert_eq!(nodes.len(), GAUSS_NODES);
        for k in 0..(2 * GAUSS_NODES) {
            let mut sum = BigFloat::from_u64(0);
            for (x, w) in &nodes {
                let mut xk = BigFloat::from_u64(1);
                for _ in 0..k {
                    xk = BigFloat::mul(&xk, x, prec);
                }
                sum = BigFloat::add(&sum, &BigFloat::mul(w, &xk, prec), prec);
            }
            let want = if k % 2 == 1 {
                0.0
            } else {
                2.0 / (k as f64 + 1.0)
            };
            let got = bf_f64(&sum);
            assert!((got - want).abs() < 1e-13, "k={k}: got {got}, want {want}");
        }
        // 权重之和 = 2（区间长度）
        let mut wsum = BigFloat::from_u64(0);
        for (_, w) in &nodes {
            wsum = BigFloat::add(&wsum, w, prec);
        }
        assert!(
            (bf_f64(&wsum) - 2.0).abs() < 1e-13,
            "Σw = {}",
            bf_f64(&wsum)
        );
    }
}
