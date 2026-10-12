use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive, Zero};

use crate::parser::{DisplayMode, Evaluator, Expr};
use hipercalc_core::bigfloat::{self, BigFloat};
use hipercalc_core::complex::ComplexNum;
use hipercalc_core::number::Number;
use hipercalc_core::trig::{self, AngleMode};

/// 高次数值求根被规模护栏拒绝时的错误前缀。
/// 调用方（main.rs::handle_equation）据此判断"这是明确拒绝"而不是"求根失败"，
/// 从而**不再回退牛顿单根**——高次多项式的牛顿求值代价极高（x^5000-1 每次求值都要算 5000 次幂），
/// 回退只会让界面长时间无响应，而且只能给出一个根。
pub const DEGREE_GUARD_PREFIX: &str = "数值求根次数过高";

/// Fast 模式下的次数上限（DK 与幂替换共用同一口径）。
///
/// DK 每轮 O(n²) 复数乘除，实测 n≈200 已需约 25 秒；幂替换本身很快，但**根的数量仍是原次数**
/// （`x^5000-1` 会刷出 5000 个根），所以两者用同一个上限，避免"换个入口就把护栏绕过去"。
/// Deep 模式放开（死算由用户承担代价）。
const MAX_DEGREE_FAST: usize = 200;

