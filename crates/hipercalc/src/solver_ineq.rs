//! 不等式求解：多项式不等式与不等式组。
//!
//! # 核心思路
//!
//! 不等式本质是"**求根 + 定符号**"：把一侧移项成 `f(x) op 0`，求出 `f` 的所有实根，
//! 用根把实轴切成若干段；每段内符号不变 ⇒ 只要取一个测试点判符号，就知道整段的取舍。
//! 求根直接复用 `solver_poly`（因式分解/求根那套已经做过最难的活）。
//!
//! # 解集的表示
//!
//! 统一用**区间表示**（端点 + 开闭），因为它是通用记号、不需要翻译：
//! - `x^2 > 4` → `(-∞, -2) ∪ (2, +∞)`
//! - `x^2 <= 4` → `[-2, 2]`
//! - `x^2 != 4` → `(-∞, -2) ∪ (-2, 2) ∪ (2, +∞)`（开端点天然表达了"挖掉 ±2"）
//! - 无解 → `∅`
//!
//! 不等式组的解 = 各条解集的**交集**（区间逐个相交再合并）。

use crate::parser::{Evaluator, Expr};
use hipercalc_core::bigfloat::BigFloat;
use hipercalc_core::number::Number;
use num_rational::BigRational;
use num_traits::{Signed, Zero};

/// 比较运算符（与解析期脱糖时写入的编码一致）
pub const OP_LT: u8 = 0;
pub const OP_LE: u8 = 1;
pub const OP_GT: u8 = 2;
pub const OP_GE: u8 = 3;
pub const OP_NE: u8 = 4;

/// 区间端点：`(值, 是否闭)；值为 None 表示无穷`
#[derive(Clone, Debug)]
struct Bound(Option<BigRational>, bool);

/// 一个区间 `[lo, hi]`（端点各带开闭）
type Interval = (Bound, Bound);

/// 比较两个有理数
fn cmp(a: &BigRational, b: &BigRational) -> std::cmp::Ordering {
    if a < b {
        std::cmp::Ordering::Less
    } else if a == b {
        std::cmp::Ordering::Equal
    } else {
        std::cmp::Ordering::Greater
    }
}

/// 渲染端点：无穷用 ±∞
fn fmt_bound(b: &Bound) -> String {
    match &b.0 {
        None => "∞".to_string(), // 负无穷由调用方加符号
        Some(r) => {
            let n = Number::from_rational(r.clone());
            hipercalc_core::display::format_mathio(&n)
        }
    }
}

/// 渲染整个解集
pub fn render(intervals: &[Interval]) -> String {
    if intervals.is_empty() {
        return "∅".to_string();
    }
    intervals
        .iter()
        .map(|(lo, hi)| {
            let l = match &lo.0 {
                None => "-∞".to_string(),
                Some(_) => fmt_bound(lo),
            };
            let r = fmt_bound(hi);
            format!(
                "{}{}, {}{}",
                if lo.1 { "[" } else { "(" },
                l,
                r,
                if hi.1 { "]" } else { ")" }
            )
        })
        .collect::<Vec<_>>()
        .join(" ∪ ")
}

/// 找出表达式里的自变量（单字母、且不是 ans/inf 这类伪变量）。
/// 项目里方程走的是自己的变量提取，这里只需要"我的不等式作用在谁身上"。
pub fn find_var(e: &Expr) -> Option<char> {
    match e {
        Expr::Variable(n) => {
            let mut ch = n.chars();
            match (ch.next(), ch.next()) {
                (Some(c), None) if c.is_ascii_alphabetic() => Some(c),
                _ => None,
            }
        }
        Expr::Binary(a, _, b) => find_var(a).or_else(|| find_var(b)),
        Expr::Unary(_, a) | Expr::Sd(a) | Expr::Factor(a) => find_var(a),
        Expr::Pow(a, b) => find_var(a).or_else(|| find_var(b)),
        Expr::Function(_, args) => args.iter().find_map(find_var),
        _ => None,
    }
}

