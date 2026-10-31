//! **求解结果的组织与渲染**：把散落的根变成人能读的答案。
//!
//! 具体包括：方程解的**通式**（含周期项 `+kπ`）、根的**合并与去重**、
//! 不等式解集的**区间合并**、以及结果前缀（`=` 精确 / `≈` 近似）的判定。
//!
//! 边界：它不"求"根，只负责**收集、整理、排版**；真正的求根在 `solver_*` 里。

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::Signed;

use crate::parser::{BinOp, DisplayMode, Evaluator, Expr, UnaryOp};
use crate::solver_poly;
use hipercalc_core::bigfloat::{self, BigFloat};
use hipercalc_core::number::Number;
use hipercalc_core::trig::{self, AngleMode};

/// 判定结果前缀：有限小数/精确符号表达 → "="；无限小数（近似/循环小数/无理）→ "≈"
pub fn result_prefix(n: &Number, mode: DisplayMode) -> &'static str {
    match n {
        // 量纲值：按数值部分的精度给前缀（量纲不改变"是否精确"）
        Number::Quantity(q) => result_prefix(&q.value, mode),
        // 矩阵：元素都是精确值时才给 `=`（近似元素按 ≈）
        Number::Matrix(m) => {
            if m.iter().flatten().all(|x| result_prefix(x, mode) == "=") {
                "="
            } else {
                "≈"
            }
        }
        // 复数：两个分量都能"精确呈现"才给 `=`
        Number::Complex(z) => {
            if result_prefix(&z.re, mode) == "=" && result_prefix(&z.im, mode) == "=" {
                "="
            } else {
                "≈"
            }
        }
        Number::Approx(_) => "≈",
        Number::Exact(e) => match mode {
            DisplayMode::MathIO => "=", // MathIO 下精确值以符号/分数显示，均为精确定义
            DisplayMode::LineIO => {
                // LineIO 一律输出小数：只有"能完整显示"的有理数才是 `=`。
                // 有限小数超过 bigfloat::display_digits() 位有效数字时会被四舍五入，
                // 旧实现只看分母是否只含 2/5 因子，导致 1/2^70 被标成 `=`（实际已截断）。
                if e.as_rational().is_some_and(|r| finite_decimal_fits(&r)) {
                    "="
                } else {
                    "≈" // 循环小数、无理数（π、√2 等）
                }
            }
        },
    }
}

/// LineIO 下该有理数能否**完整**显示（不被 20 位有效数字截断）：
/// - 整数：BigFloat 的 precision 归一后为 0，会整串输出 ⇒ true；
/// - 有限小数：十进制展开的有效数字位数 ≤ bigfloat::display_digits() ⇒ true；
/// - 循环小数（分母含 2、5 以外因子）⇒ false。
fn finite_decimal_fits(r: &BigRational) -> bool {
    if r.is_integer() {
        return true;
    }
    let mut d = r.denom().clone();
    let mut places = 0usize;
    while &d % BigInt::from(2) == BigInt::from(0) {
        d /= 2u32;
        places += 1;
    }
    while &d % BigInt::from(5) == BigInt::from(0) {
        d /= 5u32;
        places += 1;
    }
    if d != BigInt::from(1) {
        return false;
    }
    // 死算模式（/mode deep）输出完整小数，不做 20 位有效数字截断 ⇒ 有限小数一律完整
    if hipercalc_core::calc_mode::is_deep() {
        return true;
    }
    // 小数位数过大时必超上限，无需精确计算（避免 10^places 过大）
    if places > 200 {
        return false;
    }
    // 完整展开的整数形式：|p|·10^places/q 去尾零后的位数即有效数字位数
    let n_abs = (r.numer().abs() * BigInt::from(10).pow(places as u32)) / r.denom();
    let s = n_abs.to_string();
    s.trim_end_matches('0').len() <= hipercalc_core::bigfloat::display_digits()
}

/// 若近似值与某个简单分数（p/q，|p|≤200，q≤20）或整数足够接近，返回其精确有理数 (p, q)。
/// 容差随量级缩放（tol = 1e-12·min(|x|,1)），否则 1e-30 这类极小根会被错误回填成 0。
/// 返回分子分母而不是字符串：调用方要按显示模式输出（MathIO `1 / 2`、LineIO `0.5`）。
pub fn float_to_exact_rational(x: &BigFloat) -> Option<(i64, i64)> {
    if x.is_zero() {
        return Some((0, 1));
    }
    // 早退：候选全部落在 [-200, 200] 内，量级明显超出时直接放弃，
    // 避免为 x ≈ 1000 这类根白扫 8020 个候选（每次都要构造 BigFloat 并做除法）
    if x.magnitude_log10() > 3.0 {
        return None;
    }
    let tol = {
        let mag = x.magnitude_log10(); // log10|x|
        let denom = if mag >= 0.0 {
            BigInt::from(10).pow(12)
        } else {
            BigInt::from(10).pow((-mag).ceil() as u32 + 12)
        };
        BigFloat::div(&BigFloat::from_u64(1), &denom.into(), bigfloat::precision())
    };
    // 候选常量表只构造一次（旧实现每轮 `BigFloat::from_i64(p/q)` 各分配一次，共 ~8000 次）
    let p_table: Vec<BigFloat> = (-200i64..=200).map(BigFloat::from_i64).collect();
    let q_table: Vec<BigFloat> = (1i64..=20).map(BigFloat::from_i64).collect();
    for (qi, qf) in q_table.iter().enumerate() {
        let q = (qi + 1) as i64;
        for (pi, pf) in p_table.iter().enumerate() {
            let p = pi as i64 - 200;
            let cand = BigFloat::div(pf, qf, bigfloat::precision());
            if bf_abs(BigFloat::sub(&cand, x, bigfloat::precision()))
                .value
                .abs()
                <= tol.value.abs()
            {
                let g = gcd64(p, q);
                return Some((p / g, q / g));
            }
        }
    }
    None
}

/// i64 最大公约数（绝对值）
pub fn gcd64(mut a: i64, mut b: i64) -> i64 {
    a = a.abs();
    b = b.abs();
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    if a == 0 { 1 } else { a }
}

/// BigFloat 绝对值（value 为负时取反）
pub fn bf_abs(b: BigFloat) -> BigFloat {
    if b.value < BigInt::from(0) {
        BigFloat::neg(&b)
    } else {
        b
    }
}

/// π 的有理倍字符串（π/2、2π、3π/4…）。会先约分：(10, 6) → `5π/3`。
pub fn pi_frac_str(p: i64, q: i64) -> String {
    if p == 0 {
        return "0".to_string();
    }
    // 统一处理负号：p = -1 输出 "-π/q" 而不是 "-1π/q"
    let g = gcd64(p, q);
    let (p, q) = if g > 1 { (p / g, q / g) } else { (p, q) };
    let sign = if p < 0 { "-" } else { "" };
    let p_abs = p.abs();
    if q == 1 {
        if p_abs == 1 {
            format!("{}π", sign)
        } else {
            format!("{}{}π", sign, p_abs)
        }
    } else if p_abs == 1 {
        format!("{}π/{}", sign, q)
    } else {
        format!("{}{}π/{}", sign, p_abs, q)
    }
}

/// π 的有理倍在**角度模式**下的标签：值 = `p·180/q`，精确十进制。
/// 识别出的分母 q ∈ {1,2,3,4,6,8,12}，只有 q=8 会出现一位小数，故不会引入舍入误差。
pub fn deg_str(p: i64, q: i64) -> String {
    if p == 0 {
        return "0".to_string();
    }
    let g = gcd64(p, q);
    let (p, q) = if g > 1 { (p / g, q / g) } else { (p, q) };
    let num = p * 180;
    let (sign, mag) = if num < 0 { ("-", -num) } else { ("", num) };
    if q == 0 {
        return format!("{}{}", sign, mag);
    }
    if mag % q == 0 {
        format!("{}{}", sign, mag / q)
    } else {
        // 仅 q=8 命中：值恒为 x.5，×10 后必整除
        let scaled = mag * 10 / q;
        format!("{}{}.{}", sign, scaled / 10, scaled % 10)
    }
}

