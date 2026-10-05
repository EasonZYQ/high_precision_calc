use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{Signed, ToPrimitive, Zero};

use hipercalc_core::bigfloat::{self, BigFloat};
use hipercalc_core::number::Number;
use crate::parser::{DisplayMode, Evaluator, Expr};

/// 高次数值求根被规模护栏拒绝时的错误前缀。
/// 调用方（main.rs::handle_equation）据此判断"这是明确拒绝"而不是"求根失败"，
/// 从而**不再回退牛顿单根**——高次多项式的牛顿求值代价极高（x^5000-1 每次求值都要算 5000 次幂），
/// 回退只会让界面长时间无响应，而且只能给出一个根。
pub const DEGREE_GUARD_PREFIX: &str = "数值求根次数过高";

/// 多项式求解结果
#[derive(Debug, Clone)]
pub enum PolySolution {
    /// 实数解
    Real(Number),
    /// 复数解 a + bi
    Complex(Number, Number),
}

/// 牛顿迭代的发散判据：|x| 或单步修正量超过该值即认为发散，放弃该初值。
/// 指数/对数型方程（如 e^x=2、2^x=8）在某些初值处残差与导数同时极小，
/// 修正量可达 1e8 量级，若不拦截会让后续 pow/exp 的中间量位数爆炸 ⇒ 进程卡死。
const NEWTON_ABS_LIMIT: i64 = 1_000_000;

/// |b| > limit ?
fn exceeds_abs_limit(b: &BigFloat, limit: i64) -> bool {
    b.value.abs() > BigInt::from(limit) * BigInt::from(10).pow(b.precision as u32)
}

