use num_bigint::BigInt;
use num_traits::Signed;

use crate::parser::{Evaluator, Expr};
use crate::solver_linear;
use hipercalc_core::bigfloat::{self, BigFloat};
use hipercalc_core::number::Number;

/// 发散判据：|x| 超过 1e6 即认为牛顿发散，放弃该初值。
/// 指数/对数型方程（如 e^x+y=1）在某些初值处修正量会爆炸，不拦截会让 exp 的
/// 中间量位数失控（卡死）。
const NEWTON_ABS_LIMIT: i64 = 1_000_000;

/// |b| > limit ?
fn exceeds_abs_limit(b: &BigFloat, limit: i64) -> bool {
    b.value.abs() > BigInt::from(limit) * BigInt::from(10).pow(b.precision as u32)
}

/// 在给定点求各方程残差（F_i 值）。任一方程求值失败返回 None。
fn eval_residuals(
    evaluator: &Evaluator,
    equations: &[Expr],
    vars: &[char],
    x: &[BigFloat],
) -> Option<Vec<BigFloat>> {
    let xnums: Vec<Number> = x.iter().map(|b| Number::Approx(b.clone())).collect();
    let substs: Vec<(String, &Number)> = vars
        .iter()
        .zip(xnums.iter())
        .map(|(v, num)| (v.to_string(), num))
        .collect();
    let res: Result<Vec<BigFloat>, String> = equations
        .iter()
        .map(|e| {
            evaluator
                .evaluate_with_vars(e, &substs)
                .map(|n| n.to_approx())
        })
        .collect();
    res.ok()
}

/// 两个解是否相同（各分量差小于显示精度量级：显示 20 位 → 1e-12）
fn same_solution(a: &[BigFloat], b: &[BigFloat]) -> bool {
    let tol_exp = bigfloat::display_digits().div_ceil(2) + 2;
    let eps = BigFloat::div(
        &BigFloat::from_u64(1),
        &BigInt::from(10).pow(tol_exp as u32).into(),
        bigfloat::precision(),
    );
    a.iter()
        .zip(b.iter())
        .all(|(x, y)| BigFloat::sub(x, y, bigfloat::precision()).value.abs() <= eps.value.abs())
}

/// 从单个初值做多维牛顿，返回收敛解（或 None）。
/// max_iter 为迭代上限；tol_exp 控制收敛容差（10^-tol_exp）；
/// forward 为 true 时雅可比用前向差分（粗扫提速）。
fn newton_once(
    evaluator: &Evaluator,
    equations: &[Expr],
    vars: &[char],
    x0: &[BigFloat],
    max_iter: usize,
    tol_exp: u32,
    forward: bool,
) -> Option<Vec<BigFloat>> {
    let n = vars.len();
    let neq = equations.len();
    let mut x: Vec<BigFloat> = x0.to_vec();

    let h = BigFloat::div(
        &BigFloat::from_u64(1),
        &BigFloat::from_u64(1000000000),
        bigfloat::precision(),
    );
    let tol = BigFloat::div(
        &BigFloat::from_u64(1),
        &BigInt::from(10).pow(tol_exp).into(),
        bigfloat::precision(),
    );

    for _ in 0..max_iter {
        // 残差（奇异/求值失败则放弃该初值）
        let f = eval_residuals(evaluator, equations, vars, &x)?;

        // 雅可比：前向差分（粗扫用，每变量 1 次求值）或中心差分（精收敛用）
        let mut jac: Vec<Vec<Number>> = vec![vec![Number::from_int(0); n]; neq];
        for j in 0..n {
            if forward {
                let mut xp = x.clone();
                xp[j] = BigFloat::add(&xp[j], &h, bigfloat::precision());
                let fp = eval_residuals(evaluator, equations, vars, &xp)?;
                for i in 0..neq {
                    let df = BigFloat::sub(&fp[i], &f[i], bigfloat::precision());
                    jac[i][j] = Number::Approx(BigFloat::div(&df, &h, bigfloat::precision()));
                }
            } else {
                let mut xp = x.clone();
                xp[j] = BigFloat::add(&xp[j], &h, bigfloat::precision());
                let fp = eval_residuals(evaluator, equations, vars, &xp)?;
                let mut xm = x.clone();
                xm[j] = BigFloat::sub(&xm[j], &h, bigfloat::precision());
                let fm = eval_residuals(evaluator, equations, vars, &xm)?;
                let two_h = BigFloat::from_u64(2) * h.clone();
                for i in 0..neq {
                    let df = BigFloat::sub(&fp[i], &fm[i], bigfloat::precision());
                    jac[i][j] = Number::Approx(BigFloat::div(&df, &two_h, bigfloat::precision()));
                }
            }
        }

        // 解 J·δ = −F
        let mut aug: Vec<Vec<Number>> = Vec::new();
        for i in 0..neq {
            let mut row = jac[i].clone();
            row.push(Number::Approx(BigFloat::neg(&f[i])));
            aug.push(row);
        }
        let ls = match solver_linear::gaussian_elimination(&mut aug, vars) {
            Some(ls) => ls,
            None => return None,
        };
        if !ls.unique {
            return None; // 奇异雅可比
        }

        // x += δ
        let mut max_step = BigFloat::from_u64(0);
        for (v, dv) in &ls.values {
            if let Some(pos) = vars.iter().position(|c| c == v) {
                let d = dv.to_approx();
                x[pos] = BigFloat::add(&x[pos], &d, bigfloat::precision());
                if d.value.abs() > max_step.value.abs() {
                    max_step = d;
                }
            }
        }
        // 发散保护：任一分量跑飞即放弃该初值
        if x.iter().any(|b| exceeds_abs_limit(b, NEWTON_ABS_LIMIT)) {
            return None;
        }

        // 收敛判定
        let loose_tol = BigFloat::div(
            &BigFloat::from_u64(1),
            &BigInt::from(10).pow(25).into(),
            bigfloat::precision(),
        );
        let f1 = eval_residuals(evaluator, equations, vars, &x);
        if let Some(f1) = &f1 {
            if f1.iter().all(|fi| fi.value.abs() <= tol.value.abs()) {
                return Some(x);
            }
        }
        if max_step.value.abs() <= tol.value.abs() {
            // 步长极小但残差未必达标（可能落在平坦区/局部极小），必须再校验残差，
            // 否则会返回"假根"（旧实现直接返回 x）。
            if let Some(f1) = &f1 {
                if f1.iter().all(|fi| fi.value.abs() <= loose_tol.value.abs()) {
                    return Some(x);
                }
            }
        }
    }
    None // 迭代超限未收敛，放弃该初值
}