/// 周期/残基被识别为 π 的有理倍时的分母上限
const PI_DEN_MAX: i64 = 12;

/// 周期比 `d/π` 与其有理逼近之间的容差（沿用旧实现的相对容差 `tol = 1e-6`）
const PI_TOL_REL: f64 = 1e-6;

/// 残基比 `r/π` 与其有理逼近之间的容差（沿用旧实现的 `10·tol`）
const PI_TOL_REL_RESIDUAL: f64 = 1e-5;

/// BigFloat → f64。只用于"把周期/残基比上 π 再做小分母有理逼近"，
/// 比值量级 O(1~10)，f64 的 1e-16 相对误差远够（判定容差是 1e-6）。
fn bf_to_f64(b: &BigFloat) -> f64 {
    use num_traits::ToPrimitive;
    let v = b.value.to_f64().unwrap_or(f64::NAN);
    v / 10f64.powi(b.precision as i32)
}

/// 在分母 ≤ `max_den` 的分数里找最接近 `x` 的那个（q 从小到大、严格更优才替换 ⇒ 天然取最简分母）
fn best_rational(x: f64, max_den: i64) -> (i64, i64) {
    let mut best = (0i64, 1i64);
    let mut best_err = f64::INFINITY;
    for q in 1..=max_den.max(1) {
        let p = (x * q as f64).round();
        if !p.is_finite() || p.abs() > 1e9 {
            continue;
        }
        let err = (x - p / q as f64).abs();
        if err < best_err {
            best_err = err;
            best = (p as i64, q);
        }
    }
    best
}

/// 把 `x` 识别成 π 的有理倍：返回 `(p, q)` 使 `x ≈ (p/q)·π`；比值误差超过 `tol_rel` 则 `None`。
///
/// 取代了原先的固定候选表（`d_m` / `r_c`）——固定表覆盖不到 `5π/3`（`cos(x)=0.5` 的第二族），
/// 会让这类方程在**两种模式**下都退化成数值列表。
fn pi_ratio(x: &BigFloat, pi: &BigFloat, max_den: i64, tol_rel: f64) -> Option<(i64, i64)> {
    let ratio = bf_to_f64(x) / bf_to_f64(pi);
    if !ratio.is_finite() {
        return None;
    }
    let (p, q) = best_rational(ratio, max_den);
    if q == 0 {
        return None;
    }
    if (ratio - p as f64 / q as f64).abs() <= tol_rel {
        Some((p, q))
    } else {
        None
    }
}

/// 若收集到的根呈周期结构（单等差或两族交错），生成"通解"；否则 None。
/// 单族：cos(x)=0 → "x = π/2 + k·π，k 为整数"（度模式则为 "x = 90 + k·180，k 为整数"）
/// 多族：sin(x)=0.5 → "x = π/6 + k·2π  或  5π/6 + k·2π，k 为整数"
///
/// 内部**一律把根折算成弧度**再判定（`angle_mode` 只影响入参解释与最终标签），
/// 因此两种模式的判定完全同构；标签按单位分派给 `pi_frac_str` / `deg_str`。
///
/// 判定前先按 `±3π` 的窗口过滤：牛顿从远处的初值可能落到很远的根（弧度模式下 `sin(x)+cos(x)=1`
/// 会混进 ≈29.8 这类根），它们混进根集合会让相邻差不再规整、把整条通式判否。
/// 窗口**只影响判定输入**，不影响调用方在回退分支里列出的全部根。
pub fn format_periodic_roots(roots: &[Number], var: char, angle_mode: AngleMode) -> Option<String> {
    format_periodic_roots_min(roots, var, angle_mode, MIN_ROOTS_FOR_PERIOD)
}

/// 判定周期所需的**最少根数**。
///
/// 数值扫根来的根可能残缺（`sin(x)=1` 的切点重根只有 3 个）⇒ 阈值取 4，宁可退回列表也不误判。
/// 而闭式快路是**自己按家族构造**出来的根，根集完整，可以用更低的阈值（见 `trig_closed_form`）。
const MIN_ROOTS_FOR_PERIOD: usize = 4;

