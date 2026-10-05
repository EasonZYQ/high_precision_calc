//! 符号结果的**规范化**：把 `Expr` 归结为"若干项之和"，每一项是 `系数 · Π 底^指数`。
//!
//! 目的只有一个——让 `diff(x^2, x)` 输出 `2*x` 而不是 `1*2*x^1 + 0`。
//! 这是**项目私有**的轻量化简，不是通用 CAS：
//!
//! - **做**：常量折叠、`0/+1/*1` 消去、同类项合并、同底指数相加、负指数收进分母、确定性排序；
//! - **不做**：三角恒等式（`sin(x)^2 + cos(x)^2` 不合并）、`exp(ln x)` 之类逆运算化简。
//!
//! 中间表示只存在于本模块，**不碰 `parser::Expr`**（那边的穷尽 match 有 12 处）。

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, Zero};

use hipercalc_core::number::Number;
use crate::parser::{BinOp, Expr, UnaryOp};

use super::{max_terms, ERROR_TOO_MANY_TERMS};

/// 一项：`coeff · Π base^exp`
#[derive(Clone)]
struct NTerm {
    coeff: Number,
    /// (底, 指数)；指数始终是精确有理数
    factors: Vec<(Expr, Number)>,
}

/// 简化一个表达式（主要用于求导/积分/Taylor 的结果）
pub fn simplify(ev: &crate::parser::Evaluator, e: &Expr) -> Result<Expr, String> {
    let terms = to_terms(ev, e)?;
    from_terms(terms)
}

/// 递归展开为项列表（会做分配律展开，必要时按护栏报错）
fn to_terms(ev: &crate::parser::Evaluator, e: &Expr) -> Result<Vec<NTerm>, String> {
    match e {
        Expr::Number(n) => {
            if n.is_zero() {
                Ok(Vec::new())
            } else {
                Ok(vec![NTerm {
                    coeff: n.clone(),
                    factors: Vec::new(),
                }])
            }
        }
        Expr::Variable(v) => Ok(vec![NTerm {
            coeff: Number::from_int(1),
            factors: vec![(Expr::Variable(v.clone()), Number::from_int(1))],
        }]),
        Expr::Function(..) => {
            // 无自由变量 ⇒ 折叠成常数（如 sin(1)、ln(2)）
            if let Some(n) = fold_constant(ev, e) {
                if n.is_zero() {
                    return Ok(Vec::new());
                }
                return Ok(vec![NTerm {
                    coeff: n,
                    factors: Vec::new(),
                }]);
            }
            Ok(vec![NTerm {
                coeff: Number::from_int(1),
                factors: vec![(e.clone(), Number::from_int(1))],
            }])
        }
        Expr::Pow(base, exp) => to_terms_pow(ev, base, exp),
        Expr::Unary(UnaryOp::Pos, x) => to_terms(ev, x),
        Expr::Unary(UnaryOp::Neg, x) => {
            let mut ts = to_terms(ev, x)?;
            for t in ts.iter_mut() {
                t.coeff = t.coeff.neg();
            }
            Ok(drop_zero(ts))
        }
        Expr::Binary(l, op, r) => match op {
            BinOp::Add => {
                let mut a = to_terms(ev, l)?;
                a.extend(to_terms(ev, r)?);
                check_size(&a)?;
                Ok(a)
            }
            BinOp::Sub => {
                let mut a = to_terms(ev, l)?;
                let mut b = to_terms(ev, r)?;
                for t in b.iter_mut() {
                    t.coeff = t.coeff.neg();
                }
                a.extend(b);
                check_size(&a)?;
                Ok(drop_zero(a))
            }
            BinOp::Mul => {
                let a = to_terms(ev, l)?;
                let b = to_terms(ev, r)?;
                check_size(&a)?;
                check_size(&b)?;
                // 含"函数底"（`sin(x)`、`x^x`）时**不做分配律展开**：
                // `x^x*(ln(x)+1)` 展开成 `x^x*ln(x) + x^x` 反而更难读，
                // 而多项式（`(x+1)*(x+2)`）就该展开成 `x^2 + 3*x + 2`。
                if has_opaque_base(&a) || has_opaque_base(&b) {
                    let ta = wrap_terms(a)?;
                    let tb = wrap_terms(b)?;
                    return Ok(drop_zero(vec![mul_terms(&ta, &tb)?]));
                }
                let mut out = Vec::with_capacity(a.len().saturating_mul(b.len()));
                for x in &a {
                    for y in &b {
                        out.push(mul_terms(x, y)?);
                        check_size(&out)?;
                    }
                }
                Ok(drop_zero(out))
            }
            BinOp::Div => {
                let a = to_terms(ev, l)?;
                let b = to_terms(ev, r)?;
                // 除以"常数项"时直接除系数（保持精确）
                if b.len() == 1 && b[0].factors.is_empty() {
                    let d = &b[0].coeff;
                    if d.is_zero() {
                        return Err("除以零错误".to_string());
                    }
                    let mut out = Vec::with_capacity(a.len());
                    for t in &a {
                        ok_div_exact(&t.coeff, d)?;
                        out.push(NTerm {
                            coeff: hipercalc_core::number::Number::div(&t.coeff, d),
                            factors: t.factors.clone(),
                        });
                    }
                    return Ok(drop_zero(out));
                }
                // 否则：× (除数)^(-1)。
                //
                // **除数含多项时不把分子拆开**（`num/re` 保持一项）：否则 `2*(1-x)` 会先被
                // 分配成 `2 - 2x`，再各乘一遍 `(1-x)^-4`，下次求导项数继续翻倍 ——
                // 实测 `taylor(1/(1-x), x, 0, 5)` 就是这样指数膨胀到撞护栏的。
                // 除数只有一项（单项式）时分配是安全的（`(x+1)/x → 1 + 1/x`）。
                let neg_one = Expr::Number(Number::from_int(-1));
                let inv = to_terms_pow(ev, r, &neg_one)?;
                check_size(&inv)?;
                let left: Vec<NTerm> = if b.len() > 1 {
                    vec![wrap_terms(a)?]
                } else {
                    a
                };
                let mut out = Vec::new();
                for x in &left {
                    for y in &inv {
                        out.push(mul_terms(x, y)?);
                        check_size(&out)?;
                    }
                }
                Ok(drop_zero(out))
            }
        },
        // 包装节点不该进入符号层（mod.rs 会提前拦截）
        Expr::Sd(x) | Expr::Factor(x) => to_terms(ev, x),
        Expr::Equation(..) | Expr::System(..) => Err("等式/方程组不能出现在符号化简中".to_string()),
    }
}