/// 当前允许的最大次数（Deep 模式不设限）
fn degree_limit() -> usize {
    if hipercalc_core::calc_mode::is_deep() {
        usize::MAX
    } else {
        MAX_DEGREE_FAST
    }
}

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
///
/// **不能用 `b.value.abs() > limit * 10^b.precision` 这种尾数比较**：`BigFloat` 的
/// `value` 只是尾数，真实值是 `value / 10^precision`。`round_to` 会在保持数值不变的前提下
/// 同时改变尾数与精度（`1e-50` 的尾数是 `10^50`、精度 100，也可以是 `10^30`、精度 80），
/// 所以两个不同精度的数直接比尾数会得出完全相反的结论。用 `magnitude_log10`（已经考虑精度）。
fn exceeds_abs_limit(b: &BigFloat, limit: i64) -> bool {
    b.magnitude_log10() > (limit.abs() as f64).log10()
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
        &BigInt::from(10)
            .pow(bigfloat::precision().div_ceil(2) as u32 + 10)
            .into(),
        bigfloat::precision(),
    );
    // 判据统一用**量级**比较（`magnitude_log10` 已经把 precision 考虑进去）。
    // 直接比 `value.abs()`（尾数）是错的：BigFloat 的真实值是 value/10^precision，
    // 两个精度不同的数比尾数会得出完全相反的结论 —— 这正是"f(x)=x 在初值 1 处被判为
    // 已收敛"的原因（`|fx|` 尾数 0.3、阈值尾数 10^30，1 <= 10^30 成立）。
    let zero_log = zero_compare.magnitude_log10();

    for _ in 0..max_iter {
        // 中断检查：本函数返回 Option，故中断表现为"放弃这个初值"（外层循环另有检查点）
        hipercalc_core::cancel::check().ok()?;
        // 计算 f(x)
        // 复数结果（如 `ln(-1)`、`sqrt(-4)`）说明该初值落在实数域之外：
        // `Number::to_approx` 对复数会 debug_assert（不能静默丢虚部）⇒ 必须先分流、当作无效初值放弃。
        let fx =
            match evaluator.evaluate_with_var(f_expr, &var.to_string(), &Number::Approx(x.clone()))
            {
                Ok(v) if !v.is_complex() => v.to_approx(),
                _ => return None,
            };

        // 检查与 0 的接近程度。
        //
        // 这里**只能**用与 0 的绝对距离：曾经还有一条
        // `|| (fx.value.abs() <= 1 && fx.precision >= 60)` 的宽松分支，它的本意是
        // "定点尾数只剩个位数、说明已经算到零附近"，但实际上 `f(x) = x` 这类**恒等/线性**
        // 函数在**任何**初值处都满足 `|f(x)| = |x|`，于是牛顿会"立即收敛"并返回初值本身。
        // 症状：`solve_aux::collect_all_roots(ev, x, 'x')` 返回 `[0, ±1..±6]`（全是初值、
        // 没有一个是根），下游 `calculus::integrate::singular_inside` 拿它当"分母的根"，
        // 把 `int(1/x,x,1,3)` 误判成区间内有奇点而拒绝计算（`[1,2]`、`[2,3]` 恰好躲过）。
        // 删掉该分支后，`f(x) = x` 会正常走一步牛顿迭代到真正的根 0。
        if fx.magnitude_log10() <= zero_log {
            return Some(x);
        }

        // 中心差分近似导数 f'(x) ≈ (f(x+h) - f(x-h)) / (2h)
        let x_plus = BigFloat::add(&x, &h, bigfloat::precision());
        let x_minus = BigFloat::sub(&x, &h, bigfloat::precision());

        let f_plus =
            match evaluator.evaluate_with_var(f_expr, &var.to_string(), &Number::Approx(x_plus)) {
                Ok(v) if !v.is_complex() => v.to_approx(),
                _ => return None,
            };
        let f_minus =
            match evaluator.evaluate_with_var(f_expr, &var.to_string(), &Number::Approx(x_minus)) {
                Ok(v) if !v.is_complex() => v.to_approx(),
                _ => return None,
            };

        let df = BigFloat::sub(&f_plus, &f_minus, bigfloat::precision());
        let two_h = BigFloat::mul(&BigFloat::from_u64(2), &h, bigfloat::precision());
        let derivative = BigFloat::div(&df, &two_h, bigfloat::precision());

        // 导数为零 ⇒ 该初值不可用。**必须显式判零**：`BigFloat::div` 内部是 `assert!`，除数为零会
        // 直接把进程 panic 掉（实测 `/mode prec 40` 下 `sin(x)=0` 崩溃）。
        // 这里也一律用**量级**判据，不用 `value.abs()`（尾数）——后者忽略 precision，
        // 在 `round_to` 调整过精度的数上会得出错误结论。
        if derivative.is_zero() || derivative.magnitude_log10() <= zero_log {
            return None; // 导数为零（或小到无法做除法）
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
        if correction.magnitude_log10() <= zero_log {
            return Some(x_new);
        }

        x = x_new;
    }

    // 迭代未在循环内确认收敛：校验最终残差，避免返回假根。
    //
    // 判据必须与循环内的收敛判据同量级。旧实现这里写的是 `|f(x)| ≤ 10^(precision/2)`
    // （默认精度下 = 1e40）—— 那个阈值宽松到几乎等于不做校验：`f(x) = x` 在初值 1 处
    // 的残差是 1，照样被当成根返回，于是 `collect_all_roots` 把整列初值当成了根。
    match evaluator.evaluate_with_var(f_expr, &var.to_string(), &Number::Approx(x.clone())) {
        Ok(v) if !v.is_complex() => {
            let bf = v.to_approx();
            if bf.magnitude_log10() <= zero_log {
                Some(x.rounded(bigfloat::precision()))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// 从 Expr 中提取多项式系数（按升幂排列: coeffs[i] = x^i 的系数）
pub fn extract_polynomial(evaluator: &Evaluator, expr: &Expr, var: char) -> Option<Vec<Number>> {
    // 先判断是否多项式：仅含常数、变量、+-*^(整数)
    if !crate::equation::is_polynomial(expr) {
        return None;
    }

    // 尝试直接提取系数
    extract_coeffs_direct(evaluator, expr, var)
}

/// 直接从 AST 提取多项式系数
fn extract_coeffs_direct(evaluator: &Evaluator, expr: &Expr, var: char) -> Option<Vec<Number>> {
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
            } else {
                let val = evaluator.vars.get(name)?;
                // 已存储的变量（/let，全大写）作为常数项
                terms.push((val.clone(), 0));
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
            if let Expr::Variable(name) = base.as_ref()
                && name.len() == 1
                && name.chars().next().unwrap() == var
                && let Expr::Number(n) = exp.as_ref()
                && let Some(rat) = n.as_rational()
                && rat.is_integer()
            {
                let deg_u32 = rat.to_integer().to_u32()?;
                terms.push((Number::from_int(1), deg_u32));
                return Some(());
            }
            // 一般底数的小整数幂：展开为连乘（如 (x+1)^2）
            if let Expr::Number(n) = exp.as_ref()
                && let Some(rat) = n.as_rational()
                && rat.is_integer()
            {
                let k = rat.to_integer();
                if k.is_zero() {
                    terms.push((Number::from_int(1), 0));
                    return Some(());
                }
                if let Some(k) = k.to_u32()
                    && (2..=64).contains(&k)
                {
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
        Number::Matrix(_) => None,   // 矩阵不是标量
        Number::Quantity(_) => None, // 量纲值不是标量
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

    // ── 幂替换优先：把所有指数同属某个 g 的倍数的多项式用 `y = x^g` 降次 ──
    //
    // **必须排在因式分解之前**：分解器对这种结构只能提出几个平凡因子
    // （`x^100-1` 只提出 `(x-1)(x+1)(x²+1)`，`x^96+x^48+1` 一个都提不出来），
    // 剩下的整块不可约因子会落进 Durand-Kerner —— `x^100-1` 实测 1.9 s，其中 1.76 s 全在 DK。
    // 走幂替换后 `x^100-1 → y-1`、`x^96+x^48+1 → y²+y+1`，都是几次除法的事。
    //
    // 次数护栏与原 DK 同口径（见 `MAX_DEGREE_FAST`）：替换后虽快，根的数量仍是原次数，
    // 不设限会让 `x^5000-1` 从"明确拒绝"变成刷出 5000 个根。超限时照旧落到原路径去拒绝。
    if let Some(g) = power_substitution_degree(&bc)
        && bc.len() - 1 <= degree_limit()
    {
        return solve_by_power_substitution(&bc, g);
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
                let nums: Vec<Number> = fc
                    .iter()
                    .map(|c| Number::from_rational(c.clone()))
                    .collect();
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

/* ---------------- 幂替换求根（y = x^g） ---------------- */

/// 幂替换指数：所有**非零项**指数的最大公约数；≤ 1 表示没有可用结构（返回 `None`）。
///
/// `x^100-1`（指数 0、100）→ 100；`x^96+x^48+1`（0、48、96）→ 48；`x^4-5x^2+4` → 2；
/// `x^50-2x+1`（0、1、50）→ 1（无结构）。
fn power_substitution_degree(bc: &[BigRational]) -> Option<usize> {
    let mut g = 0usize;
    for (i, c) in bc.iter().enumerate() {
        if !c.is_zero() {
            g = gcd_usize(g, i);
        }
    }
    (g > 1).then_some(g)
}

/// `usize` 的最大公约数（`g == 0` 时返回另一个数，于是常数项不影响结果）
fn gcd_usize(a: usize, b: usize) -> usize {
    if b == 0 { a } else { gcd_usize(b, a % b) }
}

/// 幂替换求解：令 `y = x^g`，先解降次后的多项式（可能再被替换一次），再把每个根开 g 次方还原。
///
/// 递归天然收敛：`g ≥ 2` ⇒ 降次后的项数 `ceil(len / g) < len`，次数严格下降。
/// 重数也自动正确：降次后的某个根 y 重 m 次 ⇒ 还原出的 g·m 个根（实测 `(x²-1)²` 给 4 个根）。
fn solve_by_power_substitution(bc: &[BigRational], g: usize) -> Result<Vec<PolySolution>, String> {
    let reduced: Vec<Number> = bc
        .iter()
        .step_by(g)
        .map(|c| Number::from_rational(c.clone()))
        .collect();
    let y_roots = solve_poly_full(&reduced)?;
    let mut out: Vec<PolySolution> = Vec::with_capacity(bc.len() - 1);
    for y in &y_roots {
        out.extend(nth_roots_of(y, g));
    }
    Ok(out)
}

/// `x^g = y` 的全部 g 个根（`k = 0..g-1`：从主值开始，每步转过 `2π/g`）。
///
/// 精确性靠两条路，缺一条就会把"整根"降级成近似：
/// - **模**走 `Number` 算术：`|(-1±i√3)/2| = 1`、`nth_root(4, 2) = 2` 都能保持精确，
///   所以 `x = 1`、`x = -1`、`x = ±2` 照旧标 `=`（不是 `≈`）；
/// - **角度**：`y` 是实数时角度恰是 π 的有理倍（正 ⇒ `2kπ/g`，负 ⇒ `(2k+1)π/g`），
///   直接构造**精确角**交给 `trig::sin/cos_number` —— 特殊角表里有的（`i`、`-1`、`√3/2`…）仍是精确值。
///   `y` 非实数时才用数值辐角：此时 g 个根必然**都不实**（实数的 g 次方必是实数），
///   所以不需要再判别"虚部接近 0 就当实根"。
fn nth_roots_of(y: &PolySolution, g: usize) -> Vec<PolySolution> {
    let (re, im) = match y {
        PolySolution::Real(n) => (n.clone(), Number::from_int(0)),
        PolySolution::Complex(re, im) => (re.clone(), im.clone()),
    };
    // x^g = 0 ⇒ x = 0（g 重根）
    if re.is_zero() && im.is_zero() {
        return vec![PolySolution::Real(Number::from_int(0)); g];
    }

    let prec = bigfloat::precision();
    // 模：|y| 再开 g 次方
    let modulus = if im.is_zero() {
        re.abs()
    } else {
        Number::from_complex(ComplexNum::new(re.clone(), im.clone())).abs()
    };
    let rho = match u32::try_from(g) {
        Ok(k) => modulus
            .nth_root(k)
            .unwrap_or_else(|_| Number::Approx(modulus.to_approx())),
        // g ≤ MAX_DEGREE_FAST，走不到这里；兜底保持"不 panic"
        Err(_) => Number::Approx(modulus.to_approx()),
    };

    // 辐角基准（π 的有理倍）：实正 ⇒ 0，实负 ⇒ π；非实数 ⇒ None（循环里用数值辐角）
    let arg_base = if !im.is_zero() {
        None
    } else if re.is_negative() {
        Some(BigRational::one())
    } else {
        Some(BigRational::zero())
    };
    let g_rat = BigRational::from_integer(BigInt::from(g));
    // z 非零（上面已排除）⇒ `arg()` 不会失败；失败只可能是"0 没有辐角"，用 0 兜底
    let numeric_arg = if arg_base.is_none() {
        Some(
            ComplexNum::new(re.clone(), im.clone())
                .arg()
                .map(|a| a.to_approx())
                .unwrap_or_else(|_| BigFloat::from_u64(0)),
        )
    } else {
        None
    };
    let two_pi = BigFloat::mul(&BigFloat::from_u64(2), &BigFloat::pi(prec), prec);

    let mut out: Vec<PolySolution> = Vec::with_capacity(g);
    for k in 0..g {
        let (c, s) = match &arg_base {
            // 精确角：(base + 2k)·π/g
            Some(base) => {
                let coeff =
                    (base.clone() + BigRational::from_integer(BigInt::from(2 * k))) / &g_rat;
                if coeff.is_integer() {
                    // 整数倍 π（含 0）**必须自己给**：rad 模式的精确表只认 `PiPow` 项，
                    // 而 π 的 0 倍会被规范化成普通有理数 0 ⇒ `cos(0)` 退化成 `≈ 1`，
                    // 会把本该精确的 `x = 1` 降级成 `x ≈ 1`。
                    let n = coeff.to_integer();
                    let cos = if (n % 2i32).is_zero() {
                        Number::from_int(1)
                    } else {
                        Number::from_int(-1)
                    };
                    (cos, Number::from_int(0))
                } else {
                    let a = Number::from_pi_times(coeff);
                    (
                        trig::cos_number(&a, AngleMode::Radian),
                        trig::sin_number(&a, AngleMode::Radian),
                    )
                }
            }
            // 数值角：(arg + 2kπ)/g
            None => {
                let arg = numeric_arg.clone().unwrap_or_else(|| BigFloat::from_u64(0));
                let step = BigFloat::mul(&two_pi, &BigFloat::from_u64(k as u64), prec);
                let total = BigFloat::add(&arg, &step, prec);
                let a = Number::Approx(BigFloat::div(&total, &BigFloat::from_u64(g as u64), prec));
                (
                    trig::cos_number(&a, AngleMode::Radian),
                    trig::sin_number(&a, AngleMode::Radian),
                )
            }
        };
        let xr = rho.mul(&c);
        let xi = rho.mul(&s);
        out.push(if xi.is_zero() {
            PolySolution::Real(xr)
        } else {
            PolySolution::Complex(xr, xi)
        });
    }
    out
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
                zs[k] = cx_sub(&zk, &delta, prec);
                if max_step.value.abs() < delta.re.value.abs() {
                    max_step = delta.re.clone();
                }
                if max_step.value.abs() < delta.im.value.abs() {
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
    if n > MAX_DEGREE_FAST && !hipercalc_core::calc_mode::is_deep() {
        return Err(format!(
            "{DEGREE_GUARD_PREFIX}（{n} 次，Fast 模式上限 {MAX_DEGREE_FAST}）；\
             如确认需要继续，请先执行 /mode deep",
        ));
    }
    // 首一化
    let lead = coeffs[n].clone();
    let norm: Vec<BigFloat> = coeffs
        .iter()
        .map(|c| BigFloat::div(c, &lead, bigfloat::precision()))
        .collect();

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
                let k_half =
                    BigFloat::add(&BigFloat::from_u64(k as u64), &half, bigfloat::precision());
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
    // 阶段 1 用低精度快速把根拉到正确位置附近；阶段 2 用全精度精化。
    //
    // 粗阶段的精度按用户精度**相对**取（原来固定 40 位）：`/mode prec 200` 时
    // 40 位粗收敛与 200 位精化之间的差距过大，精化轮数会明显增加；
    // 而 `/mode prec 20` 时又几乎等于全精度、失去了"粗"的意义。
    // 取 `precision/2` 且下限 16：既保留"单轮便宜一半以上"的收益（BigInt 位数减半），
    // 又保证粗解足够接近真根（DK 是二次收敛，粗解精度每低一位只多约一轮精化）。
    let rough_prec = (bigfloat::precision() / 2).clamp(16, 60);
    let rough_coeffs: Vec<BigFloat> = norm.iter().map(|c| c.rounded(rough_prec)).collect();
    let rough_tol = BigFloat::div(
        &BigFloat::from_u64(1),
        &BigInt::from(10).pow(rough_prec.div_ceil(2) as u32).into(),
        rough_prec,
    );
    let tol = BigFloat::div(
        &BigFloat::from_u64(1),
        &BigInt::from(10)
            .pow(bigfloat::precision().div_ceil(2) as u32)
            .into(),
        bigfloat::precision(),
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
        &BigInt::from(10)
            .pow((bigfloat::precision() * 3 / 8) as u32)
            .into(),
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
        let sols = solve_quadratic(
            &Number::from_int(1),
            &Number::from_int(-3),
            &Number::from_int(2),
        );
        assert_eq!(root_set(&sols, DisplayMode::MathIO), set(&["1", "2"]));
    }

    #[test]
    fn quadratic_double_root() {
        // x² - 2x + 1 = (x-1)² ⇒ 两个相等的根
        let sols = solve_quadratic(
            &Number::from_int(1),
            &Number::from_int(-2),
            &Number::from_int(1),
        );
        assert_eq!(root_set(&sols, DisplayMode::MathIO), set(&["1", "1"]));
    }

    #[test]
    fn quadratic_complex_pair() {
        // x² + 1 = 0 ⇒ ±i（判别式为负的分支）
        let sols = solve_quadratic(
            &Number::from_int(1),
            &Number::from_int(0),
            &Number::from_int(1),
        );
        assert_eq!(
            root_set(&sols, DisplayMode::MathIO),
            set(&["0 + 1i", "0 - 1i"])
        );
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
        assert_eq!(
            root_set(&sols, DisplayMode::MathIO),
            set(&["-2", "-1", "1", "2"])
        );
    }

    /// 由 `(指数, 系数)` 生成升幂系数表（构造测试用多项式）
    fn poly(terms: &[(usize, i64)]) -> Vec<Number> {
        let n = terms.iter().map(|(e, _)| e + 1).max().unwrap();
        let mut v = vec![Number::from_int(0); n];
        for (e, c) in terms {
            v[*e] = Number::from_int(*c);
        }
        v
    }

    /// 系数表 → 有理数表（幂替换只看指数，用它来构造输入）
    fn ratios(ns: &[Number]) -> Vec<BigRational> {
        ns.iter().map(|n| number_to_rational(n).unwrap()).collect()
    }

    #[test]
    fn power_substitution_degree_detects_structure() {
        // x⁴ - 5x² + 4 → g = 2；x^100 - 1 → g = 100；x⁵ - x - 1 → 无结构
        assert_eq!(
            power_substitution_degree(&ratios(&poly(&[(4, 1), (2, -5), (0, 4)]))),
            Some(2)
        );
        assert_eq!(
            power_substitution_degree(&ratios(&poly(&[(100, 1), (0, -1)]))),
            Some(100)
        );
        assert_eq!(
            power_substitution_degree(&ratios(&poly(&[(5, 1), (1, -1), (0, -1)]))),
            None
        );
        // 只有常数项 ⇒ 没有可替换的指数
        assert_eq!(power_substitution_degree(&ratios(&poly(&[(0, 7)]))), None);
    }

    #[test]
    fn power_substitution_solves_high_degree_without_durand_kerner() {
        // x^100 - 1：因式分解只提得出 `(x-1)(x+1)(x²+1)`，余下 96 次整块会落进 DK（实测约 1.8 s）；
        // 幂替换把它变成 `y - 1 = 0` 再开 100 次方。
        let sols = solve_poly_full(&poly(&[(100, 1), (0, -1)])).expect("应能求解");
        assert_eq!(sols.len(), 100, "100 次方程应给 100 个根");
        let labels = root_set(&sols, DisplayMode::MathIO);
        // ±1 与 ±i 必须仍是**精确**根：幂替换不能让它们退化成近似值
        for k in ["1", "-1", "0 + 1i", "0 - 1i"] {
            assert!(labels.contains(k), "缺精确根 {k}: {labels:?}");
        }
        // 单位根：|z|² = 1
        let prec = bigfloat::precision();
        for s in &sols {
            let (re, im) = match s {
                PolySolution::Real(n) => (n.to_approx(), BigFloat::from_u64(0)),
                PolySolution::Complex(re, im) => (re.to_approx(), im.to_approx()),
            };
            let m2 = BigFloat::add(
                &BigFloat::mul(&re, &re, prec),
                &BigFloat::mul(&im, &im, prec),
                prec,
            );
            let diff = BigFloat::sub(&m2, &BigFloat::from_u64(1), prec);
            assert!(
                diff.magnitude_log10() < -30.0,
                "根不在单位圆上: {}",
                format_solution(s, DisplayMode::LineIO)
            );
        }
    }

    #[test]
    fn power_substitution_keeps_original_multiplicity() {
        // (x² - 1)² = x⁴ - 2x² + 1 ⇒ 幂替换后是 (y-1)² ⇒ 4 个根（±1 各重 2 次）
        let sols = solve_poly_full(&poly(&[(4, 1), (2, -2), (0, 1)])).expect("应能求解");
        assert_eq!(
            root_set(&sols, DisplayMode::MathIO),
            set(&["1", "1", "-1", "-1"])
        );
    }

    #[test]
    fn power_substitution_handles_root_at_zero() {
        // x⁶ = 0 ⇒ 6 重根 0（降次后是 `y = 0`，再开 6 次方）
        let sols = solve_poly_full(&poly(&[(6, 1)])).expect("应能求解");
        assert_eq!(
            root_set(&sols, DisplayMode::MathIO),
            set(&["0", "0", "0", "0", "0", "0"])
        );
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
        assert!(
            err.starts_with(DEGREE_GUARD_PREFIX),
            "错误应带护栏前缀: {err}"
        );
    }

    #[test]
    fn polynomial_coefficients_are_ascending() {
        // x² - 4 ⇒ 升幂 [-4, 0, 1]
        let ev = Evaluator::new();
        let expr = Parser::new("x^2-4").parse_expression().unwrap();
        let coeffs = extract_polynomial(&ev, &expr, 'x').expect("应识别为多项式");
        let want = [
            Number::from_int(-4),
            Number::from_int(0),
            Number::from_int(1),
        ];
        assert_eq!(coeffs.len(), want.len(), "系数个数不符: {coeffs:?}");
        for (got, w) in coeffs.iter().zip(want.iter()) {
            assert_eq!(
                hipercalc_core::display::format_mathio(got),
                hipercalc_core::display::format_mathio(w),
                "升幂系数不符: {coeffs:?}"
            );
        }
    }
}