/// 同 `format_periodic_roots`，但可指定"最少根数"（闭式快路传 3：`x = π/2 + k·2π` 这类
/// 单族解在 ±3π 窗口内本来就只有 3 个根）
pub fn format_periodic_roots_min(
    roots: &[Number],
    var: char,
    angle_mode: AngleMode,
    min_roots: usize,
) -> Option<String> {
    if roots.len() < min_roots {
        return None;
    }
    let prec = bigfloat::precision();
    let pi = BigFloat::pi(prec);
    // 度模式：×π/180 折算成弧度（之后完全沿用既有 π 逻辑）
    let rad_per_deg = BigFloat::div(&pi, &BigFloat::from_u64(180), prec);
    let mut vals: Vec<BigFloat> = roots
        .iter()
        .map(|n| {
            let v = n.to_approx();
            match angle_mode {
                AngleMode::Radian => v,
                AngleMode::Degree => BigFloat::mul(&v, &rad_per_deg, prec),
            }
        })
        .collect();
    vals.sort_by(|a, b| a.value.cmp(&b.value));

    // 窗口：只保留 ±3 个单位角以内的根（已折算成弧度，故两模式窗口一致）
    let window = BigFloat::mul(&pi, &BigFloat::from_u64(3), prec);
    vals.retain(|v| bf_abs(v.clone()).value.abs() <= window.value.abs());
    if vals.len() < min_roots {
        return None;
    }

    let tol = BigFloat::div(
        &BigFloat::from_u64(1),
        &BigFloat::from_u64(1000000),
        bigfloat::precision(),
    );
    // 绝对容差：残基接近 0 时相对容差会同步趋近 0（上界 = |r|·tol），导致去重失效。
    // 例如 sin(x)=0 的各根残基约 1e-50，旧实现下会被当成互不相同的多个家族。
    let tol_abs = BigFloat::div(
        &BigFloat::from_u64(1),
        &BigInt::from(10).pow(40).into(),
        bigfloat::precision(),
    );
    let same = |a: &BigFloat, b: &BigFloat| -> bool {
        // |a-b| ≤ tol·max(|a|,|b|) + tol_abs
        let aa = bf_abs(a.clone());
        let bb = bf_abs(b.clone());
        let ma = if aa.value >= bb.value { aa } else { bb };
        let bound = BigFloat::add(
            &BigFloat::mul(&ma, &tol, bigfloat::precision()),
            &tol_abs,
            bigfloat::precision(),
        );
        let diff = bf_abs(BigFloat::sub(a, b, bigfloat::precision()));
        diff.value.abs() <= bound.value.abs()
    };

    // 相邻差
    let diffs: Vec<BigFloat> = vals
        .windows(2)
        .map(|w| BigFloat::sub(&w[1], &w[0], bigfloat::precision()))
        .collect();
    if diffs.is_empty() || diffs[0].value == BigInt::from(0) {
        return None;
    }

    // 结构判定：单一等差 或 两组交替差
    //
    // ⚠️ 直接 `let d = if … { … } else { … };`：先声明后赋值会被
    // clippy 的 `needless_late_init` 拦下（CI 的 stable 比本地 toolchain 新）。
    let ref0 = diffs[0].clone();
    let d: BigFloat = if diffs.iter().all(|x| same(x, &ref0)) {
        ref0.clone() // 单族
    } else {
        // 检查交替（a, b, a, b, ...）：相邻差恰好两类轮流出现
        let a = ref0.clone();
        let mut b: Option<BigFloat> = None;
        let mut ok = true;
        for (i, x) in diffs.iter().enumerate() {
            if i % 2 == 0 {
                if !same(x, &a) {
                    ok = false;
                    break;
                }
            } else {
                match &b {
                    None => b = Some(x.clone()),
                    Some(bb) => {
                        if !same(x, bb) {
                            ok = false;
                            break;
                        }
                    }
                }
            }
        }
        let bb = b?;
        if !ok || same(&a, &bb) {
            return None;
        }
        // 公周期 = a + b（两族交错的堆叠周期）
        BigFloat::add(&a, &bb, bigfloat::precision())
    };

    // 计算残基集合：每根归一到 [0, d)，按容差去重
    let zero = BigFloat::from_u64(0);
    let mut residuals: Vec<BigFloat> = Vec::new();
    for v in &vals {
        let mut r = v.clone();
        let mut guard = 0;
        while r.value < zero.value && guard < 100 {
            r = BigFloat::add(&r, &d, bigfloat::precision());
            guard += 1;
        }
        guard = 0;
        while r.value >= d.value && guard < 100 {
            r = BigFloat::sub(&r, &d, bigfloat::precision());
            guard += 1;
        }
        // 与 d 接近视为 0
        if same(&r, &d) {
            r = zero.clone();
        }
        // 落在绝对容差内的残基一律归零，保证去重可靠（避免重复家族）
        if bf_abs(r.clone()).value.abs() <= tol_abs.value.abs() {
            r = zero.clone();
        }
        if residuals.iter().any(|x| same(x, &r)) {
            continue;
        }
        residuals.push(r);
    }
    residuals.sort_by(|a, b| a.value.cmp(&b.value));

    // d 识别为 (p/q)·π：用通用有理逼近（分母上限 12），不再用固定候选表
    // ⇒ 也能覆盖 1/6、5/6、5/3 之类表外的角
    let (dp, dq) = pi_ratio(&d, &pi, PI_DEN_MAX, PI_TOL_REL)?;
    // 标签按当前角度单位渲染：弧度 → "π/6"；角度 → "30"
    let render = |p: i64, q: i64| -> String {
        match angle_mode {
            AngleMode::Radian => pi_frac_str(p, q),
            AngleMode::Degree => deg_str(p, q),
        }
    };
    let d_str = render(dp, dq);

    // 每个残基识别为 (p/q)·π（同样用通用有理逼近）；识别失败则整体退回数值列表。
    // "残基≈0"沿用旧实现的阈值 `10·π·tol ≈ 3.1e-5`（`tol_abs` 太紧，会把数值噪声误判成新家族）
    let zero_residual_tol = BigFloat::mul(
        &pi,
        &BigFloat::from_big_rational(&BigRational::new(BigInt::from(1), BigInt::from(100_000))),
        prec,
    );
    let mut family: Vec<String> = Vec::new();
    // 按"识别结果标签"去重：数值上略有差异、但会被识别成同一族的残基只输出一次。
    // 旧实现只按数值相对容差去重，残基约 1e-30 时（如 x·sin(x)=1e-30）彼此差异远大于容差，
    // 于是同一族被输出 7 遍（"k·π 或 k·π 或 …"）。
    let mut seen_labels: Vec<(i64, i64)> = Vec::new();
    for r in &residuals {
        let (label, body) = if bf_abs(r.clone()).value.abs() <= zero_residual_tol.value.abs() {
            ((0i64, 1i64), format!("k·{}", d_str))
        } else {
            // 残基无法识别为 π 的有理倍 → 整体退回数值列表
            let (p, q) = pi_ratio(r, &pi, PI_DEN_MAX, PI_TOL_REL_RESIDUAL)?;
            ((p, q), format!("{} + k·{}", render(p, q), d_str))
        };
        if !seen_labels.contains(&label) {
            seen_labels.push(label);
            family.push(body);
        }
    }
    if family.is_empty() {
        return None;
    }
    // 分隔符用两侧各两个空格：与 i18n 词条键 `"  或  "`（i18n.rs）保持一致，
    // 否则英文模式下 `或` 会残留成中文。
    Some(format!("{} = {}，k 为整数", var, family.join("  或  ")))
}

/// 把一个**不含未知量**的表达式取成有理数。
///
/// `as_rational()` 只认 `ExactTerm::Rational`，而 `e`、`pi` 这类常数在 AST 里是
/// `Exact(E(1))` / `Exact(PiPow(1,1))`，直接取必然失败（`e^x=2` 的闭式识别就是这样卡住的）。
/// 所以先试精确有理数，再退到求值器算一次数值——`detect_linear` 只在方程求解的最外层调用几次，
/// 这点求值开销可以忽略。
fn const_rational(ev: &Evaluator, e: &Expr) -> Option<BigRational> {
    match e {
        Expr::Number(n) => {
            if let Some(r) = n.as_rational() {
                return Some(r);
            }
            ev.evaluate_with_var(e, "\u{1}", &Number::from_int(0))
                .ok()
                .filter(|v| !v.is_complex())
                .map(|v| v.to_approx().to_big_rational())
        }
        _ => None,
    }
}