fn to_terms_pow(
    ev: &crate::parser::Evaluator,
    base: &Expr,
    exp: &Expr,
) -> Result<Vec<NTerm>, String> {
    // 嵌套幂先合并：`((1-x)^2)^2 = (1-x)^4`（两个指数都是整数时）。
    // **不合并会出大问题**：两个底的字面不同 ⇒ 同底指数永远合不上 ⇒ 反复求导时
    // 会留下 `(1-x)^2^2^2…` 这种畸形节点、表达式指数膨胀直到撞护栏
    // （实测 `taylor(1/(1-x), x, 0, 4)` 会报"泰勒展开式过大"）。
    if let Expr::Pow(inner, e1) = base {
        if let (Some(r1), Some(r2)) = (constant_rational(ev, e1), constant_rational(ev, exp)) {
            if r1.is_integer() && r2.is_integer() {
                let combined = Expr::Number(Number::from_rational(r1 * r2));
                return to_terms_pow(ev, inner, &combined);
            }
        }
    }
    // 指数是常数
    if let Some(r) = constant_rational(ev, exp) {
        if r.is_integer() {
            let k = r.to_integer();
            if k.magnitude().bits() <= 8 {
                let kk: i64 = k.try_into().unwrap_or(0);
                if kk == 0 {
                    // u^0 = 1
                    return Ok(vec![NTerm {
                        coeff: Number::from_int(1),
                        factors: Vec::new(),
                    }]);
                }
                if kk.abs() <= 64 {
                    // 小整数幂：连乘展开
                    let base_terms = to_terms(ev, base)?;
                    check_size(&base_terms)?;
                    // **只有单项底才展开整数幂**：
                    //  - 展开 `(2*x)^3 → 8*x^3`、`x^-2 → 1/x^2` 是安全的、也是需要的；
                    //  - 但把 `(1-x)^4` 展开成 5 项、`(1-x)^8` 展开成 9 项，会让反复求导
                    //    的项数按倍数增长（实测 `taylor(1/(1-x), x, 0, 5)` 因此撞护栏），
                    //    而且 `(1 - x)^4` 本来就比展开式更好读 —— 多顶底一律保留为原子幂。
                    //  - 负幂对多顶底还有额外问题：逐项取倒数会把 `1` 与 `-x` 分别求倒数
                    //    （得到 `1 - 1/x` 这种垃圾）。
                    if base_terms.len() != 1 {
                        return Ok(vec![NTerm {
                            coeff: Number::from_int(1),
                            factors: vec![(base.clone(), Number::from_rational(r))],
                        }]);
                    }
                    let mut acc = vec![NTerm {
                        coeff: Number::from_int(1),
                        factors: Vec::new(),
                    }];
                    for _ in 0..kk.abs() {
                        let mut next = Vec::new();
                        for a in &acc {
                            for b in &base_terms {
                                next.push(mul_terms(a, b)?);
                                check_size(&next)?;
                            }
                        }
                        acc = next;
                    }
                    if kk < 0 {
                        for t in acc.iter_mut() {
                            if t.coeff.is_zero() {
                                return Err("除以零错误".to_string());
                            }
                            t.coeff = hipercalc_core::number::Number::div(&Number::from_int(1), &t.coeff);
                            for f in t.factors.iter_mut() {
                                f.1 = f.1.neg();
                            }
                        }
                    }
                    return Ok(drop_zero(acc));
                }
            }
        }
        // 有理指数（如 1/2、-3/2）或过大的整数指数：作为原子因子保留指数
        return Ok(vec![NTerm {
            coeff: Number::from_int(1),
            factors: vec![(base.clone(), Number::from_rational(r))],
        }]);
    }
    // 指数含变量（`x^x`、`f^g`）⇒ 整体作为原子因子
    Ok(vec![NTerm {
        coeff: Number::from_int(1),
        factors: vec![(
            Expr::Pow(Box::new(base.clone()), Box::new(exp.clone())),
            Number::from_int(1),
        )],
    }])
}