/// 数值求解非线性方程组 F_i(vars) = 0。
/// equations 均为已移项为 0 的表达式；vars 顺序即未知量顺序。
/// 返回所有收敛到的解（网格初值遍历，多解去重）。
pub fn solve_system(
    evaluator: &Evaluator,
    equations: &[Expr],
    vars: &[char],
) -> Vec<Vec<BigFloat>> {
    let n = vars.len();
    // 网格初值：变量少时用更密集（含 0.5 步长）以捕捉更多解，变量多时用稀疏网格保持性能
    let half = BigFloat::div(
        &BigFloat::from_u64(1),
        &BigFloat::from_u64(2),
        bigfloat::precision(),
    );
    let grid: Vec<BigFloat> = if n <= 2 {
        vec![
            BigFloat::from_i64(-2),
            BigFloat::from_i64(-1),
            BigFloat::mul(&BigFloat::from_i64(-1), &half, bigfloat::precision()),
            BigFloat::from_u64(0),
            half.clone(),
            BigFloat::from_u64(1),
            BigFloat::from_u64(2),
        ]
    } else {
        vec![
            BigFloat::from_i64(-2),
            BigFloat::from_i64(-1),
            BigFloat::from_u64(0),
            BigFloat::from_u64(1),
            BigFloat::from_u64(2),
        ]
    };
    // 两阶段：先粗扫（少迭代、松容差）收集候选，再对候选精收敛到高精度
    let mut candidates: Vec<Vec<BigFloat>> = Vec::new();

    // 网格初值的笛卡尔积（共 grid.len()^n 个初值）
    let total = grid.len().pow(n as u32);
    let mut idx = vec![0usize; n];
    for _ in 0..total {
        let guess: Vec<BigFloat> = idx.iter().map(|&k| grid[k].clone()).collect();
        // 粗扫：迭代少、容差为显示精度量级（默认 16 次、1e-12）
        let rough_tol_exp = (bigfloat::display_digits().div_ceil(2) + 2) as u32;
        if let Some(c) = newton_once(evaluator, equations, vars, &guess, 16, rough_tol_exp, true) {
            if !candidates.iter().any(|r| same_solution(r, &c)) {
                candidates.push(c);
            }
        }
        // 进位（grid.len() 进制计数，遍历全部组合后自然结束）
        let mut pos = n;
        while pos > 0 {
            pos -= 1;
            idx[pos] += 1;
            if idx[pos] < grid.len() {
                break;
            }
            idx[pos] = 0;
        }
    }

    // 精收敛阶段
    let mut results: Vec<Vec<BigFloat>> = Vec::new();
    for c in &candidates {
        // 精收敛：全精度、容差取精度的一半再加 5（默认 80 位 → 1e-45）
        let fine_tol_exp = (bigfloat::precision().div_ceil(2) + 5) as u32;
        if let Some(sol) = newton_once(
            evaluator,
            equations,
            vars,
            c,
            bigfloat::precision(),
            fine_tol_exp,
            false,
        ) {
            if !results.iter().any(|r| same_solution(r, &sol)) {
                results.push(sol);
            }
        }
    }
    results
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;
    use hipercalc_core::number::Number;

    fn eq(s: &str) -> Expr {
        Parser::new(s).parse_expression().expect("解析方程失败")
    }

    /// `BigFloat` 是 `value / 10^precision` 的**定点**表示 ⇒ 判"|x| 很小"必须同时看两个字段，
    /// 直接拿 `value` 比大小会把 `2.0`（即 2×10^80）当成天文数字（这个坑我自己先踩了一次）。
    fn is_tiny(b: &BigFloat, digits: usize) -> bool {
        b.value.abs() < BigInt::from(10).pow((b.precision - digits) as u32).into()
    }

    /// 判 |x| 是否小于 10^exp
    fn below(b: &BigFloat, exp: i64) -> bool {
        let limit = if exp >= 0 {
            BigInt::from(10).pow(exp as u32)
        } else {
            BigInt::from(1)
        } * BigInt::from(10).pow(b.precision as u32);
        b.value.abs() < limit.into()
    }

    /// 把解代回各方程算残差 —— **测试自己验证**，不信任求解器内部的"找到了根"判定
    fn residuals_ok(ev: &Evaluator, eqs: &[Expr], vars: &[char], x: &[BigFloat]) -> bool {
        let nums: Vec<Number> = x.iter().map(|b| Number::Approx(b.clone())).collect();
        let substs: Vec<(String, &Number)> = vars
            .iter()
            .zip(nums.iter())
            .map(|(v, n)| (v.to_string(), n))
            .collect();
        eqs.iter().all(|e| {
            ev.evaluate_with_vars(e, &substs)
                .map(|n| !n.is_complex() && is_tiny(&n.to_approx(), 6))
                .unwrap_or(false)
        })
    }

    #[test]
    fn circle_meets_line_in_two_points() {
        // x² + y² = 25, y = x + 1 ⇒ x² + (x+1)² = 25 ⇒ x² + x - 12 = 0 ⇒ x = 3 或 -4
        // 对应 (3,4) 与 (-4,-3)；两解都满足 x²+y²=25
        let ev = Evaluator::new();
        let eqs = [eq("x^2+y^2-25"), eq("y-x-1")];
        let sols = solve_system(&ev, &eqs, &['x', 'y']);
        assert_eq!(sols.len(), 2, "应恰有两个交点: {sols:?}");
        for s in &sols {
            assert!(
                residuals_ok(&ev, &eqs, &['x', 'y'], s),
                "解不满足方程: {s:?}"
            );
        }
    }

    #[test]
    fn unique_solution_is_one_one() {
        // x + y = 2, x - y = 0 ⇒ x = y = 1
        let ev = Evaluator::new();
        let eqs = [eq("x+y-2"), eq("x-y")];
        let sols = solve_system(&ev, &eqs, &['x', 'y']);
        assert_eq!(sols.len(), 1, "应唯一解: {sols:?}");
        assert!(residuals_ok(&ev, &eqs, &['x', 'y'], &sols[0]));
    }

    #[test]
    fn no_real_solution_returns_empty() {
        // x² + y² = -1 在实数上无解（配合 y = x 亦无解）
        let ev = Evaluator::new();
        let eqs = [eq("x^2+y^2+1"), eq("y-x")];
        assert!(solve_system(&ev, &eqs, &['x', 'y']).is_empty());
    }

    #[test]
    fn three_variables_are_solved() {
        // x+y+z=6, x-y=1, y-z=0 ⇒ 由后两式 y=z、x=y+1；代入第一式 3y+1=6 ⇒ y=z=5/3, x=8/3
        let ev = Evaluator::new();
        let eqs = [eq("x+y+z-6"), eq("x-y-1"), eq("y-z")];
        let sols = solve_system(&ev, &eqs, &['x', 'y', 'z']);
        assert!(!sols.is_empty(), "应有解");
        assert!(residuals_ok(&ev, &eqs, &['x', 'y', 'z'], &sols[0]));
    }

    #[test]
    fn transcendental_pair_stays_finite() {
        // exp(x) = y, y = 2 ⇒ x = ln2；发散保护下必须收敛到有限值而不是跑飞
        let ev = Evaluator::new();
        let eqs = [eq("exp(x)-y"), eq("y-2")];
        let sols = solve_system(&ev, &eqs, &['x', 'y']);
        assert!(!sols.is_empty(), "应有解");
        for s in &sols {
            assert!(
                s.iter().all(|b| below(b, 3)),
                "解的分量应有限（|x| < 1000）: {s:?}"
            );
            assert!(residuals_ok(&ev, &eqs, &['x', 'y'], s));
        }
    }
}