/// 求一元多项式不等式 `f op 0` 的解集。`f` 为多项式，`var` 为自变量。
fn solve_one(ev: &Evaluator, f: &Expr, op: u8, var: char) -> Result<Vec<Interval>, String> {
    let coeffs = crate::solver_poly::extract_polynomial(ev, f, var)
        .ok_or_else(|| format!("不等式目前只支持**多项式**（自变量 {var}）：{f:?} 不是多项式"))?;

    // 实根（去重后升序）。复根不参与分段。
    let mut roots: Vec<BigRational> = Vec::new();
    if !coeffs.iter().all(|c| c.is_zero()) {
        if let Ok(sols) = crate::solver_poly::solve_poly_full(&coeffs) {
            for s in sols {
                if let crate::solver_poly::PolySolution::Real(n) = s {
                    if let Some(r) = n.as_rational() {
                        if !roots.contains(&r) {
                            roots.push(r);
                        }
                    }
                }
            }
        }
    }
    roots.sort_by(cmp);

    // `!= 0`：解集 = 全体实数挖掉这些根 ⇒ 用开端点区间表达
    if op == OP_NE {
        if roots.is_empty() {
            return Ok(vec![(Bound(None, false), Bound(None, false))]);
        }
        let mut out: Vec<Interval> = Vec::new();
        out.push((Bound(None, false), Bound(Some(roots[0].clone()), false)));
        for w in roots.windows(2) {
            out.push((
                Bound(Some(w[0].clone()), false),
                Bound(Some(w[1].clone()), false),
            ));
        }
        let last = roots[roots.len() - 1].clone();
        out.push((Bound(Some(last), false), Bound(None, false)));
        // 去掉因重根产生的空区间
        out.retain(|(l, r)| match (&l.0, &r.0) {
            (Some(a), Some(b)) => a != b,
            _ => true,
        });
        return Ok(out);
    }

    // 逐段判符号：段与段之间由根分隔
    let mut out: Vec<Interval> = Vec::new();
    let n = roots.len();
    for seg in 0..=n {
        // 本段：lo_side 为 true 表示下界是该段的左端点（含开闭由 op 决定）
        let lo = if seg == 0 {
            Bound(None, false)
        } else {
            Bound(Some(roots[seg - 1].clone()), op == OP_LE || op == OP_GE)
        };
        let hi = if seg == n {
            Bound(None, false)
        } else {
            Bound(Some(roots[seg].clone()), op == OP_LE || op == OP_GE)
        };
        // 取段内测试点：无界段用 ±1 偏移，有界段用中点
        let test = match (lo.0.as_ref(), hi.0.as_ref()) {
            (None, None) => BigRational::from_integer(0.into()),
            (None, Some(b)) => b - BigRational::from_integer(1.into()),
            (Some(a), None) => a + BigRational::from_integer(1.into()),
            (Some(a), Some(b)) => (a + b) / BigRational::from_integer(2.into()),
        };
        let v = ev
            .evaluate_with_var(f, &var.to_string(), &Number::from_rational(test))
            .map_err(|e| format!("不等式求值失败: {e}"))?;
        let neg = sign_is_negative(&v).ok_or_else(|| "不等式在该段无法定符号".to_string())?;
        let take = match op {
            OP_LT => neg,
            OP_LE => neg,
            OP_GT => !neg,
            OP_GE => !neg,
            _ => unreachable!(),
        };
        // 全零多项式（恒 0）：`0 < 0` 无解、`0 <= 0` 全集
        if take {
            out.push((lo, hi));
        }
    }

    // 合并相邻且相连的区间（前一个的右端 = 后一个的左端且至少一侧闭合）
    let mut merged: Vec<Interval> = Vec::new();
    for cur in out {
        match merged.last_mut() {
            Some(prev) if bounds_touch(&prev.1, &cur.0) => {
                prev.1 = cur.1; // 向右延伸
            }
            _ => merged.push(cur),
        }
    }
    Ok(merged)
}

fn bounds_touch(a: &Bound, b: &Bound) -> bool {
    match (a.0.as_ref(), b.0.as_ref()) {
        (None, None) => true,
        (Some(x), Some(y)) => x == y && (a.1 || b.1),
        _ => false,
    }
}

/// 判断数值的符号：负 ⇒ Some(true)，正 ⇒ Some(false)，无法判定 ⇒ None
fn sign_is_negative(v: &Number) -> Option<bool> {
    if let Some(r) = v.as_rational() {
        return Some(r < BigRational::zero());
    }
    let bf = v.to_approx();
    let zero = BigFloat::from_int(&num_bigint::BigInt::from(0));
    let d = BigFloat::sub(&bf, &zero, 40);
    if d.value.is_negative() {
        Some(true)
    } else if d.value.is_zero() {
        None
    } else {
        Some(false)
    }
}

/// 两个区间求交
fn intersect(a: &Interval, b: &Interval) -> Option<Interval> {
    let lo = match (a.0.0.as_ref(), b.0.0.as_ref()) {
        (None, _) => b.0.clone(),
        (_, None) => a.0.clone(),
        (Some(x), Some(y)) => match cmp(x, y) {
            std::cmp::Ordering::Greater => a.0.clone(),
            std::cmp::Ordering::Less => b.0.clone(),
            // 相等：任一侧开 ⇒ 交也开
            std::cmp::Ordering::Equal => Bound(Some(x.clone()), a.0.1 && b.0.1),
        },
    };
    let hi = match (a.1.0.as_ref(), b.1.0.as_ref()) {
        (None, _) => b.1.clone(),
        (_, None) => a.1.clone(),
        (Some(x), Some(y)) => match cmp(x, y) {
            std::cmp::Ordering::Less => a.1.clone(),
            std::cmp::Ordering::Greater => b.1.clone(),
            std::cmp::Ordering::Equal => Bound(Some(x.clone()), a.1.1 && b.1.1),
        },
    };
    // 空集判定
    match (lo.0.as_ref(), hi.0.as_ref()) {
        (Some(x), Some(y)) => match cmp(x, y) {
            std::cmp::Ordering::Greater => None,
            std::cmp::Ordering::Equal if !(lo.1 && hi.1) => None,
            _ => Some((lo, hi)),
        },
        _ => Some((lo, hi)),
    }
}

/// 解不等式（组）。`items` 每项是 `(移项后的 f, 运算符)`，全部对同一个 `var`。
pub fn solve(ev: &Evaluator, items: &[(Expr, u8)], var: char) -> Result<String, String> {
    if items.is_empty() {
        return Err("没有可解的不等式".to_string());
    }
    let mut acc: Option<Vec<Interval>> = None;
    for (f, op) in items {
        let cur = solve_one(ev, f, *op, var)?;
        acc = Some(match acc {
            None => cur,
            Some(prev) => {
                // 与已有解集求交（两两相交，保留可行区间）
                let mut out: Vec<Interval> = Vec::new();
                for a in &prev {
                    for b in &cur {
                        if let Some(x) = intersect(a, b) {
                            out.push(x);
                        }
                    }
                }
                out
            }
        });
        if acc.as_ref().is_some_and(|v| v.is_empty()) {
            return Ok("∅".to_string());
        }
    }
    Ok(render(&acc.unwrap_or_default()))
}