/// 项列表里是否有"不透明底"（函数调用、或含符号指数的幂）——有则不做分配律展开
fn has_opaque_base(ts: &[NTerm]) -> bool {
    ts.iter().any(|t| {
        t.factors
            .iter()
            .any(|(b, _)| matches!(b, Expr::Function(..) | Expr::Pow(..)))
    })
}

/// 把多顶表达式整体收成**单个**因子（原样保留其外观，不做展开）。
///
/// 多顶时先 `from_terms` 化简好再包起来——否则 `ln(x) + x*1/x` 这类内部垃圾
/// 会被原样封存在因子组里（`x^x * (1*ln(x) + x*1 / x)`）。
fn wrap_terms(ts: Vec<NTerm>) -> Result<NTerm, String> {
    if ts.len() == 1 {
        return Ok(ts.into_iter().next().unwrap());
    }
    let rebuilt = from_terms(ts)?;
    Ok(NTerm {
        coeff: Number::from_int(1),
        factors: vec![(rebuilt, Number::from_int(1))],
    })
}

/// 项 × 项：系数相乘、因子拼接（同底指数相加）
fn mul_terms(a: &NTerm, b: &NTerm) -> Result<NTerm, String> {
    let mut factors = a.factors.clone();
    for (base, exp) in &b.factors {
        if let Some(slot) = factors
            .iter_mut()
            .find(|(eb, _)| same_base(eb, base))
        {
            slot.1 = slot.1.add(exp);
        } else {
            factors.push((base.clone(), exp.clone()));
        }
    }
    Ok(NTerm {
        coeff: hipercalc_core::number::Number::mul(&a.coeff, &b.coeff),
        factors,
    })
}

/// 两个底是否"同一个"（用渲染串做规范化键，简单且确定）
fn same_base(a: &Expr, b: &Expr) -> bool {
    key_of(a) == key_of(b)
}

fn key_of(e: &Expr) -> String {
    super::render::render_expr(e, crate::parser::DisplayMode::LineIO)
}

/// 无自由变量时折叠成常数
fn fold_constant(ev: &crate::parser::Evaluator, e: &Expr) -> Option<Number> {
    ev.evaluate_with_vars(e, &[]).ok()
}