/// 尝试把表达式识别成"关于 `var` 的线性式" `a·var + b`，返回最简有理数 `(a, b)`。
/// 只认"常数 + 变量"的加减与"常数 × 变量"的乘，**不认变量之间相乘**（那已不是线性）。
fn detect_linear(ev: &Evaluator, e: &Expr, var: char) -> Option<(BigRational, BigRational)> {
    let zero = BigRational::from_integer(BigInt::from(0));
    match e {
        Expr::Number(_) => Some((zero, const_rational(ev, e)?)),
        Expr::Variable(v) => {
            if v.len() == 1 && v.starts_with(var) {
                Some((BigRational::from_integer(BigInt::from(1)), zero))
            } else {
                None
            }
        }
        Expr::Unary(crate::parser::UnaryOp::Neg, inner) => {
            let (a, b) = detect_linear(ev, inner, var)?;
            Some((-a, -b))
        }
        Expr::Unary(_, _) => None,
        Expr::Binary(l, crate::parser::BinOp::Add, r) => {
            let (a1, b1) = detect_linear(ev, l, var)?;
            let (a2, b2) = detect_linear(ev, r, var)?;
            Some((a1 + a2, b1 + b2))
        }
        Expr::Binary(l, crate::parser::BinOp::Sub, r) => {
            let (a1, b1) = detect_linear(ev, l, var)?;
            let (a2, b2) = detect_linear(ev, r, var)?;
            Some((a1 - a2, b1 - b2))
        }
        Expr::Binary(l, crate::parser::BinOp::Mul, r) => {
            // 只允许一边是纯常数（斜率 0）、另一边含变量；两边都含变量或都不含则不是线性
            let (a1, b1) = detect_linear(ev, l, var)?;
            let (a2, b2) = detect_linear(ev, r, var)?;
            let z = BigRational::from_integer(BigInt::from(0));
            match (a1 == z, a2 == z) {
                (true, false) => {
                    let c = b1;
                    Some((&c * &a2, &c * &b2))
                }
                (false, true) => {
                    let c = b2;
                    Some((&a1 * &c, &b1 * &c))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// `a^x = c` 型方程（`a` 为正的实常数、指数是 `var` 的线性式）的闭式解。
///
/// 解为 `x = (ln c / ln a - b) / k`，其中指数是 `k·var + b`。用现成的 `ln/exp` 算一次即可，
/// 不必让牛顿迭代从几十个初值各跑上百轮（实测 `e^x=2` 从约 470 ms 降到微秒级）。
///
/// 返回 `None` 表示"不认识这个形状或无法给出可信解"——调用方**原样回落到通用数值求根**，
/// 因此这里的保守不会损失任何能力。
pub fn try_exponential_equation(
    evaluator: &Evaluator,
    expr: &Expr,
    var: char,
) -> Option<(Number, &'static str)> {
    // 左边需要是"底数^指数"：AST 里是 Expr::Pow
    let Expr::Binary(lhs, crate::parser::BinOp::Sub, rhs) = expr else {
        return None;
    };
    let Expr::Pow(base, expo) = lhs.as_ref() else {
        return None;
    };

    // 底数与右端必须是"不含未知量"的常数
    let zero = BigRational::from_integer(BigInt::from(0));
    let base_lin = detect_linear(evaluator, base, var)?;
    if base_lin.0 != zero {
        return None; // 底数含变量（如 x^x）⇒ 不认识
    }
    let c_lin = detect_linear(evaluator, rhs, var)?;
    if c_lin.0 != zero {
        return None; // 右端含变量 ⇒ 不认识
    }

    // 指数必须是线性的：k·var + b
    let (k, b) = detect_linear(evaluator, expo, var)?;
    if k == zero {
        return None; // 指数不含变量 ⇒ 这是恒等式而非方程
    }

    let a_f = BigFloat::from_big_rational(&base_lin.1);
    let c_f = BigFloat::from_big_rational(&c_lin.1);
    // 底数必须为正且 ≠1（负底数的实数幂只在特殊点有定义；1^x 恒为 1）⇒ 交给通用路径
    //
    // ⚠️ 不要写成 `!(a_f > 0)`：`BigFloat` 是偏序，取反会**连带把"不可比"也当成
    // "不大于"**（clippy 的 neg_cmp_op_on_partial_ord 就是在提醒这一点）。
    // 这里显式用 `partial_cmp`：只有明确 `Greater` 才算"为正"。
    let zero = BigFloat::from_u64(0);
    let one = BigFloat::from_u64(1);
    let positive = |x: &BigFloat| matches!(x.partial_cmp(&zero), Some(std::cmp::Ordering::Greater));
    if !positive(&a_f) || a_f == one {
        return None;
    }
    // 右端必须为正（否则实数域无解；复数解交给通用路径）
    if !positive(&c_f) {
        return None;
    }

    // k 由 detect_linear 从常数运算构造，恒为有理数；只排除 k = 0（指数不含变量）
    let k_f = BigFloat::from_big_rational(&k);
    let b_f = BigFloat::from_big_rational(&b);

    // x = (ln c / ln a - b) / k
    let ln_c = c_f.ln(bigfloat::precision());
    let ln_a = a_f.ln(bigfloat::precision());
    if ln_a.is_zero() {
        return None;
    }
    let ratio = BigFloat::div(&ln_c, &ln_a, bigfloat::precision());
    let num = BigFloat::sub(&ratio, &b_f, bigfloat::precision());
    let x = BigFloat::div(&num, &k_f, bigfloat::precision());

    // **回代校验**：形状识别只看 AST，可能碰到"看起来像但语义不同"的情形
    // （例如底数或指数中含已存储变量被当作常数）。残差不过就放弃，交给通用路径。
    let xv = Number::Approx(x.clone());
    const VERIFY_LOG10: f64 = -30.0;
    match evaluator.evaluate_with_var(expr, &var.to_string(), &xv) {
        Ok(v) if !v.is_complex() => {
            let r = v.to_approx();
            if !(r.is_zero() || r.magnitude_log10() <= VERIFY_LOG10) {
                return None;
            }
        }
        _ => return None,
    }

    // 精确化：整数/简单分数的结果还原成精确形式，让显示与"手工解方程"一致
    // （`2^x=8` 必须仍然给 `x = 3`，而不是 `x ≈ 3.0000000000000000000`）
    let num_out = match float_to_exact_rational(&x) {
        Some((p, q)) if q != 0 => {
            Number::from_rational(BigRational::new(BigInt::from(p), BigInt::from(q)))
        }
        _ => Number::Approx(x),
    };
    let mode = evaluator.display_mode;
    Some((num_out.clone(), result_prefix(&num_out, mode)))
}

/* ---------------- 三角方程闭式快路 ---------------- */

/// `e` 是否含自变量 `var`
fn expr_has_var(e: &Expr, var: char) -> bool {
    match e {
        Expr::Number(_) => false,
        Expr::Variable(n) => n.starts_with(var),
        Expr::Binary(a, _, b) | Expr::Pow(a, b) => expr_has_var(a, var) || expr_has_var(b, var),
        Expr::Unary(_, a) | Expr::Sd(a) | Expr::Factor(a) => expr_has_var(a, var),
        Expr::Function(_, args) => args.iter().any(|a| expr_has_var(a, var)),
        Expr::Equation(l, r) => expr_has_var(l, var) || expr_has_var(r, var),
        Expr::System(items) => items.iter().any(|e| expr_has_var(e, var)),
    }
}

/// 不含自变量的子表达式求值（`pi/2`、`sqrt(2)/2` 这类常数都要能取到精确值）
fn const_value(ev: &Evaluator, e: &Expr) -> Option<Number> {
    ev.evaluate_with_vars(e, &[]).ok()
}

/// 两个子表达式是否**结构相同**。
///
/// `Expr` 与 `Number` 都没有实现 `PartialEq`（`Number` 把精确/近似混在一起，做相等判断很容易出错，
/// 见 `number.rs` 的说明），这里用 `Debug` 形式比较。只在"`sin(u)` 与 `cos(u)` 是否同相"这一处调用，
/// AST 很小、次数极少，开销可以忽略。
fn same_expr(a: &Expr, b: &Expr) -> bool {
    format!("{a:?}") == format!("{b:?}")
}

/// 三角线性形：`A·sin(u) + B·cos(u) + C·tan(u) + m`（`u` 必须同相）
struct TrigLinear {
    sin: Number,
    cos: Number,
    tan: Number,
    m: Number,
    /// 三者的公共内层参数；`None` 表示整式里没有三角节点
    arg: Option<Expr>,
}

impl TrigLinear {
    fn zero() -> Self {
        TrigLinear {
            sin: Number::from_int(0),
            cos: Number::from_int(0),
            tan: Number::from_int(0),
            m: Number::from_int(0),
            arg: None,
        }
    }

    /// 只有常数项
    fn constant(v: Number) -> Self {
        let mut t = Self::zero();
        t.m = v;
        t
    }

    fn is_pure_constant(&self) -> bool {
        self.sin.is_zero() && self.cos.is_zero() && self.tan.is_zero()
    }

    /// 不加/相减两个线性形
    fn combine(a: &Self, b: &Self, sub: bool) -> Self {
        let f = |x: &Number, y: &Number| if sub { x.sub(y) } else { x.add(y) };
        // 两个三角节点的内层参数必须一致：`sin(x) + cos(2x)` 不是同相，
        // 这种形状把 `arg` 置空，调用方据此放弃（走数值路径）。
        let arg = match (&a.arg, &b.arg) {
            (Some(x), Some(y)) if !same_expr(x, y) => None,
            (Some(x), _) => Some(x.clone()),
            (None, y) => y.clone(),
        };
        TrigLinear {
            sin: f(&a.sin, &b.sin),
            cos: f(&a.cos, &b.cos),
            tan: f(&a.tan, &b.tan),
            m: f(&a.m, &b.m),
            arg,
        }
    }

    /// 整体乘以常数 `k`
    fn scaled(&self, k: &Number) -> Self {
        TrigLinear {
            sin: self.sin.mul(k),
            cos: self.cos.mul(k),
            tan: self.tan.mul(k),
            m: self.m.mul(k),
            arg: self.arg.clone(),
        }
    }
}

/// 把 `e` 拆成 `A·sin(u) + B·cos(u) + C·tan(u) + m`：系数是与 `var` 无关的常数，
/// 三角节点的内层参数必须同相。形状不符（`sin²`、`x·sin(x)`、`sin(x)·cos(2x)`…）返回 `None`。
fn affine_in_trig(ev: &Evaluator, e: &Expr, var: char) -> Option<TrigLinear> {
    fn walk(e: &Expr, var: char, ev: &Evaluator) -> Option<TrigLinear> {
        match e {
            Expr::Number(n) => Some(TrigLinear::constant(n.clone())),
            Expr::Function(name, args)
                if args.len() == 1
                    && matches!(name.as_str(), "sin" | "cos" | "tan")
                    && expr_has_var(&args[0], var) =>
            {
                let mut t = TrigLinear::zero();
                t.arg = Some(args[0].clone());
                let one = Number::from_int(1);
                match name.as_str() {
                    "sin" => t.sin = one,
                    "cos" => t.cos = one,
                    _ => t.tan = one,
                }
                Some(t)
            }
            Expr::Unary(UnaryOp::Neg, a) => {
                let t = walk(a, var, ev)?;
                Some(t.scaled(&Number::from_int(-1)))
            }
            Expr::Unary(UnaryOp::Pos, a) => walk(a, var, ev),
            Expr::Binary(a, op @ (BinOp::Add | BinOp::Sub), b) => {
                let ta = walk(a, var, ev)?;
                let tb = walk(b, var, ev)?;
                Some(TrigLinear::combine(&ta, &tb, matches!(op, BinOp::Sub)))
            }
            Expr::Binary(a, BinOp::Mul, b) => {
                let ta = walk(a, var, ev)?;
                let tb = walk(b, var, ev)?;
                // 只允许"常数 × 线性项"；两边都含三角节点（`sin·cos`）一律放弃
                if ta.is_pure_constant() {
                    Some(tb.scaled(&ta.m))
                } else if tb.is_pure_constant() {
                    Some(ta.scaled(&tb.m))
                } else {
                    None
                }
            }
            Expr::Binary(a, BinOp::Div, b) => {
                let ta = walk(a, var, ev)?;
                let tb = walk(b, var, ev)?;
                // 只允许"线性项 ÷ 常数"（`1/sin(x)` 这类不算）
                if tb.is_pure_constant() && !tb.m.is_zero() {
                    Some(ta.scaled(&Number::from_int(1).div(&tb.m)))
                } else {
                    None
                }
            }
            other => {
                if expr_has_var(other, var) {
                    None
                } else {
                    Some(TrigLinear::constant(const_value(ev, other)?))
                }
            }
        }
    }
    walk(e, var, ev)
}

/// 把 `e` 拆成 `a·var + b`（`a`、`b` 为常数）；不是线性就 `None`
fn affine_in_var(ev: &Evaluator, e: &Expr, var: char) -> Option<(Number, Number)> {
    match e {
        Expr::Number(n) => Some((Number::from_int(0), n.clone())),
        Expr::Variable(n) if n.starts_with(var) => Some((Number::from_int(1), Number::from_int(0))),
        Expr::Unary(UnaryOp::Neg, a) => {
            let (x, y) = affine_in_var(ev, a, var)?;
            Some((x.neg(), y.neg()))
        }
        Expr::Unary(UnaryOp::Pos, a) => affine_in_var(ev, a, var),
        Expr::Binary(a, op @ (BinOp::Add | BinOp::Sub), b) => {
            let (x1, y1) = affine_in_var(ev, a, var)?;
            let (x2, y2) = affine_in_var(ev, b, var)?;
            if matches!(op, BinOp::Add) {
                Some((x1.add(&x2), y1.add(&y2)))
            } else {
                Some((x1.sub(&x2), y1.sub(&y2)))
            }
        }
        Expr::Binary(a, BinOp::Mul, b) => {
            let (x1, y1) = affine_in_var(ev, a, var)?;
            let (x2, y2) = affine_in_var(ev, b, var)?;
            if x1.is_zero() {
                Some((y1.mul(&x2), y1.mul(&y2)))
            } else if x2.is_zero() {
                Some((y2.mul(&x1), y2.mul(&y1)))
            } else {
                None
            }
        }
        Expr::Binary(a, BinOp::Div, b) => {
            let (x1, y1) = affine_in_var(ev, a, var)?;
            let (x2, y2) = affine_in_var(ev, b, var)?;
            if x2.is_zero() && !y2.is_zero() {
                Some((x1.div(&y2), y1.div(&y2)))
            } else {
                None
            }
        }
        other => {
            if expr_has_var(other, var) {
                None
            } else {
                Some((Number::from_int(0), const_value(ev, other)?))
            }
        }
    }
}

/// 用户单位 → 弧度（度模式下 `b` 是"度数"，要折成弧度才能与反三角的主值相加）
fn unit_to_radians(v: &Number, mode: AngleMode) -> Number {
    match mode {
        AngleMode::Radian => v.clone(),
        AngleMode::Degree => {
            let pi_over_180 = BigRational::new(BigInt::from(1), BigInt::from(180));
            v.mul(&Number::from_pi_times(pi_over_180))
        }
    }
}

/// 弧度 → 用户单位（度模式下把精确 `Pi(coeff)` 精确折成 `coeff·180`）
fn radians_to_unit(v: &Number, mode: AngleMode) -> Number {
    match mode {
        AngleMode::Radian => v.clone(),
        AngleMode::Degree => crate::parser::radians_to_degrees(v.clone()),
    }
}

/// `A·sin(u) + B·cos(u) = √(A²+B²)·sin(u + φ)` 里的相位 `φ`（弧度、精确）。
///
/// `φ = atan2(B, A)`，而 `trig::try_exact_arctan` 只覆盖 `(−π/2, π/2)` ⇒ `A < 0` 时
/// 按 `B` 的符号平移 `±π` 补回第二/三象限；比值不是特殊角（`try_exact_arctan` 给 `None`）就放弃。
fn exact_phase(a: &Number, b: &Number) -> Option<Number> {
    let pi = Number::from_pi_times(BigRational::from_integer(BigInt::from(1)));
    let base = trig::try_exact_arctan(&b.div(a))?;
    if !a.is_negative() {
        return Some(base);
    }
    Some(if b.is_negative() {
        base.sub(&pi)
    } else {
        base.add(&pi)
    })
}

/// 闭式快路判定周期所需的最少根数：根由家族**完整**构造，3 个就够定单族（见 `format_periodic_roots_min`）
const MIN_ROOTS_FOR_CLOSED_FORM: usize = 3;

/// `F(u) = c` 的主值（弧度）与周期：返回若干 `(主值, 周期)`；常数不是特殊角返回 `None`
fn principal_angles(fname: &str, c: &Number) -> Option<Vec<(Number, Number)>> {
    let pi = || Number::from_pi_times(BigRational::from_integer(BigInt::from(1)));
    let two_pi = || Number::from_pi_times(BigRational::from_integer(BigInt::from(2)));
    match fname {
        // sin(u) = c ⇒ u = α + 2kπ 或 π − α + 2kπ
        "sin" => {
            let a = trig::try_exact_arcsin(c)?;
            let b = pi().sub(&a);
            Some(vec![(a, two_pi()), (b, two_pi())])
        }
        // cos(u) = c ⇒ u = ±α + 2kπ
        "cos" => {
            let a = trig::try_exact_arccos(c)?;
            let b = a.neg();
            Some(vec![(a, two_pi()), (b, two_pi())])
        }
        // tan(u) = c ⇒ u = α + kπ
        "tan" => Some(vec![(trig::try_exact_arctan(c)?, pi())]),
        _ => None,
    }
}

/// 去掉数值上完全相同的根（同一族被两个主值各生成一遍的情况：`cos(x) = ±1`）
fn dedup_exact(roots: &mut Vec<Number>) {
    let mut out: Vec<Number> = Vec::with_capacity(roots.len());
    for r in roots.drain(..) {
        if !out.iter().any(|x| x.sub(&r).is_zero()) {
            out.push(r);
        }
    }
    *roots = out;
}

/// 三角方程闭式快路：`k·F(a·x + b) + m = 0`（`F ∈ {sin, cos, tan}`）。
///
/// **为什么值得做**：这类方程现在要走"约 50 个初值 × 最多 20 轮牛顿"的数值扫根，
/// 只为了**认出它是周期方程** —— 根全部用 80 位精度算出来，最后却只输出一行通式。
/// 实测（本机、度模式）：`tan(x)=1` 1869 ms、`sin(x)=1` 1533 ms、`sin(x)=0.5` 438 ms。
///
/// **做法**：用**精确反三角表**直接给出主值（`1/2 → π/6`、`1 → π/2`…），把各家族在 ±3π
/// 窗口内的成员（都是精确值）全部构造出来，再交给既有的 `format_periodic_roots` 出通式 ——
/// 渲染与数值路径**同源**，不必另写一套格式，也就不会出现"两套格式慢慢漂移"。
///
/// 下面三种情况一律返回 `None`，由调用方照旧走数值路径（那些情况的输出因此**逐字不变**）：
/// 形状不匹配、常数不是特殊角（`cos(x)=0.3`）、通式判不出来（周期不是 π 的有理倍，如 `sin(pi*x)=0`）。
pub fn trig_closed_form(ev: &Evaluator, eq_expr: &Expr, var: char) -> Option<String> {
    let lin = affine_in_trig(ev, eq_expr, var)?;
    if lin.is_pure_constant() {
        return None;
    }
    let u = lin.arg.clone()?;
    let (a, b) = affine_in_var(ev, &u, var)?;
    if a.is_zero() {
        return None;
    }
    // 归约成 `R·F(v) = c`（`v = u + φ`）：
    // - 只有 sin / 只有 cos ⇒ R 就是那个系数、φ = 0；
    // - `A·sin + B·cos` ⇒ `√(A²+B²)·sin(u + φ)`（φ 走精确反三角，非特殊角就放弃）；
    // - 只有 tan ⇒ `tan(u) = c`；`sin + tan` 这类混合形状直接放弃。
    let (fname, c, phase) = if !lin.tan.is_zero() {
        if !lin.sin.is_zero() || !lin.cos.is_zero() {
            return None;
        }
        ("tan", lin.m.neg().div(&lin.tan), Number::from_int(0))
    } else if lin.cos.is_zero() {
        ("sin", lin.m.neg().div(&lin.sin), Number::from_int(0))
    } else if lin.sin.is_zero() {
        ("cos", lin.m.neg().div(&lin.cos), Number::from_int(0))
    } else {
        let r = lin.sin.mul(&lin.sin).add(&lin.cos.mul(&lin.cos)).sqrt();
        if r.is_zero() {
            return None;
        }
        ("sin", lin.m.neg().div(&r), exact_phase(&lin.sin, &lin.cos)?)
    };
    let families = principal_angles(fname, &c)?;

    let angle_mode = ev.angle_mode;
    let prec = bigfloat::precision();
    let window = BigFloat::mul(&BigFloat::pi(prec), &BigFloat::from_u64(3), prec);
    let b_rad = unit_to_radians(&b, angle_mode);

    let a_f = bf_to_f64(&a.to_approx());
    let b_f = bf_to_f64(&b_rad.to_approx());
    if a_f == 0.0 || !a_f.is_finite() || !b_f.is_finite() {
        return None;
    }

    // 窗口条件 |x| ≤ 3π 等价于 u ∈ [b − 3π|a|, b + 3π|a|] ⇒ 直接算出 j 的范围，
    // 不必盲目地多生成一堆再筛（f64 只用来定边界，取整后再 ±1 留余量，最终仍按精确值筛）。
    let span = 3.0 * std::f64::consts::PI * a_f.abs();
    let mut roots: Vec<Number> = Vec::new();
    for (u0, period) in &families {
        let u0_f = bf_to_f64(&u0.to_approx());
        let p_f = bf_to_f64(&period.to_approx());
        if !p_f.is_finite() || p_f <= 0.0 || !u0_f.is_finite() {
            return None;
        }
        let j_min = ((b_f - span - u0_f) / p_f).floor() as i64 - 1;
        let j_max = ((b_f + span - u0_f) / p_f).ceil() as i64 + 1;
        // 周期太密（`sin(1000x)=0.5`）⇒ 窗口里塞得下几千个根，交给数值路径去处理
        if j_max - j_min > 512 {
            return None;
        }
        for j in j_min..=j_max {
            // v = u + φ ⇒ u = v − φ
            let uu = u0.sub(&phase).add(&period.mul(&Number::from_int(j)));
            let x_rad = uu.sub(&b_rad).div(&a);
            if bf_abs(x_rad.to_approx()).value.abs() <= window.value.abs() {
                roots.push(radians_to_unit(&x_rad, angle_mode));
            }
        }
    }
    // 两个主值可能是同一族（`sin(x)=1` 的 α 与 π−α 相等、`cos(x)=±1`）⇒ 先精确去重
    dedup_exact(&mut roots);
    // 根是**按家族构造**的，所以完整；窗口内只落到 3 个根也足以定出单族解
    // （`sin(x)=1` ⇒ `x = π/2 + k·2π`，数值扫根那条路认不出来、只会列出残缺的 3 个根）
    if roots.len() < MIN_ROOTS_FOR_CLOSED_FORM {
        return None;
    }
    format_periodic_roots_min(&roots, var, angle_mode, MIN_ROOTS_FOR_CLOSED_FORM)
}

/// 用多组初始猜测收集非多项式方程在区间内的所有实根（去重）
pub fn collect_all_roots(evaluator: &Evaluator, expr: &Expr, var: char) -> Vec<Number> {
    let mut guesses: Vec<BigFloat> = vec![BigFloat::from_u64(0)];
    for i in 1i64..=6 {
        guesses.push(BigFloat::from_i64(i));
        guesses.push(BigFloat::from_i64(-i));
    }
    let half = BigFloat::div(
        &BigFloat::from_u64(1),
        &BigFloat::from_u64(2),
        bigfloat::precision(),
    );
    for s in [1i64, -1, 3, -3] {
        let g = BigFloat::mul(&BigFloat::from_i64(s), &half, bigfloat::precision());
        guesses.push(g);
    }
    // 近零初值：`0` 与 `±0.5` 之间是空的，而 `ln(x)=c`（c 为负）这类方程的根可以非常小
    // —— `ln(x)=-10` 的真根是 4.5e-5，从 0.5/1.57/18.8 出发牛顿都会一步跳到负半轴（对数无定义）⇒ 丢根。
    // 加一列 10^-k 初值就能接住（牛顿对这类根收敛很稳；最终仍要过残差校验，不会造出假根）。
    for k in 2..=6u32 {
        let mut d = BigFloat::from_u64(1);
        for _ in 0..k {
            d = BigFloat::mul(&d, &BigFloat::from_u64(10), bigfloat::precision());
        }
        let g = BigFloat::div(&BigFloat::from_u64(1), &d, bigfloat::precision());
        guesses.push(g.clone());
        guesses.push(g.neg());
    }
    // π 及其半倍（覆盖三角函数零点）。
    //
    // **只在该方程真的含"以该变量为角度的三角函数"时才加**：这些初值本身很"远"
    // （±π..±6π 及其半倍，|x| 最大到 18.8），牛顿从远处奔向小根要十几步，
    // 而每一步都在求 `2^18.8` 这类**大指数幂**（实测单次 1~3ms，是整数指数时的百倍）。
    // 非三角方程用不到它们（±1..±6、±0.5/±1.5/±2.5/±3.5 已经覆盖了邻近区域，
    // 而且牛顿从邻近点收敛很快），去掉后实测 `2^x=8` 从 1.9s 降到 0.5s 量级。
    let pi = BigFloat::pi(bigfloat::precision());
    if crate::equation::has_trig_of_var(expr, var) {
        for k in 1i64..=6 {
            for s in [1i64, -1i64] {
                let kpi = BigFloat::mul(&BigFloat::from_i64(s * k), &pi, bigfloat::precision());
                guesses.push(kpi.clone());
                guesses.push(BigFloat::mul(&kpi, &half, bigfloat::precision()));
            }
        }
    }

    // 上面这套初值是按**弧度**校准的（`±1..±6`、`±k·π/2`）。若方程的自变量其实是**角度**
    // （出现了 `sin(x)` 这类"以该变量为角度的前向三角函数"）而当前又是角度模式，
    // 就必须整体 ×180/π 换算：否则 `±k·π ≈ ±3.14°` 这类初值太小、牛顿走不到 `30°/150°`，
    // 而 `±1..±6` 又会过冲落到远端根，根集合残缺 ⇒ 周期通式判不出来。
    // 换算后**牛顿轨迹与弧度模式在物理上完全一致**（Δx_度 = (180/π)·Δx_弧度），两种模式行为对齐。
    // 非三角方程（`1/x=2`、`ln(x)=1`）的自变量是普通实数，**必须保留原初值**，否则会丢根。
    if evaluator.angle_mode == AngleMode::Degree && crate::equation::has_trig_of_var(expr, var) {
        let scale = BigFloat::div(&BigFloat::from_u64(180), &pi, bigfloat::precision());
        guesses = guesses
            .into_iter()
            .map(|g| BigFloat::mul(&g, &scale, bigfloat::precision()))
            .collect();
    }

    // 两阶段：粗扫（20 次迭代上限、1e-8 去重）收候选，再对候选精收敛（80 次、1e-12 去重）
    let rough_tol = BigFloat::div(
        &BigFloat::from_u64(1),
        &BigInt::from(10).pow(8).into(),
        bigfloat::precision(),
    );
    let tol = BigFloat::div(
        &BigFloat::from_u64(1),
        &BigInt::from(10).pow(12).into(),
        bigfloat::precision(),
    );
    let root_close = |a: &BigFloat, b: &BigFloat, t: &BigFloat| {
        BigFloat::sub(a, b, bigfloat::precision()).value.abs() <= t.value.abs()
    };

    // 阶段一：粗扫
    let mut cands: Vec<Number> = Vec::new();
    for g in guesses {
        if let Some(r) = solver_poly::newton_solve(evaluator, expr, var, g, 20) {
            let dup = cands.iter().any(|x| match x {
                Number::Approx(bf) => root_close(bf, &r, &rough_tol),
                _ => false,
            });
            if !dup {
                cands.push(Number::Approx(r));
            }
        }
    }

    // 阶段二：精收敛
    let mut roots: Vec<Number> = Vec::new();
    for c in cands {
        if let Number::Approx(g0) = &c
            && let Some(r) = solver_poly::newton_solve(evaluator, expr, var, g0.clone(), 80)
        {
            let dup = roots.iter().any(|x| match x {
                Number::Approx(bf) => root_close(bf, &r, &tol),
                _ => false,
            });
            if !dup {
                roots.push(Number::Approx(r));
            }
        }
    }
    roots
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{BinOp, EvalResult, parse_and_eval};

    /// 单位角（度）的根
    fn deg(v: i64) -> Number {
        Number::from_int(v)
    }

    /// 弧度根：(p/q)·π
    fn pi_frac(p: i64, q: i64) -> Number {
        let prec = bigfloat::precision();
        let pi = BigFloat::pi(prec);
        Number::Approx(BigFloat::div(
            &BigFloat::mul(&BigFloat::from_i64(p), &pi, prec),
            &BigFloat::from_i64(q),
            prec,
        ))
    }

    /// 把 `sin(x)=0.5` 这样的等式变成 `sin(x)-0.5`（与 `handle_equation` 的第一步一致）
    fn eq_expr(input: &str) -> Expr {
        let mut ev = Evaluator::new();
        match parse_and_eval(input, &mut ev).expect("解析失败") {
            EvalResult::Equation(l, r) => Expr::Binary(l, BinOp::Sub, r),
            _ => panic!("{input} 不是等式"),
        }
    }

    fn solve(input: &str, angle_mode: AngleMode) -> Option<String> {
        let mut ev = Evaluator::new();
        ev.angle_mode = angle_mode;
        let expr = eq_expr(input);
        let roots = collect_all_roots(&ev, &expr, 'x');
        format_periodic_roots(&roots, 'x', angle_mode)
    }

    /// 闭式快路（绕开数值扫根）的结果
    fn closed(input: &str, angle_mode: AngleMode) -> Option<String> {
        let mut ev = Evaluator::new();
        ev.angle_mode = angle_mode;
        trig_closed_form(&ev, &eq_expr(input), 'x')
    }

    #[test]
    fn closed_form_covers_special_angle_equations() {
        for (input, want) in [
            ("sin(x)=0.5", "x = π/6 + k·2π  或  5π/6 + k·2π，k 为整数"),
            ("cos(x)=0.5", "x = π/3 + k·2π  或  5π/3 + k·2π，k 为整数"),
            ("sin(x)=0", "x = k·π，k 为整数"),
            ("cos(x)=0", "x = π/2 + k·π，k 为整数"),
            ("tan(x)=1", "x = π/4 + k·π，k 为整数"),
            ("tan(x)=sqrt(3)", "x = π/3 + k·π，k 为整数"),
            ("sin(2*x)=0.5", "x = π/12 + k·π  或  5π/12 + k·π，k 为整数"),
            ("2*sin(x)=1", "x = π/6 + k·2π  或  5π/6 + k·2π，k 为整数"),
            ("sin(x)=-1/2", "x = 7π/6 + k·2π  或  11π/6 + k·2π，k 为整数"),
            ("cos(x)=-1", "x = π + k·2π，k 为整数"),
            // `A·sin(u) + B·cos(u)` 走"合成一个相位"的路
            ("sin(x)+cos(x)=1", "x = k·2π  或  π/2 + k·2π，k 为整数"),
            ("sin(x)+cos(x)=0", "x = 3π/4 + k·π，k 为整数"),
            ("sin(x)=cos(x)", "x = π/4 + k·π，k 为整数"),
        ] {
            assert_eq!(
                closed(input, AngleMode::Radian).as_deref(),
                Some(want),
                "{input}"
            );
        }
        // 度模式：标签换成度数，通式必须一致
        assert_eq!(
            closed("sin(x)=0.5", AngleMode::Degree).as_deref(),
            Some("x = 30 + k·360  或  150 + k·360，k 为整数")
        );
        assert_eq!(
            closed("sin(x)+cos(x)=1", AngleMode::Degree).as_deref(),
            Some("x = k·360  或  90 + k·360，k 为整数")
        );
    }

    #[test]
    fn closed_form_declines_shapes_it_cannot_promise() {
        // 这些一律返回 None ⇒ 走数值扫根，输出与加快路之前逐字相同
        for input in [
            "cos(x)=0.3",        // 常数不是特殊角
            "sin(x)^2=1/4",      // 平方不是线性形
            "sin(pi*x)=0",       // 周期不是 π 的有理倍，通式判不出来
            "sin(x)+cos(x)=3",   // |c| > √(A²+B²)，无实根
            "sin(x)+cos(2*x)=1", // 两个三角节点不同相
            "x*sin(x)=1",        // 系数含自变量
            "1/sin(x)=2",        // 三角在分母
            "sin(x)+tan(x)=1",   // sin 与 tan 混合
            "ln(x)=1",           // 根本不是三角方程
            "e^x=2",
        ] {
            assert!(
                closed(input, AngleMode::Radian).is_none(),
                "{input} 不该走闭式快路"
            );
        }
    }

    #[test]
    fn deg_str_matches_the_pi_table() {
        // 旧 d_m / r_c 两张表在度模式下的全部取值
        for (p, q, want) in [
            (1, 2, "90"),
            (1, 1, "180"),
            (3, 2, "270"),
            (2, 1, "360"),
            (3, 1, "540"),
            (4, 1, "720"),
            (6, 1, "1080"),
            (8, 1, "1440"),
            (2, 3, "120"),
            (4, 3, "240"),
            (1, 3, "60"),
            (1, 4, "45"),
            (3, 4, "135"),
            (1, 6, "30"),
            (5, 6, "150"),
            (1, 8, "22.5"),
            (3, 8, "67.5"),
            (5, 8, "112.5"),
            (7, 8, "157.5"),
            (1, 12, "15"),
            (5, 12, "75"),
            (5, 3, "300"),
            (0, 1, "0"),
        ] {
            assert_eq!(deg_str(p, q), want, "deg_str({p},{q})");
        }
        // 约分：10π/6 = 5π/3
        assert_eq!(deg_str(10, 6), "300");
        assert_eq!(pi_frac_str(10, 6), "5π/3");
    }

    #[test]
    fn best_rational_picks_simplest_fraction() {
        assert_eq!(best_rational(1.0, 12), (1, 1));
        assert_eq!(best_rational(2.0, 12), (2, 1));
        assert_eq!(best_rational(1.0 / 6.0, 12), (1, 6));
        assert_eq!(best_rational(5.0 / 6.0, 12), (5, 6));
        assert_eq!(best_rational(5.0 / 3.0, 12), (5, 3));
        assert_eq!(best_rational(0.75, 12), (3, 4));
    }

    #[test]
    fn radian_formulas_unchanged() {
        // k·π
        let roots: Vec<Number> = (-3..=3).map(|k| pi_frac(k, 1)).collect();
        assert_eq!(
            format_periodic_roots(&roots, 'x', AngleMode::Radian).as_deref(),
            Some("x = k·π，k 为整数")
        );
        // π/2 + k·π
        let roots: Vec<Number> = (-3..=3).map(|k| pi_frac(2 * k + 1, 2)).collect();
        assert_eq!(
            format_periodic_roots(&roots, 'x', AngleMode::Radian).as_deref(),
            Some("x = π/2 + k·π，k 为整数")
        );
        // π/4 + k·π
        let roots: Vec<Number> = (-3..=3).map(|k| pi_frac(4 * k + 1, 4)).collect();
        assert_eq!(
            format_periodic_roots(&roots, 'x', AngleMode::Radian).as_deref(),
            Some("x = π/4 + k·π，k 为整数")
        );
        // 3π/4 + k·π
        let roots: Vec<Number> = (-3..=3).map(|k| pi_frac(4 * k + 3, 4)).collect();
        assert_eq!(
            format_periodic_roots(&roots, 'x', AngleMode::Radian).as_deref(),
            Some("x = 3π/4 + k·π，k 为整数")
        );
    }

    #[test]
    fn degree_formulas_from_synthetic_roots() {
        // 单族：k·180
        let roots: Vec<Number> = (-3..=3).map(|k| deg(k * 180)).collect();
        assert_eq!(
            format_periodic_roots(&roots, 'x', AngleMode::Degree).as_deref(),
            Some("x = k·180，k 为整数")
        );
        // 两族：30 + k·360  或  150 + k·360
        let mut roots: Vec<Number> = Vec::new();
        for k in -2..=2 {
            roots.push(deg(30 + 360 * k));
            roots.push(deg(150 + 360 * k));
        }
        assert_eq!(
            format_periodic_roots(&roots, 'x', AngleMode::Degree).as_deref(),
            Some("x = 30 + k·360  或  150 + k·360，k 为整数")
        );
        // 两族：k·360  或  90 + k·360
        let mut roots: Vec<Number> = Vec::new();
        for k in -2..=2 {
            roots.push(deg(360 * k));
            roots.push(deg(90 + 360 * k));
        }
        assert_eq!(
            format_periodic_roots(&roots, 'x', AngleMode::Degree).as_deref(),
            Some("x = k·360  或  90 + k·360，k 为整数")
        );
    }

    #[test]
    fn end_to_end_trig_equations_both_modes() {
        // 弧度：既有行为
        assert_eq!(
            solve("sin(x)=0.5", AngleMode::Radian).as_deref(),
            Some("x = π/6 + k·2π  或  5π/6 + k·2π，k 为整数")
        );
        assert_eq!(
            solve("cos(x)=0", AngleMode::Radian).as_deref(),
            Some("x = π/2 + k·π，k 为整数")
        );
        // 角度：本次修复的核心
        assert_eq!(
            solve("sin(x)=0.5", AngleMode::Degree).as_deref(),
            Some("x = 30 + k·360  或  150 + k·360，k 为整数")
        );
        assert_eq!(
            solve("cos(x)=0", AngleMode::Degree).as_deref(),
            Some("x = 90 + k·180，k 为整数")
        );
        assert_eq!(
            solve("tan(x)=1", AngleMode::Degree).as_deref(),
            Some("x = 45 + k·180，k 为整数")
        );
        assert_eq!(
            solve("sin(x)+cos(x)=0", AngleMode::Degree).as_deref(),
            Some("x = 135 + k·180，k 为整数")
        );
        // 原先两种模式都只能列数值根的两例，现已修复
        assert_eq!(
            solve("cos(x)=0.5", AngleMode::Radian).as_deref(),
            Some("x = π/3 + k·2π  或  5π/3 + k·2π，k 为整数")
        );
        assert_eq!(
            solve("cos(x)=0.5", AngleMode::Degree).as_deref(),
            Some("x = 60 + k·360  或  300 + k·360，k 为整数")
        );
        assert_eq!(
            solve("sin(x)+cos(x)=1", AngleMode::Radian).as_deref(),
            Some("x = k·2π  或  π/2 + k·2π，k 为整数")
        );
        assert_eq!(
            solve("sin(x)+cos(x)=1", AngleMode::Degree).as_deref(),
            Some("x = k·360  或  90 + k·360，k 为整数")
        );
    }

    #[test]
    fn non_trig_roots_survive_in_degree_mode() {
        // 度模式下非三角方程的自变量不是角度：初值不得被换算，否则会丢根
        let mut ev = Evaluator::new();
        ev.angle_mode = AngleMode::Degree;
        let roots = collect_all_roots(&ev, &eq_expr("1/x=2"), 'x');
        assert!(
            roots
                .iter()
                .any(|r| { float_to_exact_rational(&r.to_approx()) == Some((1, 2)) }),
            "度模式下 1/x=2 应仍找到 0.5，实得 {:?}",
            roots.len()
        );
    }

    #[test]
    fn complex_domain_seeds_do_not_panic() {
        // 回归：初值 -1 会让 ln(-1) 落入复数域，`newton_solve` 直接取实部近似曾 panic
        let ev = Evaluator::new();
        let roots = collect_all_roots(&ev, &eq_expr("ln(x)=1"), 'x');
        assert!(!roots.is_empty(), "ln(x)=1 应找到 e");
        let roots = collect_all_roots(&ev, &eq_expr("sqrt(x)=2"), 'x');
        assert!(
            roots
                .iter()
                .any(|r| float_to_exact_rational(&r.to_approx()) == Some((4, 1))),
            "sqrt(x)=2 应找到 4"
        );
    }
}