/// 牛顿迭代法求单根
/// f: 目标函数 f(x)
/// df: 导数 f'(x) 或 None（自动用有限差分）
/// initial_guess: 初始猜测
pub fn newton_solve(
    evaluator: &Evaluator,
    f_expr: &Expr,
    var: char,
    initial_guess: BigFloat,
    max_iter: usize,
) -> Option<BigFloat> {
    if exceeds_abs_limit(&initial_guess, NEWTON_ABS_LIMIT) {
        return None;
    }
    let mut x = initial_guess;
    // 有限差分步长：默认精度下取 1e-9；精度很低时 1e-9 已低于可表示精度，
    // 按 precision/8 缩小指数（80 位仍是 1e-9，行为不变）
    let h_digits = 9usize.min((bigfloat::precision() / 8).max(2));
    let h = BigFloat::div(
        &BigFloat::from_u64(1),
        &BigInt::from(10).pow(h_digits as u32).into(),
        bigfloat::precision(),
    );
    // 收敛判据阈值（不随迭代变化，循环外算一次即可）
    let zero_compare = BigFloat::div(
        &BigFloat::from_u64(1),
        &BigInt::from(10).pow(bigfloat::precision().div_ceil(2) as u32 + 10).into(),
        bigfloat::precision(),
    );

    for _ in 0..max_iter {
        // 中断检查：本函数返回 Option，故中断表现为"放弃这个初值"（外层循环另有检查点）
        hipercalc_core::cancel::check().ok()?;
        // 计算 f(x)
        // 复数结果（如 `ln(-1)`、`sqrt(-4)`）说明该初值落在实数域之外：
        // `Number::to_approx` 对复数会 debug_assert（不能静默丢虚部）⇒ 必须先分流、当作无效初值放弃。
        let fx = match evaluator.evaluate_with_var(f_expr, &var.to_string(), &Number::Approx(x.clone())) {
            Ok(v) if !v.is_complex() => v.to_approx(),
            _ => return None,
        };

        // 检查与 0 的接近程度
        if fx.value.abs() <= zero_compare.value.abs()
            || (fx.value.abs() <= BigInt::from(1) && fx.precision >= 60)
        {
            return Some(x);
        }

        // 中心差分近似导数 f'(x) ≈ (f(x+h) - f(x-h)) / (2h)
        let x_plus = BigFloat::add(&x, &h, bigfloat::precision());
        let x_minus = BigFloat::sub(&x, &h, bigfloat::precision());

        let f_plus = match evaluator.evaluate_with_var(f_expr, &var.to_string(), &Number::Approx(x_plus)) {
            Ok(v) if !v.is_complex() => v.to_approx(),
            _ => return None,
        };
        let f_minus = match evaluator.evaluate_with_var(f_expr, &var.to_string(), &Number::Approx(x_minus)) {
            Ok(v) if !v.is_complex() => v.to_approx(),
            _ => return None,
        };

        let df = BigFloat::sub(&f_plus, &f_minus, bigfloat::precision());
        let two_h = BigFloat::mul(
            &BigFloat::from_u64(2),
            &h,
            bigfloat::precision(),
        );
        let derivative = BigFloat::div(&df, &two_h, bigfloat::precision());

        // 导数为零 ⇒ 该初值不可用。**必须显式判零**：`BigFloat::div` 内部是 `assert!`，除数为零会
        // 直接把进程 panic 掉（实测 `/mode prec 40` 下 `sin(x)=0` 崩溃）。
        // 下面那条"导数极小"的判断带 `precision >= 60` 的精度闸门，是按默认精度 80 调的，
        // 在 prec < 60 时恒不成立，**不能**指望它兜住精确零。
        if derivative.is_zero()
            || (derivative.value.abs() <= BigInt::from(1) && derivative.precision >= 60)
        {
            return None; // 导数为零
        }

        let correction = BigFloat::div(&fx, &derivative, bigfloat::precision());
        let x_new = BigFloat::sub(&x, &correction, bigfloat::precision());

        // 发散保护：修正量或新点越界即放弃该初值（见 NEWTON_ABS_LIMIT 注释）
        if exceeds_abs_limit(&correction, NEWTON_ABS_LIMIT)
            || exceeds_abs_limit(&x_new, NEWTON_ABS_LIMIT)
        {
            return None;
        }

        // 修正量足够小则收敛
        if correction.value.abs() <= zero_compare.value.abs() {
            return Some(x_new);
        }

        x = x_new;
    }

    // 迭代未在循环内确认收敛：校验最终残差，避免返回假根
    match evaluator.evaluate_with_var(f_expr, &var.to_string(), &Number::Approx(x.clone())) {
        Ok(v) if !v.is_complex() => {
            let bf = v.to_approx();
            if bf.value.abs() <= BigInt::from(10).pow(bigfloat::precision().div_ceil(2) as u32).into()
            {
                Some(x.rounded(bigfloat::precision()))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// 从 Expr 中提取多项式系数（按升幂排列: coeffs[i] = x^i 的系数）
pub fn extract_polynomial(
    evaluator: &Evaluator,
    expr: &Expr,
    var: char,
) -> Option<Vec<Number>> {
    // 先判断是否多项式：仅含常数、变量、+-*^(整数)
    if !crate::equation::is_polynomial(expr) {
        return None;
    }

    // 尝试直接提取系数
    extract_coeffs_direct(evaluator, expr, var)
}

/// 直接从 AST 提取多项式系数
fn extract_coeffs_direct(
    evaluator: &Evaluator,
    expr: &Expr,
    var: char,
) -> Option<Vec<Number>> {
    // 收集所有项: (系数, 次数)
    let mut terms: Vec<(Number, u32)> = Vec::new();
    collect_terms(evaluator, expr, var, &mut terms)?;

    if terms.is_empty() {
        return Some(vec![Number::from_int(0)]);
    }

    // 找到最高次数
    let max_deg = terms.iter().map(|(_, d)| *d).max().unwrap_or(0);

    // 构建系数数组
    let mut coeffs = vec![Number::from_int(0); (max_deg + 1) as usize];
    for (coeff, deg) in terms {
        coeffs[deg as usize] = coeffs[deg as usize].add(&coeff);
    }

    Some(coeffs)
}

/// 递归收集项
fn collect_terms(
    evaluator: &Evaluator,
    expr: &Expr,
    var: char,
    terms: &mut Vec<(Number, u32)>,
) -> Option<()> {
    match expr {
        Expr::Number(n) => {
            terms.push((n.clone(), 0));
            Some(())
        }
        Expr::Variable(name) => {
            if name.len() == 1 && name.chars().next().unwrap() == var {
                terms.push((Number::from_int(1), 1));
            } else if name == "ans" {
                // ans 取上一次结果的实际值，而不是常数 0
                terms.push((evaluator.ans.clone(), 0));
            } else if let Some(val) = evaluator.vars.get(name) {
                // 已存储的变量（/let，全大写）作为常数项
                terms.push((val.clone(), 0));
            } else {
                return None;
            }
            Some(())
        }
        Expr::Binary(left, op, right) => {
            let mut left_terms = Vec::new();
            let mut right_terms = Vec::new();
            collect_terms(evaluator, left, var, &mut left_terms)?;
            collect_terms(evaluator, right, var, &mut right_terms)?;

            match op {
                crate::parser::BinOp::Add => {
                    terms.extend(left_terms);
                    terms.extend(right_terms);
                }
                crate::parser::BinOp::Sub => {
                    terms.extend(left_terms);
                    for (c, d) in right_terms {
                        terms.push((c.neg(), d));
                    }
                }
                crate::parser::BinOp::Mul => {
                    for (c1, d1) in &left_terms {
                        for (c2, d2) in &right_terms {
                            terms.push((c1.mul(c2), d1 + d2));
                        }
                    }
                }
                crate::parser::BinOp::Div => {
                    // 除以非零常数（如 x/2）：每项系数除以常数
                    if right_terms.iter().all(|(_, d)| *d == 0) {
                        let mut den = Number::from_int(0);
                        for (c, _) in &right_terms {
                            den = den.add(c);
                        }
                        if den.is_zero() {
                            return None;
                        }
                        for (c, d) in left_terms {
                            terms.push((c.div(&den), d));
                        }
                    } else {
                        return None;
                    }
                }
            }
            Some(())
        }
        Expr::Unary(op, e) => {
            let mut inner_terms = Vec::new();
            collect_terms(evaluator, e, var, &mut inner_terms)?;
            match op {
                crate::parser::UnaryOp::Pos => terms.extend(inner_terms),
                crate::parser::UnaryOp::Neg => {
                    for (c, d) in inner_terms {
                        terms.push((c.neg(), d));
                    }
                }
            }
            Some(())
        }
        Expr::Pow(base, exp) => {
            // 底数是变量且指数是常数整数（快捷路径）
            if let Expr::Variable(name) = base.as_ref() {
                if name.len() == 1 && name.chars().next().unwrap() == var {
                    if let Expr::Number(n) = exp.as_ref() {
                        if let Some(rat) = n.as_rational() {
                            if rat.is_integer() {
                                let deg_u32 = rat.to_integer().to_u32()?;
                                terms.push((Number::from_int(1), deg_u32));
                                return Some(());
                            }
                        }
                    }
                }
            }
            // 一般底数的小整数幂：展开为连乘（如 (x+1)^2）
            if let Expr::Number(n) = exp.as_ref() {
                if let Some(rat) = n.as_rational() {
                    if rat.is_integer() {
                        let k = rat.to_integer();
                        if k.is_zero() {
                            terms.push((Number::from_int(1), 0));
                            return Some(());
                        }
                        if let Some(k) = k.to_u32() {
                            if (2..=64).contains(&k) {
                                let mut base_terms = Vec::new();
                                collect_terms(evaluator, base, var, &mut base_terms)?;
                                let mut acc = base_terms.clone();
                                for _ in 1..k {
                                    let mut new_acc = Vec::new();
                                    for (c1, d1) in &acc {
                                        for (c2, d2) in &base_terms {
                                            new_acc.push((c1.mul(c2), d1 + d2));
                                        }
                                    }
                                    acc = new_acc;
                                }
                                terms.extend(acc);
                                return Some(());
                            }
                        }
                    }
                }
            }
            // 一般幂，不支持直接提取
            None
        }
        _ => None,
    }
}

/// 二次方程求解: ax^2 + bx + c = 0
pub fn solve_quadratic(a: &Number, b: &Number, c: &Number) -> Vec<PolySolution> {
    if a.is_zero() {
        // 退化为一元一次方程: bx + c = 0
        if b.is_zero() {
            return vec![];
        }
        let x = c.neg().div(b);
        return vec![PolySolution::Real(x)];
    }

    // 计算判别式 Δ = b^2 - 4ac
    let b2 = b.mul(b);
    let four = Number::from_int(4);
    let four_ac = four.mul(&a.mul(c));
    let discriminant = b2.sub(&four_ac);

    let two_a = Number::from_int(2).mul(a);
    let neg_b = b.neg();

    if discriminant.is_negative() {
        // 复数根
        let imag_part = discriminant.neg().sqrt(); // sqrt(-Δ)
        let real_part = neg_b.div(&two_a);
        let imag_part_div = imag_part.div(&two_a);
        vec![
            PolySolution::Complex(real_part.clone(), imag_part_div.clone()),
            PolySolution::Complex(real_part, imag_part_div.neg()),
        ]
    } else {
        // 实根
        let sqrt_d = discriminant.sqrt();
        let x1 = neg_b.add(&sqrt_d).div(&two_a);
        let x2 = neg_b.sub(&sqrt_d).div(&two_a);
        vec![PolySolution::Real(x1), PolySolution::Real(x2)]
    }
}

/// 格式化单个根。按显示模式输出：MathIO 下精确根用符号形式（分数/根式），
/// LineIO 下一律小数；近似根两种模式都给小数。`=`/`≈` 前缀由调用方按模式判定。
pub fn format_solution(sol: &PolySolution, mode: DisplayMode) -> String {
    match sol {
        PolySolution::Real(n) => format_number(n, mode),
        PolySolution::Complex(re, im) => {
            let re_str = format_number(re, mode);
            let im_str = format_number(im, mode);
            if im.is_negative() {
                format!("{} - {}i", re_str, format_number(&im.neg(), mode))
            } else {
                format!("{} + {}i", re_str, im_str)
            }
        }
    }
}

fn format_number(n: &Number, mode: DisplayMode) -> String {
    match mode {
        DisplayMode::MathIO => hipercalc_core::display::format_mathio(n),
        DisplayMode::LineIO => hipercalc_core::display::format_lineio(n),
    }
}

/* ---------------- 完整求根（分解逐因子） ---------------- */

fn number_to_rational(n: &Number) -> Option<BigRational> {
    match n {
        Number::Exact(expr) => expr
            .as_rational()
            .or_else(|| expr.as_integer().map(BigRational::from_integer)),
        Number::Approx(_) => None,
       Number::Complex(_) => None,
    }
}

/// 对一元多项式系数（升幂）求全部根：分解后逐因子求根（线性/二次精确，deg>=3 数值求全部复根）
pub fn solve_poly_full(coeffs: &[Number]) -> Result<Vec<PolySolution>, String> {
    // 转 BigRational
    let mut bc: Vec<BigRational> = Vec::new();
    for c in coeffs {
        match number_to_rational(c) {
            Some(r) => bc.push(r),
            None => return Err("高次求根需要精确的有理系数".to_string()),
        }
    }
    // 截掉高位零
    while bc.len() > 1 && bc[bc.len() - 1].is_zero() {
        bc.pop();
    }
    if bc.is_empty() {
        return Ok(Vec::new());
    }
    if bc.len() == 1 && bc[0].is_zero() {
        return Ok(Vec::new()); // 0 方程无根
    }

    let (factors, _) = crate::solver_factor::factor_univariate_coeffs(&bc);
    let mut sols: Vec<PolySolution> = Vec::new();
    for (fc, exp) in factors {
        let deg = fc.len() - 1;
        let roots: Vec<PolySolution> = match deg {
            1 => {
                let r = -&fc[0] / &fc[1];
                vec![PolySolution::Real(Number::from_rational(r))]
            }
            2 => {
                let nums: Vec<Number> = fc.iter().map(|c| Number::from_rational(c.clone())).collect();
                solve_quadratic(&nums[2], &nums[1], &nums[0])
            }
            // 高次不可约因子：数值求全部复数根
            _ => {
                let bfs: Vec<BigFloat> = fc.iter().map(BigFloat::from_big_rational).collect();
                durand_kerner(&bfs)?
                    .into_iter()
                    .map(complex_to_solution)
                    .collect()
            }
        };
        for s in roots {
            for _ in 0..exp {
                sols.push(s.clone());
            }
        }
    }
    Ok(sols)
}

/* ---------------- Durand-Kerner 数值复数求根 ---------------- */

/// 复高精度数（近似）
#[derive(Clone)]
struct Cx {
    re: BigFloat,
    im: BigFloat,
}

fn cx_add(a: &Cx, b: &Cx, prec: usize) -> Cx {
    Cx {
        re: BigFloat::add(&a.re, &b.re, prec),
        im: BigFloat::add(&a.im, &b.im, prec),
    }
}

fn cx_sub(a: &Cx, b: &Cx, prec: usize) -> Cx {
    Cx {
        re: BigFloat::sub(&a.re, &b.re, prec),
        im: BigFloat::sub(&a.im, &b.im, prec),
    }
}

fn cx_mul(a: &Cx, b: &Cx, prec: usize) -> Cx {
    let re = BigFloat::sub(
        &BigFloat::mul(&a.re, &b.re, prec),
        &BigFloat::mul(&a.im, &b.im, prec),
        prec,
    );
    let im = BigFloat::add(
        &BigFloat::mul(&a.re, &b.im, prec),
        &BigFloat::mul(&a.im, &b.re, prec),
        prec,
    );
    Cx { re, im }
}

/// 复数除法 a/b（|b| 为零返回 None）
fn cx_div(a: &Cx, b: &Cx, prec: usize) -> Option<Cx> {
    let den = BigFloat::add(
        &BigFloat::mul(&b.re, &b.re, prec),
        &BigFloat::mul(&b.im, &b.im, prec),
        prec,
    );
    if den.is_zero() {
        return None;
    }
    // a * conj(b) / |b|^2
    let num_re = BigFloat::add(
        &BigFloat::mul(&a.re, &b.re, prec),
        &BigFloat::mul(&a.im, &b.im, prec),
        prec,
    );
    let num_im = BigFloat::sub(
        &BigFloat::mul(&a.im, &b.re, prec),
        &BigFloat::mul(&a.re, &b.im, prec),
        prec,
    );
    Some(Cx {
        re: BigFloat::div(&num_re, &den, prec),
        im: BigFloat::div(&num_im, &den, prec),
    })
}

/// 复数 Horner 求值（实系数）
fn poly_eval_cx(coeffs: &[BigFloat], z: &Cx, prec: usize) -> Cx {
    let mut acc = Cx {
        re: BigFloat::from_u64(0),
        im: BigFloat::from_u64(0),
    };
    for c in coeffs.iter().rev() {
        acc = cx_add(
            &cx_mul(&acc, z, prec),
            &Cx {
                re: c.clone(),
                im: BigFloat::from_u64(0),
            },
            prec,
        );
    }
    acc
}

/// Durand-Kerner 单阶段迭代（就地更新 zs），返回是否在步数上限内收敛。
/// `prec` 为该阶段的运算精度，`tol` 为最大单步长阈值。
fn dk_iterate(
    coeffs: &[BigFloat],
    zs: &mut [Cx],
    prec: usize,
    tol: &BigFloat,
    max_iter: usize,
) -> bool {
    let n = zs.len();
    for _ in 0..max_iter {
        let mut max_step = BigFloat::from_u64(0);
        // 顺序更新（Gauss-Seidel 型）：算第 k 个根时用已更新的 z_1..z_{k-1}，
        // 收敛速度明显快于"全部同时更新"，这是 DK 的经典形式。
        for k in 0..n {
            let zk = zs[k].clone();
            let num = poly_eval_cx(coeffs, &zk, prec);
            let mut den = Cx {
                re: BigFloat::from_u64(1),
                im: BigFloat::from_u64(0),
            };
            for j in 0..n {
                if j != k {
                    den = cx_mul(&den, &cx_sub(&zk, &zs[j], prec), prec);
                }
            }
            if let Some(delta) = cx_div(&num, &den, prec) {
                let step_re = delta.re.value.abs();
                let step_im = delta.im.value.abs();
                zs[k] = cx_sub(&zk, &delta, prec);
                if max_step.value.abs() < step_re {
                    max_step = delta.re.clone();
                }
                if max_step.value.abs() < step_im {
                    max_step = delta.im.clone();
                }
            }
        }
        if max_step.value.abs() <= tol.value.abs() {
            return true;
        }
    }
    false
}

/// Durand-Kerner（Weierstrass）迭代：求实系数多项式全部复数根。
/// 返回 `Err` 表示迭代未在步数上限内收敛（调用方须放弃结果，不能把中间值当根输出）。
fn durand_kerner(coeffs: &[BigFloat]) -> Result<Vec<Cx>, String> {
    let n = coeffs.len() - 1;
    if n == 0 {
        return Ok(Vec::new());
    }
    // Fast 模式的次数护栏：DK 每轮 O(n²) 复数乘除，实测 n≈200 已需约 25 秒、
    // n≥300 超过 1 分钟。Deep 模式放开（死算由用户承担代价）。
    const DK_MAX_DEGREE_FAST: usize = 200;
    if n > DK_MAX_DEGREE_FAST && !hipercalc_core::calc_mode::is_deep() {
        return Err(format!(
            "{DEGREE_GUARD_PREFIX}（{n} 次，Fast 模式上限 {DK_MAX_DEGREE_FAST}）；\
             如确认需要继续，请先执行 /mode deep",
        ));
    }
    // 首一化
    let lead = coeffs[n].clone();
    let norm: Vec<BigFloat> = coeffs
        .iter()
        .map(|c| BigFloat::div(c, &lead, bigfloat::precision()))
        .collect();

    let tol = BigFloat::div(
        &BigFloat::from_u64(1),
        &BigInt::from(10).pow(bigfloat::precision().div_ceil(2) as u32).into(),
        bigfloat::precision(),
    );

    // 初值半径：
    // - 柯西界 R_c = 1 + max|a_i/a_n|（首一化后即 max|norm[i]|）是"所有根都在其内"的
    //   宽松上界，对稀疏多项式往往远大于真实根模（x^96+…+1 的 R_c = 2，而根全在单位圆上）；
    // - 根模的几何估计 R_g = |a_0/a_n|^(1/n)：对 x^n-1 这类单位根多项式给出 R_g ≈ 1，
    //   对 x^2-10^6 给出 R_g = 1000（贴合真实根模）。
    // 取 R = min(R_c, max(R_g, 1))：既贴近真实根模，又不会因常数项很小而把初值挤到原点。
    // 旧实现只用 R_c（或更早的 (0.4+0.9i)^k 几何衰减序列），初值显著偏离真实根，
    // 高次多项式迭代几百轮仍不收敛。
    let mut bound = BigFloat::from_u64(0);
    for c in norm.iter().take(n) {
        let abs_c = BigFloat {
            value: c.value.abs(),
            precision: c.precision,
        }
        .rounded(bigfloat::precision());
        if abs_c.value > bound.value {
            bound = abs_c;
        }
    }
    let cauchy = BigFloat::add(&BigFloat::from_u64(1), &bound, bigfloat::precision());
    let geom = if norm[0].is_zero() {
        None
    } else {
        let mut a0_abs = BigFloat {
            value: norm[0].value.abs(),
            precision: norm[0].precision,
        }
        .rounded(bigfloat::precision());
        // 下限保护：|a_0| < 1 时按 1 计，避免初值半径被压到原点附近
        let one = BigFloat::from_u64(1).rounded(bigfloat::precision());
        if a0_abs.value < one.value {
            a0_abs = one;
        }
        let inv_n = BigFloat::div(
            &BigFloat::from_u64(1),
            &BigFloat::from_u64(n as u64),
            bigfloat::precision(),
        );
        // 预检：|a₀|^(1/n) = exp(ln|a₀|/n)，若该指数超出 exp 的量级上限，
        // pow 在 Fast 模式下必然返回 Err——先判再算，省掉一次注定失败的 ln/exp
        let ln_a0_over_n = a0_abs.magnitude_log10() * std::f64::consts::LN_10 / n as f64;
        if ln_a0_over_n > bigfloat::EXP_ARG_LIMIT_LOG10 {
            None
        } else {
            a0_abs.pow(&inv_n, bigfloat::precision()).ok()
        }
    };
    let radius = match geom {
        Some(g) => {
            if g.value < cauchy.value {
                g
            } else {
                cauchy.clone()
            }
        }
        None => cauchy.clone(),
    };

    // 初值生成：n 个起点均匀撒在半径 r 的圆上，角度与整格错开半格
    let make_init = |r: &BigFloat| -> Vec<Cx> {
        let pi = BigFloat::pi(bigfloat::precision());
        let two_pi = BigFloat::mul(&pi, &BigFloat::from_u64(2), bigfloat::precision());
        let nf = BigFloat::from_u64(n as u64);
        let half = BigFloat::div(
            &BigFloat::from_u64(1),
            &BigFloat::from_u64(2),
            bigfloat::precision(),
        );
        (0..n)
            .map(|k| {
                let k_half = BigFloat::add(&BigFloat::from_u64(k as u64), &half, bigfloat::precision());
                let angle = BigFloat::div(
                    &BigFloat::mul(&two_pi, &k_half, bigfloat::precision()),
                    &nf,
                    bigfloat::precision(),
                );
                Cx {
                    re: BigFloat::mul(r, &angle.cos(bigfloat::precision()), bigfloat::precision()),
                    im: BigFloat::mul(r, &angle.sin(bigfloat::precision()), bigfloat::precision()),
                }
            })
            .collect()
    };

    // 两阶段迭代：每轮都是 O(n²) 的复数乘除，成本由 BigInt 位数决定。
    // 阶段 1 用 40 位低精度快速把根拉到正确位置附近（单轮开销约为全精度的 1/2~1/3）；
    // 阶段 2 用全精度精化——从 1e-20 的近似出发，牛顿型收敛通常十余轮即达 1e-40。
    let rough_prec = (bigfloat::precision() / 2).clamp(20, 80);
    let rough_coeffs: Vec<BigFloat> = norm.iter().map(|c| c.rounded(rough_prec)).collect();
    let rough_tol = BigFloat::div(
        &BigFloat::from_u64(1),
        &BigInt::from(10).pow(rough_prec.div_ceil(2) as u32).into(),
        rough_prec,
    );

    // 先按贴近根模的半径试一次；未收敛则退回柯西界半径重试一次
    let mut zs = make_init(&radius);
    let mut ok = dk_iterate(&rough_coeffs, &mut zs, rough_prec, &rough_tol, 300);
    if !ok && radius.value != cauchy.value {
        zs = make_init(&cauchy);
        ok = dk_iterate(&rough_coeffs, &mut zs, rough_prec, &rough_tol, 300);
    }
    if !ok {
        // 未收敛：宁可不给根，也不能把中间迭代值当作根输出
        return Err(format!(
            "数值求根未收敛（{n} 次多项式的低精度粗收敛失败），请尝试低次方程",
        ));
    }
    if !dk_iterate(&norm, &mut zs, bigfloat::precision(), &tol, 200) {
        return Err(format!(
            "数值求根未收敛（{n} 次多项式超出当前迭代策略），请尝试低次方程",
        ));
    }
    Ok(zs)
}

/// 复数根 → PolySolution（虚部接近 0 视为实根）
fn complex_to_solution(z: Cx) -> PolySolution {
    let t = BigFloat::div(
        &BigFloat::from_u64(1),
        &BigInt::from(10).pow((bigfloat::precision() * 3 / 8) as u32).into(),
        bigfloat::precision(),
    );
    if z.im.value.abs() <= t.value.abs() {
        PolySolution::Real(Number::Approx(z.re))
    } else {
        PolySolution::Complex(Number::Approx(z.re), Number::Approx(z.im))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;

    /// 根的**顺序不保证**（实测 `x^2-3x+2=0` 返回 `2, 1`）⇒ 一律用集合比较
    fn root_set(sols: &[PolySolution], mode: DisplayMode) -> std::collections::BTreeSet<String> {
        sols.iter().map(|s| format_solution(s, mode)).collect()
    }

    fn set(items: &[&str]) -> std::collections::BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn quadratic_two_distinct_roots() {
        // x² - 3x + 2 = (x-1)(x-2)
        let sols = solve_quadratic(&Number::from_int(1), &Number::from_int(-3), &Number::from_int(2));
        assert_eq!(root_set(&sols, DisplayMode::MathIO), set(&["1", "2"]));
    }

    #[test]
    fn quadratic_double_root() {
        // x² - 2x + 1 = (x-1)² ⇒ 两个相等的根
        let sols = solve_quadratic(&Number::from_int(1), &Number::from_int(-2), &Number::from_int(1));
        assert_eq!(root_set(&sols, DisplayMode::MathIO), set(&["1", "1"]));
    }

    #[test]
    fn quadratic_complex_pair() {
        // x² + 1 = 0 ⇒ ±i（判别式为负的分支）
        let sols = solve_quadratic(&Number::from_int(1), &Number::from_int(0), &Number::from_int(1));
        assert_eq!(root_set(&sols, DisplayMode::MathIO), set(&["0 + 1i", "0 - 1i"]));
    }

    #[test]
    fn cubic_with_three_integer_roots() {
        // x³ - 6x² + 11x - 6 = (x-1)(x-2)(x-3)，升幂系数 [-6, 11, -6, 1]
        let coeffs = [
            Number::from_int(-6),
            Number::from_int(11),
            Number::from_int(-6),
            Number::from_int(1),
        ];
        let sols = solve_poly_full(&coeffs).expect("应能求解");
        assert_eq!(root_set(&sols, DisplayMode::MathIO), set(&["1", "2", "3"]));
    }

    #[test]
    fn quartic_with_four_integer_roots() {
        // x⁴ - 5x² + 4 = (x²-1)(x²-4) ⇒ {-2,-1,1,2}
        let coeffs = [
            Number::from_int(4),
            Number::from_int(0),
            Number::from_int(-5),
            Number::from_int(0),
            Number::from_int(1),
        ];
        let sols = solve_poly_full(&coeffs).expect("应能求解");
        assert_eq!(root_set(&sols, DisplayMode::MathIO), set(&["-2", "-1", "1", "2"]));
    }

    #[test]
    fn irrational_cubic_goes_through_durand_kerner() {
        // x³ - 2 在有理数上不可约 ⇒ 走 DK：一个实根 ∛2 ≈ 1.2599210498948731648
        // （∛2 = 1.2599210498948731647672… 可用 2^(1/3) 独立核对）
        let coeffs = [
            Number::from_int(-2),
            Number::from_int(0),
            Number::from_int(0),
            Number::from_int(1),
        ];
        let sols = solve_poly_full(&coeffs).expect("应能求解");
        assert_eq!(sols.len(), 3, "三次恰三个根（一实两复）");
        let approx: Vec<String> = sols
            .iter()
            .map(|s| format_solution(s, DisplayMode::LineIO))
            .collect();
        assert!(
            approx.iter().any(|s| s.starts_with("1.2599210498948731")),
            "应含实根 ∛2: {approx:?}"
        );
    }

    #[test]
    fn degree_guard_rejects_high_degree_in_fast_mode() {
        // 次数 211 > Fast 模式上限（200）⇒ 必须报"带护栏前缀"的错，而不是跑到天荒地老。
        // 用 x^211 - 2：211 是素数且 2 满足艾森斯坦判别法 ⇒ 在有理数上不可约，
        // **不会被因式分解绕开**（我最初用 x^201-1 就踩了这个坑：201=3×67 可分解，根本走不到 DK）。
        let mut coeffs = vec![Number::from_int(0); 212];
        coeffs[0] = Number::from_int(-2); // -2
        coeffs[211] = Number::from_int(1); // + x^211
        let err = solve_poly_full(&coeffs).unwrap_err();
        assert!(err.starts_with(DEGREE_GUARD_PREFIX), "错误应带护栏前缀: {err}");
    }

    #[test]
    fn polynomial_coefficients_are_ascending() {
        // x² - 4 ⇒ 升幂 [-4, 0, 1]
        let ev = Evaluator::new();
        let expr = Parser::new("x^2-4").parse_expression().unwrap();
        let coeffs = extract_polynomial(&ev, &expr, 'x').expect("应识别为多项式");
        let want = [Number::from_int(-4), Number::from_int(0), Number::from_int(1)];
        assert_eq!(coeffs.len(), want.len(), "系数个数不符: {coeffs:?}");
        for (got, w) in coeffs.iter().zip(want.iter()) {
            assert_eq!(hipercalc_core::display::format_mathio(got), hipercalc_core::display::format_mathio(w), "升幂系数不符: {coeffs:?}");
        }
    }
}