/// 常数表达式 → 精确有理数
fn constant_rational(ev: &crate::parser::Evaluator, e: &Expr) -> Option<BigRational> {
    fold_constant(ev, e)?.as_rational()
}

/// 除法可精确进行时返回 true（否则仍然继续，交给 Number::div 的兜底）
fn ok_div_exact(_a: &Number, _b: &Number) -> Result<(), String> {
    Ok(())
}

/// 去掉指数为 0 的因子与系数为 0 的项
fn drop_zero(mut ts: Vec<NTerm>) -> Vec<NTerm> {
    for t in ts.iter_mut() {
        t.factors.retain(|(_, e)| !e.is_zero());
    }
    ts.retain(|t| !t.coeff.is_zero());
    ts
}

fn check_size(ts: &[NTerm]) -> Result<(), String> {
    if ts.len() > max_terms() {
        return Err(ERROR_TOO_MANY_TERMS.to_string());
    }
    for t in ts {
        if t.factors.len() > max_terms() {
            return Err(ERROR_TOO_MANY_TERMS.to_string());
        }
    }
    Ok(())
}

/// 项列表 → 表达式：合并同类项、排序、重建
fn from_terms(mut terms: Vec<NTerm>) -> Result<Expr, String> {
    check_size(&terms)?;
    terms.retain(|t| !t.coeff.is_zero());

    // 同类项合并：因子多重集相同则系数相加
    let mut merged: Vec<NTerm> = Vec::new();
    for t in terms {
        let k = term_key(&t);
        if let Some(slot) = merged.iter_mut().find(|m| term_key(m) == k) {
            slot.coeff = slot.coeff.add(&t.coeff);
        } else {
            merged.push(t);
        }
    }
    merged.retain(|t| !t.coeff.is_zero());
    check_size(&merged)?;

    // 确定性排序：总次数降序，其次键升序
    merged.sort_by(|a, b| {
        let da = total_degree(a);
        let db = total_degree(b);
        db.partial_cmp(&da)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| term_key(a).cmp(&term_key(b)))
    });

    if merged.is_empty() {
        return Ok(Expr::Number(Number::from_int(0)));
    }

    let mut acc: Option<Expr> = None;
    for t in merged {
        let neg = t.coeff.is_negative();
        if acc.is_none() {
            // 首项：负号直接交给 build_term（负系数会渲染成 `-x^2` / `-1 / x^2`）
            acc = Some(build_term(t.coeff.clone(), &t.factors)?);
            continue;
        }
        let coeff = if neg { t.coeff.neg() } else { t.coeff.clone() };
        let body = build_term(coeff, &t.factors)?;
        let prev = acc.unwrap();
        acc = Some(Expr::Binary(
            Box::new(prev),
            if neg { BinOp::Sub } else { BinOp::Add },
            Box::new(body),
        ));
    }
    Ok(acc.unwrap())
}

/// 因子多重集的规范化键
fn term_key(t: &NTerm) -> String {
    let mut parts: Vec<String> = t
        .factors
        .iter()
        .map(|(b, e)| format!("{}^{}", key_of(b), exp_key(e)))
        .collect();
    parts.sort();
    parts.join("*")
}

fn exp_key(e: &Number) -> String {
    match e.as_rational() {
        Some(r) => format!("{}/{}", r.numer(), r.denom()),
        None => hipercalc_core::display::format_lineio(e),
    }
}

fn total_degree(t: &NTerm) -> f64 {
    t.factors
        .iter()
        .filter_map(|(_, e)| e.as_rational())
        .map(|r| {
            use num_traits::ToPrimitive;
            r.to_f64().unwrap_or(0.0)
        })
        .sum()
}

/// 由系数与因子构造单项：负指数收进分母
fn build_term(coeff: Number, factors: &[(Expr, Number)]) -> Result<Expr, String> {
    let mut num: Vec<(Expr, Number)> = Vec::new();
    let mut den: Vec<(Expr, Number)> = Vec::new();
    for (b, e) in factors {
        let Some(r) = e.as_rational() else {
            num.push((b.clone(), e.clone()));
            continue;
        };
        if r.is_zero() {
            continue;
        }
        if r.numer().is_negative() {
            den.push((b.clone(), Number::from_rational(-r)));
        } else {
            num.push((b.clone(), e.clone()));
        }
    }
    // 系数为 -1 且没有分母时省略系数，改用外层负号 ⇒ `-x^2` 而不是 `-1*x^2`；
    // 有分母时保留 ⇒ `-1 / x^2`
    let neg_one = coeff
        .as_rational()
        .map(|r| r == BigRational::from_integer(BigInt::from(-1)))
        .unwrap_or(false);
    let skip_coeff = is_one(&coeff) || (neg_one && den.is_empty() && !num.is_empty());
    // 分子：系数 + 正指数因子
    let mut head: Option<Expr> = if skip_coeff && !num.is_empty() {
        None
    } else {
        Some(Expr::Number(coeff))
    };
    for (b, e) in &num {
        let f = pow_or_atom(b, e)?;
        head = Some(match head {
            None => f,
            Some(h) => Expr::Binary(Box::new(h), BinOp::Mul, Box::new(f)),
        });
    }
    let mut body = match head {
        None => Expr::Number(Number::from_int(1)),
        Some(h) => h,
    };
    if !den.is_empty() {
        let mut d: Option<Expr> = None;
        for (b, e) in &den {
            let f = pow_or_atom(b, e)?;
            d = Some(match d {
                None => f,
                Some(prev) => Expr::Binary(Box::new(prev), BinOp::Mul, Box::new(f)),
            });
        }
        body = Expr::Binary(Box::new(body), BinOp::Div, Box::new(d.unwrap()));
    } else if neg_one && !num.is_empty() {
        body = Expr::Unary(UnaryOp::Neg, Box::new(body));
    }
    Ok(body)
}

/// 指数为 1 时直接用底，否则 `Pow`
fn pow_or_atom(base: &Expr, exp: &Number) -> Result<Expr, String> {
    if is_one(exp) {
        Ok(base.clone())
    } else {
        Ok(Expr::Pow(
            Box::new(base.clone()),
            Box::new(Expr::Number(exp.clone())),
        ))
    }
}

/// `Number` 是否等于 1（`Number` 没有 `PartialEq`，统一用这个判）
pub(crate) fn is_one(n: &Number) -> bool {
    n.as_rational().map(|r| r.is_one()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Evaluator;

    fn s(input: &str) -> String {
        let mut ev = Evaluator::new();
        let mut p = crate::parser::Parser::new(input);
        let e = p.parse_expression().unwrap();
        let r = simplify(&ev, &e).unwrap();
        let _ = &mut ev;
        super::super::render::render_expr(&r, crate::parser::DisplayMode::LineIO)
    }

    /// 用 MathIO 渲染：精确分数会显示成 `5 / 6` 而不是 20 位小数，便于断言
    fn sm(input: &str) -> String {
        let ev = Evaluator::new();
        let mut p = crate::parser::Parser::new(input);
        let e = p.parse_expression().unwrap();
        let r = simplify(&ev, &e).unwrap();
        super::super::render::render_expr(&r, crate::parser::DisplayMode::MathIO)
    }

    #[test]
    fn merges_like_terms_and_powers() {
        assert_eq!(s("x+x"), "2*x");
        assert_eq!(s("x*x"), "x^2");
        assert_eq!(s("2*x+3*x"), "5*x");
        assert_eq!(s("0*x+5"), "5");
        assert_eq!(s("sin(x)*sin(x)"), "sin(x)^2");
        assert_eq!(s("(x+1)*(x-1)"), "x^2 - 1");
        assert_eq!(s("x-x"), "0");
    }

    #[test]
    fn constant_folding() {
        assert_eq!(s("sin(0)"), "0");
        assert_eq!(s("2*3"), "6");
        // 精确分数在 MathIO 下保持分数形式
        assert_eq!(sm("1/2+1/3"), "5 / 6");
    }

    #[test]
    fn keeps_function_products_factored() {
        // 含函数底时不展开分配律，保持因式形式更易读
        assert_eq!(sm("x^x*(ln(x)+1)"), "x^x * (ln(x) + 1)");
        // 多项式则照常展开
        assert_eq!(sm("(x+1)*(x+2)"), "x^2 + 3*x + 2");
    }
}
