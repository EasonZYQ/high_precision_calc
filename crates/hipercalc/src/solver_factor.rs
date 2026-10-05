use num_bigint::BigInt;
use num_integer::Integer;
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive, Zero};
use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::bigfloat::BigFloat;
use crate::number::Number;
use crate::parser::{DisplayMode, Evaluator, Expr};

/// 变量指数向量（长度 = 变量数）
type Mono = Vec<u32>;

/// 单项式 * 有理系数
#[derive(Debug, Clone)]
struct Term {
    mono: Mono,
    coeff: BigRational,
}

/// 多项式（规范化：合并同类项、去零、字典序降序）
#[derive(Debug, Clone)]
struct Poly {
    terms: Vec<Term>,
}

impl Poly {
    /// 最高单项式总次数
    fn total_deg(&self) -> u32 {
        self.terms.iter().map(|t| t.mono.iter().sum()).max().unwrap_or(0)
    }
}

/// 因式表示
#[derive(Debug, Clone)]
enum FactorRep {
    /// 常规有理系数因式（多项式 + 重数）
    Poly(Poly, u32),
    /// 二次不可约因式的两个根式根: (x - (p + q*sqrt(d)))(x - (p - q*sqrt(d)))，重数 exp
    SqrtPair {
        p: BigRational,
        q: BigRational,
        d: BigInt,
        exp: u32,
    },
}

/* ---------------- 多项式基本运算 ---------------- */

fn mono_total(m: &Mono) -> u32 {
    m.iter().sum()
}

/// 比较单项式（字典序降序）
fn mono_cmp(a: &Mono, b: &Mono) -> Ordering {
    for (x, y) in a.iter().zip(b.iter()) {
        match y.cmp(x) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    debug_assert_eq!(a.len(), b.len(), "单项式维度应一致");
    Ordering::Equal
}

/// 规范化构造（BTreeMap 合并同类项，O(n log n)）
fn poly_of(items: Vec<(Mono, BigRational)>) -> Poly {
    let mut map: BTreeMap<Mono, BigRational> = BTreeMap::new();
    for (mono, coeff) in items {
        if coeff.is_zero() {
            continue;
        }
        *map.entry(mono).or_insert_with(BigRational::zero) += &coeff;
    }
    let mut terms: Vec<Term> = map
        .into_iter()
        .filter(|(_, c)| !c.is_zero())
        .map(|(mono, coeff)| Term { mono, coeff })
        .collect();
    terms.sort_by(|a, b| mono_cmp(&a.mono, &b.mono));
    Poly { terms }
}

fn poly_const(r: &BigRational) -> Poly {
    poly_of(vec![(Vec::new(), r.clone())])
}

/// 线性多项式: coeffs[0..n] 为各变量系数，coeffs[n]（若有）为常数项
fn poly_linear(vars: &[char], coeffs: &[BigRational]) -> Poly {
    let mut terms = Vec::new();
    for (i, c) in coeffs.iter().enumerate() {
        if c.is_zero() {
            continue;
        }
        let mut mono = vec![0u32; vars.len()];
        if i < vars.len() {
            mono[i] = 1;
        }
        terms.push((mono, c.clone()));
    }
    poly_of(terms)
}

fn poly_sub(a: &Poly, b: &Poly) -> Poly {
    let ab: Vec<(Mono, BigRational)> =
        a.terms.iter().map(|t| (t.mono.clone(), t.coeff.clone())).collect();
    let mut items = ab;
    for t in &b.terms {
        items.push((t.mono.clone(), -&t.coeff));
    }
    poly_of(items)
}

fn poly_mul(a: &Poly, b: &Poly) -> Poly {
    let mut items = Vec::new();
    for ta in &a.terms {
        for tb in &b.terms {
            let mono: Mono = ta.mono.iter().zip(tb.mono.iter()).map(|(x, y)| x + y).collect();
            items.push((mono, &ta.coeff * &tb.coeff));
        }
    }
    poly_of(items)
}

fn poly_equal(a: &Poly, b: &Poly) -> bool {
    if a.terms.len() != b.terms.len() {
        return false;
    }
    for t in &a.terms {
        if !b.terms.iter().any(|u| u.mono == t.mono && u.coeff == t.coeff) {
            return false;
        }
    }
    true
}

fn is_constant(p: &Poly) -> bool {
    // 注意：空多项式（poly_of 会过滤掉零系数项，poly_const(0) 即得到空 terms）也满足
    // `all()` 而返回 true。取系数前必须先判 `terms` 非空，见 format_factor 的用法。
    p.terms.iter().all(|t| mono_total(&t.mono) == 0)
}

/// 是否所有项次数 <= 1（线性）
fn is_linear(p: &Poly) -> bool {
    p.terms.iter().all(|t| mono_total(&t.mono) <= 1)
}

/// 提取常数因子（各系数分子 abs 的 gcd，并归一化符号使首项系数为正）
fn extract_content(p: &Poly) -> (BigRational, Poly) {
    let mut gcd = BigInt::zero();
    for t in &p.terms {
        let a = t.coeff.numer().abs();
        if gcd.is_zero() {
            gcd = a.clone();
        } else {
            gcd = gcd.gcd(&a);
        }
    }
    if gcd.is_zero() {
        return (BigRational::one(), p.clone());
    }
    let scale = if p.terms[0].coeff.is_negative() {
        -BigRational::from_integer(gcd.clone())
    } else {
        BigRational::from_integer(gcd.clone())
    };
    let rest: Vec<(Mono, BigRational)> =
        p.terms.iter().map(|t| (t.mono.clone(), &t.coeff / &scale)).collect();
    (scale, poly_of(rest))
}

/// 提取各变量最低次幂的公因子: (公因子单项式, 剩余多项式)
fn extract_common_monomial(p: &Poly, n: usize) -> (Mono, Poly) {
    if p.terms.is_empty() {
        return (vec![0u32; n], p.clone());
    }
    let mut min = vec![u32::MAX; n];
    for t in &p.terms {
        for (i, e) in t.mono.iter().enumerate() {
            if *e < min[i] {
                min[i] = *e;
            }
        }
    }
    if min.iter().all(|&e| e == 0) {
        return (min, p.clone());
    }
    let rest: Vec<(Mono, BigRational)> = p
        .terms
        .iter()
        .map(|t| {
            let m: Mono = t.mono.iter().zip(min.iter()).map(|(x, y)| x - y).collect();
            (m, t.coeff.clone())
        })
        .collect();
    (min, poly_of(rest))
}

/// 按主变量长除: p / lin，返回 (商, 余数)。整除时余数不含主变量。
fn div_linear_by_pivot(p: &Poly, lin: &Poly, pivot: usize) -> Option<(Poly, Poly)> {
    let ap = lin
        .terms
        .iter()
        .find(|t| t.mono[pivot] == 1)
        .map(|t| t.coeff.clone())
        .unwrap_or_else(|| BigRational::zero());
    if ap.is_zero() {
        return None;
    }
    let mut rem = p.clone();
    let mut quo: Vec<(Mono, BigRational)> = Vec::new();
    loop {
        if rem.terms.is_empty() {
            break;
        }
        let lt_idx = rem
            .terms
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| {
                a.mono[pivot].cmp(&b.mono[pivot]).then_with(|| mono_cmp(&a.mono, &b.mono))
            })
            .unwrap()
            .0;
        let lt = rem.terms[lt_idx].clone();
        if lt.mono[pivot] == 0 {
            break;
        }
        let mut qmono = lt.mono.clone();
        qmono[pivot] -= 1;
        let qcoeff = &lt.coeff / &ap;
        quo.push((qmono.clone(), qcoeff.clone()));
        let qpoly = poly_of(vec![(qmono, qcoeff)]);
        let sub = poly_mul(&qpoly, lin);
        rem = poly_sub(&rem, &sub);
    }
    if rem.terms.iter().any(|t| t.mono[pivot] != 0) {
        return None;
    }
    Some((poly_of(quo), rem))
}

/* ---------------- 单变量因子工具 ---------------- */

/// 整数正因子。None 表示放弃枚举：
/// - 快速模式：|n| > 10^13 直接放弃（上层提示"结果可能不完整"）；
/// - 死算模式（/mode deep）：不设上限，超出 u64 时退回 BigInt 试除（可能极慢）。
///
/// 实现说明：快速路径下试除用 u64 原生取模完成（10^13 以内的输入都可放进 u64），
/// 避免每次迭代分配 BigInt（旧实现 3×10^6 次 BigInt 取模在 debug 构建下约需 3 秒）。
fn int_divisors(n: &BigInt) -> Option<Vec<BigInt>> {
    let n = n.abs();
    if n.is_zero() {
        return None;
    }
    let deep = crate::calc_mode::is_deep();
    if !deep && n > BigInt::from(10_000_000_000_000u64) {
        return None;
    }
    let mut out = vec![BigInt::one()];
    match n.to_u64() {
        Some(nu) => {
            // sqrt 为整数开方（向下取整），i ≤ sqrt ⇒ i*i ≤ n ≤ u64::MAX，不会溢出
            let sqrt = crate::bigint_ext::int_sqrt(&n).to_u64().unwrap_or(u64::MAX);
            let mut i = 2u64;
            while i <= sqrt {
                if nu % i == 0 {
                    out.push(BigInt::from(i));
                    let other = nu / i;
                    if other != i {
                        out.push(BigInt::from(other));
                    }
                }
                i += 1;
            }
        }
        None => {
            // 仅死算模式可达：BigInt 逐 1 试除
            let sqrt = crate::bigint_ext::int_sqrt(&n);
            let mut i = BigInt::from(2);
            while i <= sqrt {
                if &n % &i == BigInt::zero() {
                    out.push(i.clone());
                    let other = &n / &i;
                    if other != i {
                        out.push(other);
                    }
                }
                i += 1u32;
            }
        }
    }
    out.push(n);
    Some(out)
}

/// 有理根定理候选：r = ±p/q，p | 常数项分子，q | 首项系数分子。
/// 返回 (候选集合, 是否发生截断)，截断时上层应提示结果可能不完整。
fn rational_root_candidates(coeffs: &[BigRational]) -> (Vec<BigRational>, bool) {
    if coeffs.is_empty() {
        return (Vec::new(), false);
    }
    let ps = match int_divisors(coeffs[0].numer()) {
        Some(v) => v,
        None => return (Vec::new(), true),
    };
    let qs = match int_divisors(coeffs[coeffs.len() - 1].numer()) {
        Some(v) => v,
        None => return (Vec::new(), true),
    };
    if ps.is_empty() || qs.is_empty() {
        return (Vec::new(), true);
    }
    let mut set: Vec<BigRational> = Vec::new();
    let mut truncated = false;
    // 死算模式 (/mode deep) 不限制候选个数
    let deep = crate::calc_mode::is_deep();
    'outer: for p in &ps {
        for q in &qs {
            let r = BigRational::new(p.clone(), q.clone());
            if !set.contains(&r) {
                set.push(r.clone());
                set.push(-r);
            }
            if !deep && set.len() > 200 {
                truncated = true;
                break 'outer;
            }
        }
    }
    (set, truncated)
}

fn poly_eval(coeffs: &[BigRational], x: &BigRational) -> BigRational {
    let mut acc = BigRational::zero();
    for c in coeffs.iter().rev() {
        acc = acc * x + c;
    }
    acc
}

/// 综合除法除以 (x - r)，返回 (商, 余数)
fn synthetic_div(coeffs: &[BigRational], r: &BigRational) -> (Vec<BigRational>, BigRational) {
    let n = coeffs.len();
    let mut q = vec![BigRational::zero(); n.saturating_sub(1)];
    if n >= 2 {
        q[n - 2] = coeffs[n - 1].clone();
    }
    for i in (1..n.saturating_sub(1)).rev() {
        q[i - 1] = &coeffs[i] + &(&q[i] * r);
    }
    let r0 = if n >= 2 { &q[0] } else { &BigRational::zero() };
    let rem = &coeffs[0] + &(r0 * r);
    (q, rem)
}

/// 单变量有理根列表（用于多元投影；截断信息在此不传播）
fn rational_roots(coeffs: &[BigRational]) -> Vec<BigRational> {
    let mut c = coeffs.to_vec();
    let mut roots = Vec::new();
    while c.len() > 1 && c[0].is_zero() {
        roots.push(BigRational::zero());
        c.remove(0);
    }
    loop {
        if c.len() <= 1 {
            break;
        }
        let (cands, _) = rational_root_candidates(&c);
        let mut found = None;
        for r in cands {
            if poly_eval(&c, &r).is_zero() {
                found = Some(r);
                break;
            }
        }
        match found {
            Some(r) => {
                roots.push(r.clone());
                let (q, _) = synthetic_div(&c, &r);
                c = q;
            }
            None => break,
        }
    }
    roots
}

/// 化简 sqrt(n) = s*sqrt(d)，d 平方自由。
/// 先判定完全平方（整数平方根，代价极低），再按小质数试除提取平方因子。
/// 试除上限按"总工作量"折算（同 number.rs::simplify_radical）：每次试除都要对 m 取模，
/// 单次代价 ≈ O(m 的 limb 数)，固定 10^6 次在大 m 上会白跑几十秒；
/// 死算模式（/mode deep）不设上限 ⇒ 大 Δ 下可能长期运行
/// （旧实现固定 10^13 上限，遇到大数会真的线性跑上万亿次，界面等效卡死）。
fn simplify_sqrt(n: &BigInt) -> (BigInt, BigInt) {
    let n = n.abs();
    if n <= BigInt::one() {
        return (BigInt::one(), n);
    }
    // 完全平方直接返回（如 Δ = 4×10^24 这类，旧实现要走 10^12 次试除）
    let root = crate::bigint_ext::int_sqrt(&n);
    if &root * &root == n {
        return (root, BigInt::one());
    }
    let mut m = n.clone();
    let mut outside = BigInt::one();
    let deep = crate::calc_mode::is_deep();
    let words = (m.bits() as usize).div_ceil(64).max(1);
    let max_p = (2_000_000usize / words).clamp(256, 1_000_000) as u64;

    let mut p: u64 = 2;
    loop {
        if !deep && p > max_p {
            break;
        }
        // p² 用 u64 比较（p ≤ 10^6 ⇒ p² ≤ 10^12 不溢出），避免每轮都做 BigInt 乘法+分配
        let p2_u64 = p * p;
        let too_big = match m.to_u64() {
            Some(mu) => p2_u64 > mu,
            None => BigInt::from(p2_u64) > m,
        };
        if too_big {
            break;
        }
        let p_big = BigInt::from(p);
        let p2 = BigInt::from(p2_u64);
        while &m % &p2 == BigInt::zero() {
            m /= &p2;
            outside *= &p_big;
        }
        p += 1;
    }
    (outside, m)
}

/* ---------------- 显示 ---------------- */

fn format_rat(r: &BigRational, mode: DisplayMode) -> String {
    match mode {
        DisplayMode::MathIO => {
            if r.is_integer() {
                r.to_integer().to_string()
            } else {
                format!("{} / {}", r.numer(), r.denom())
            }
        }
        DisplayMode::LineIO => {
            let bf = BigFloat::from_big_rational(r);
            let s = bf.to_significant_string(crate::bigfloat::display_digits());
            if s == "-0" {
                "0".to_string()
            } else {
                s
            }
        }
    }
}

fn format_mono(mono: &Mono, vars: &[char]) -> String {
    let mut s = String::new();
    for (i, e) in mono.iter().enumerate() {
        if *e == 0 {
            continue;
        }
        if !s.is_empty() {
            s.push('*');
        }
        s.push(vars[i]);
        if *e > 1 {
            s.push('^');
            s.push_str(&e.to_string());
        }
    }
    s
}

/// 格式化多项式求和串
fn format_poly(p: &Poly, vars: &[char], mode: DisplayMode) -> String {
    if p.terms.is_empty() {
        return "0".to_string();
    }
    let mut out = String::new();
    let mut first = true;
    for t in &p.terms {
        let m = format_mono(&t.mono, vars);
        let c = &t.coeff;
        let abs_c = c.abs();
        if first {
            if m.is_empty() {
                out.push_str(&format_rat(c, mode));
            } else {
                if c.is_negative() {
                    out.push('-');
                }
                if abs_c != BigRational::one() {
                    out.push_str(&format_rat(&abs_c, mode));
                    out.push('*');
                }
                out.push_str(&m);
            }
        } else {
            if c.is_negative() {
                out.push_str(" - ");
            } else {
                out.push_str(" + ");
            }
            if m.is_empty() {
                out.push_str(&format_rat(&abs_c, mode));
            } else if abs_c == BigRational::one() {
                out.push_str(&m);
            } else {
                out.push_str(&format_rat(&abs_c, mode));
                out.push('*');
                out.push_str(&m);
            }
        }
        first = false;
    }
    out
}

/// 格式化线性因式（整数化归一）: "3*x - 1"、"x - y - 2"
fn format_linear_factor(poly: &Poly, vars: &[char]) -> String {
    let n = vars.len();
    let mut coeffs = vec![BigRational::zero(); n + 1];
    for t in &poly.terms {
        if mono_total(&t.mono) == 0 {
            coeffs[n] = t.coeff.clone();
        } else if let Some(pos) = t.mono.iter().position(|&e| e == 1) {
            coeffs[pos] = t.coeff.clone();
        }
    }
    // 整数化
    let mut lcm = BigInt::one();
    for c in &coeffs {
        lcm = lcm.lcm(c.denom());
    }
    let f = BigRational::from_integer(lcm);
    let mut ints: Vec<BigInt> = coeffs.iter().map(|c| (c * &f).to_integer()).collect();
    // 符号归一
    if let Some(first) = ints.iter().find(|i| !i.is_zero()) {
        if first.is_negative() {
            for _i in ints.iter_mut() {
                *_i = -_i.clone();
            }
        }
    }
    let mut s = String::new();
    let mut first = true;
    for (idx, c) in ints.iter().enumerate() {
        if c.is_zero() {
            continue;
        }
        let abs_c = c.abs();
        if !first {
            if c.is_negative() {
                s.push_str(" - ");
            } else {
                s.push_str(" + ");
            }
        }
        if idx < n {
            if !abs_c.is_one() {
                s.push_str(&abs_c.to_string());
                s.push('*');
            }
            s.push(vars[idx]);
        } else {
            s.push_str(&abs_c.to_string());
        }
        first = false;
    }
    if first {
        s.push('0');
    }
    s
}

/// 根式根因数: 输出两因子 (x - (p + q*sqrt(d)))(x - (p - q*sqrt(d)))
fn format_sqrt_pair(p: &BigRational, q: &BigRational, d: &BigInt, mode: DisplayMode) -> String {
    // 两个根: r1 = p + q*sqrt(d), r2 = p - q*sqrt(d)
    let f1 = format_linear_surd(p, q, d, mode);
    let f2 = format_linear_surd(p, &(-q), d, mode);
    // 两个因式之间必须有 ` * `（旧实现直接拼接成 `(x - sqrt(2))(x + sqrt(2))`，
    // 与其他因式用 ` * ` 分隔的约定不一致）
    format!("{} * {}", f1, f2)
}

/// 单个线性因子 "x - (a + b*sqrt(d))"
fn format_linear_surd(a: &BigRational, b: &BigRational, d: &BigInt, mode: DisplayMode) -> String {
    // 因式 = x - (a + b*sqrt(d)) = x + (-a - b*sqrt(d))
    let np = -a;
    let nb = -b;
    let mut s = String::from("(x");
    if !np.is_zero() {
        if np.is_negative() {
            s.push_str(&format!(" - {}", format_rat(&(-&np), mode)));
        } else {
            s.push_str(&format!(" + {}", format_rat(&np, mode)));
        }
    }
    if !nb.is_zero() {
        // 化简根式前系数: nb = q * sqrt(d)，d 平方自由
        if nb.is_negative() {
            s.push_str(" - ");
        } else {
            s.push_str(" + ");
        }
        let q = nb.abs();
        if q != BigRational::one() {
            if q.is_integer() {
                s.push_str(&format_rat(&q, mode));
            } else {
                // 分数系数必须加括号（与 display.rs 的 `(1 / 2)*sqrt(2)` 约定一致）：
                // 裸写 `1 / 2*sqrt(2)` 容易被读成 1/(2√2)
                s.push_str(&format!("({})", format_rat(&q, mode)));
            }
            s.push('*');
        }
        s.push_str(&format!("sqrt({})", d));
    }
    s.push(')');
    s
}

/* ---------------- 分解 ---------------- */

/// 长除: f / (A*x^2 + B*x + C)，f 与因式为升幂系数数组。返回 (商升幂, 是否整除)
fn divide_by_quadratic(
    f: &[BigRational],
    a: &BigRational,
    b: &BigRational,
    c: &BigRational,
) -> (Vec<BigRational>, bool) {
    let n = f.len() - 1;
    let mut rem = f.to_vec();
    let mut quo = vec![BigRational::zero(); n.saturating_add(1)];
    for i in (2..=n).rev() {
        if rem[i].is_zero() {
            continue;
        }
        let lead = &rem[i] / a;
        quo[i - 2] = lead.clone();
        rem[i] = &rem[i] - &(&lead * a);
        rem[i - 1] = &rem[i - 1] - &(&lead * b);
        rem[i - 2] = &rem[i - 2] - &(&lead * c);
    }
    let ok = rem.iter().all(|x| x.is_zero());
    // 截掉高位尾零（保证下次以正确的最高次数继续提取因子）
    while quo.len() > 1 && quo[quo.len() - 1].is_zero() {
        quo.pop();
    }
    (quo, ok)
}

/// 二次因子提取时 B 枚举的上限
const QUADRATIC_BMAX_LIMIT: i64 = 200_000;

/// 二次因子枚举的试算次数预算。超出即截断并提示"结果可能不完整"。
/// 旧实现无预算：A/C 因子组合 × B 的 40 万次枚举可达上千万次大数长除，
/// `fac(x^4+9999999999998)` 这类输入会卡死数分钟。
/// 取 5×10^3：debug 构建下最坏约 2~3 秒（release 更快），足够覆盖小系数多项式的完整枚举。
const QUADRATIC_TRY_BUDGET: usize = 5_000;

/// 二次因子枚举的**工作量**预算（试算次数 × 多项式次数）。
/// 每次试算都要对 n 个系数做有理数长除（含大量 BigRational 分配与 gcd 归一），
/// 实测 n≈1000、5000 次试算需约 17 秒，即"次数 × 次数"每 1 万单位约 0.34 秒。
/// 取 4×10^4：最坏约 1.4 秒，同时低次多项式仍能用满 QUADRATIC_TRY_BUDGET（行为不变），
/// 高次多项式自动减少试算并置 truncated（提示"结果可能不完整"）。
const QUADRATIC_WORK_BUDGET: usize = 40_000;

/// 从首一整数多项式 f 的升幂系数中提取一个二次因子 (A x^2 + B x + C)。
/// A | 首项系数，C | 常数项，B 在柯西界内枚举（超上限则截断并置 truncated）。
/// 返回 (Some((g 升幂 [C,B,A], 商升幂)) 或 None, 是否发生截断)。
fn extract_quadratic_factor(
    coeffs: &[BigRational],
) -> (Option<(Vec<BigRational>, Vec<BigRational>)>, bool) {
    let n = coeffs.len() - 1;
    if n < 4 {
        return (None, false);
    }
    let lead = coeffs[n].clone();
    let c0 = coeffs[0].clone();
    if lead.is_zero() || c0.is_zero() {
        return (None, false);
    }
    let a_cands = match int_divisors(&lead.to_integer()) {
        Some(v) => v,
        None => return (None, true),
    };
    let c_cands = match int_divisors(&c0.to_integer()) {
        Some(v) => v,
        None => return (None, true),
    };
    let max_c = coeffs
        .iter()
        .map(|c| c.numer().abs())
        .max()
        .unwrap_or_else(|| BigInt::one());
    // 柯西界 |B| ≤ 2(1 + max|a_i|)，超过上限时截断并在返回值中标记
    let bmax = BigInt::from(2) * (BigInt::one() + max_c);
    let deep = crate::calc_mode::is_deep();
    let mut truncated = false;
    let bmax_i64 = match bmax.to_i64() {
        // 快速模式把 B 枚举截断到 QUADRATIC_BMAX_LIMIT；死算模式取完整柯西界（可能极慢）
        Some(v) if deep || v <= QUADRATIC_BMAX_LIMIT => v,
        _ => {
            truncated = true;
            QUADRATIC_BMAX_LIMIT
        }
    };

    let mut c_signed: Vec<BigRational> = Vec::new();
    for c in &c_cands {
        c_signed.push(BigRational::from_integer(c.clone()));
        c_signed.push(BigRational::from_integer(-c.clone()));
    }
    // 工作量折算：有效次数上限 = work / (n+1)，
    // 再与固定的次数上限取较小值（低次多项式行为不变，高次多项式自动收敛到少量试算）
    let work_tries = (QUADRATIC_WORK_BUDGET / (n + 1)).max(4);
    let try_budget = QUADRATIC_TRY_BUDGET.min(work_tries);
    if try_budget < QUADRATIC_TRY_BUDGET {
        truncated = true; // 预算被裁到不足以完整枚举 ⇒ 结果可能不完整
    }
    let mut tries = 0usize;
    for a in &a_cands {
        let ar = BigRational::from_integer(a.clone());
        for cr in &c_signed {
            for b in -bmax_i64..=bmax_i64 {
                tries += 1;
                if !deep && tries > try_budget {
                    // 预算耗尽：停止枚举并标记截断（宁可给"可能不完整"的结论，也不要卡死）
                    return (None, true);
                }
                let br = BigRational::from_integer(BigInt::from(b));
                let (quo, ok) = divide_by_quadratic(coeffs, &ar, &br, cr);
                if ok {
                    return (Some((vec![cr.clone(), br, ar], quo)), truncated);
                }
            }
        }
    }
    (None, truncated)
}

/// 将二次系数 (a, b, c) 转为因子表示并入列表。
/// 仅 MathIO 且 b == 0（形如 a*x^2 + c，平方差型）且 Δ ≥ 0 时展开为 SqrtPair / 线性；
/// 其余一律保留为整系数二次多项式。
/// 返回 true 表示已完全展开。
fn push_quad_factor(
    factors: &mut Vec<FactorRep>,
    ratio: &mut BigRational,
    vars: &[char],
    a: &BigRational,
    b: &BigRational,
    c: &BigRational,
    mode: DisplayMode,
) -> bool {
    if mode == DisplayMode::MathIO && b.is_zero() {
        // 形如 a x^2 + c，且非负（Δ = -4aci ≥ 0）
        let lcm = a.denom().lcm(b.denom()).lcm(c.denom());
        let f = BigRational::from_integer(lcm);
        let ai = (a * &f).to_integer();
        let bi = (b * &f).to_integer();
        let ci = (c * &f).to_integer();
        let delta = &bi * &bi - BigInt::from(4) * &ai * &ci;
        if delta >= BigInt::zero() {
            let (s, d) = simplify_sqrt(&delta);
            if d == BigInt::one() {
                // 完全平方，拆两个有理根
                let two_a = &ai * BigInt::from(2);
                let r1 = BigRational::new(-bi.clone() + &s, two_a.clone());
                let r2 = BigRational::new(-bi.clone() - &s, two_a);
                let q1 = r1.denom().clone();
                let q2 = r2.denom().clone();
                *ratio *= &BigRational::new(&q1 * &q2, ai.clone());
                for r in [r1, r2] {
                    let p = r.numer().clone();
                    let qb = BigRational::from_integer(r.denom().clone());
                    let lin = poly_linear(vars, &[qb, BigRational::from_integer(-p)]);
                    push_factor_rep(factors, FactorRep::Poly(lin, 1));
                }
                return true;
            }
            let two_a = &ai * BigInt::from(2);
            let p = BigRational::new(-bi, two_a.clone());
            let q = BigRational::new(s, two_a);
            push_factor_rep(factors, FactorRep::SqrtPair { p, q, d, exp: 1 });
            return true;
        }
    }
    push_factor_rep(
        factors,
        FactorRep::Poly(
            poly_of(vec![
                (vec![2u32], a.clone()),
                (vec![1u32], b.clone()),
                (vec![0u32], c.clone()),
            ]),
            1,
        ),
    );
    false
}

/// 入列表（合并相同因子——包括 SqrtPair 与常规 Poly——的重数）
fn push_factor_rep(factors: &mut Vec<FactorRep>, rep: FactorRep) {
    match &rep {
        FactorRep::SqrtPair { p, q, d, exp } => {
            for f in factors.iter_mut() {
                if let FactorRep::SqrtPair { p: ep, q: eq, d: ed, exp: e2 } = f {
                    if *ep == *p && *eq == *q && *ed == *d {
                        *e2 += exp;
                        return;
                    }
                }
            }
        }
        FactorRep::Poly(p, exp) => {
            for f in factors.iter_mut() {
                if let FactorRep::Poly(fp, e2) = f {
                    if poly_equal(fp, p) {
                        *e2 += exp;
                        return;
                    }
                }
            }
        }
    }
    factors.push(rep);
}

/// 因式分解结果（一元 / 多元共用）
struct FactorOutcome {
    /// 因子列表（含常数因子）
    factors: Vec<FactorRep>,
    /// 存在该域内不可再分的剩余因式（如次数 ≥ 5 的多项式）
    has_irreducible: bool,
    /// 候选枚举被规模上限截断 ⇒ 结果可能不完整，必须提示用户
    truncated: bool,
}

/// 一元分解
fn factor_univariate(p: &Poly, vars: &[char], mode: DisplayMode) -> FactorOutcome {
    // 升幂系数（单次扫描填充，避免 O(deg × terms) 的重复线性查找）
    let deg = p.total_deg() as usize;
    let mut coeffs: Vec<BigRational> = vec![BigRational::zero(); deg + 1];
    for t in &p.terms {
        if t.mono.len() == vars.len() {
            coeffs[t.mono[0] as usize] += &t.coeff;
        }
    }
    if coeffs.is_empty() {
        coeffs.push(BigRational::zero());
    }

    // 系数统一乘公分母化为整数（保证有理根定理候选正确）
    let mut den_lcm = BigInt::one();
    for c in &coeffs {
        den_lcm = den_lcm.lcm(c.denom());
    }
    let inv_lcm = BigRational::new(BigInt::one(), den_lcm.clone());
    let lcm_rat = BigRational::from_integer(den_lcm);
    for c in coeffs.iter_mut() {
        *c = &*c * &lcm_rat;
    }

    // content（整数系数的分子 gcd，符号归一）
    let mut gcd = BigInt::zero();
    for c in &coeffs {
        let a = c.numer().abs();
        if gcd.is_zero() {
            gcd = a.clone();
        } else {
            gcd = gcd.gcd(&a);
        }
    }
    let content = if gcd.is_zero() { BigRational::one() } else { BigRational::from_integer(gcd.clone()) };
    if !content.is_one() {
        for c in coeffs.iter_mut() {
            *c = &*c / &content;
        }
    }

    let mut factors: Vec<FactorRep> = Vec::new();
    let mut had_irreducible = false;
    let mut truncated = false; // 有理根候选 / 二次因子候选被截断（提示结果可能不完整）
    let mut ratio = BigRational::one(); // 线性因子整数化引入的额外倍数（并入 content）
    let mut c0 = BigRational::one(); // 分解末尾剩余的常数（并入 content）

    // 提取因子 x（合并为单一重数幂，避免展开成多个 (x)）
    // 注意：不能逐个 `coeffs.remove(0)` —— 那对 deg=10^6 的多项式是 O(n²)
    // （约 10^12 次元素搬移，实测 fac(x^1000000) 会卡死），这里先数再整体 drain（O(n)）
    let mut x_count: u32 = 0;
    while (x_count as usize) + 1 < coeffs.len() && coeffs[x_count as usize].is_zero() {
        x_count += 1;
    }
    if x_count > 0 {
        coeffs.drain(0..x_count as usize);
    }
    if x_count > 0 {
        factors.push(FactorRep::Poly(
            poly_linear(vars, &[BigRational::one(), BigRational::zero()]),
            x_count,
        ));
    }

    // 有理根循环
    loop {
        if coeffs.len() <= 1 {
            break;
        }
        let (cands, cand_trunc) = rational_root_candidates(&coeffs);
        if cand_trunc {
            truncated = true;
        }
        let mut found = None;
        for r in cands {
            if poly_eval(&coeffs, &r).is_zero() {
                found = Some(r);
                break;
            }
        }
        let r = match found {
            Some(r) => r,
            None => break,
        };
        let (q, _rem) = synthetic_div(&coeffs, &r);
        coeffs = q;
        // 合并重数
        let lin = poly_linear(vars, &[BigRational::one(), -r.clone()]);
        let mut merged = false;
        for f in factors.iter_mut() {
            if let FactorRep::Poly(fp, exp) = f {
                if poly_equal(fp, &lin) {
                    *exp += 1;
                    merged = true;
                    break;
                }
            }
        }
        if !merged {
            factors.push(FactorRep::Poly(lin, 1));
        }
        // 记录整数化倍数
        let den = r.denom().clone();
        if !den.is_one() && den.is_positive() {
            ratio *= &BigRational::from_integer(den);
        }
    }

    // 剩余：先递归提取二次因子，再处理最终残余
    let rem_deg = coeffs.len().saturating_sub(1);
    if rem_deg >= 2 {
        let mut remain = coeffs;
        // 反复提取二次因子（商继续尝试）
        loop {
            if remain.len().saturating_sub(1) < 4 {
                break;
            }
            let (found, quad_trunc) = extract_quadratic_factor(&remain);
            if quad_trunc {
                truncated = true;
            }
            match found {
                Some((g, quo)) => {
                    push_quad_factor(&mut factors, &mut ratio, vars, &g[2], &g[1], &g[0], mode);
                    remain = quo;
                }
                None => break,
            }
        }
        let rdeg = remain.len().saturating_sub(1);
        if rdeg == 2 {
            // 二次因子：MathIO 平方差型可拆根式；LineIO 只保留整式。
            // 走到这里说明有理根枚举已经失败（否则早就降到一次），
            // 故 LineIO 下保留的二次因式在有理数域内**确定不可再分解**，必须提示
            //（/help 与 README 都承诺该提示；旧实现静默返回 `(x^2 - 2)`）。
            let split = push_quad_factor(
                &mut factors,
                &mut ratio,
                vars,
                &remain[2],
                &remain[1],
                &remain[0],
                mode,
            );
            if !split && mode == DisplayMode::LineIO {
                had_irreducible = true;
            }
        } else if rdeg >= 3 {
            let terms: Vec<(Mono, BigRational)> = remain
                .iter()
                .enumerate()
                .filter(|(_, c)| !c.is_zero())
                .map(|(i, c)| (vec![i as u32], c.clone()))
                .collect();
            factors.push(FactorRep::Poly(poly_of(terms), 1));
            // 次数 ≥ 5 时无法判定不可约，提示结果可能不完整；
            // 次数 3/4 且已尝试有理根 / 二次因子 ⇒ Q 上不可约，不提示。
            // （候选截断不再是"不可约"，而是"没算完"，由 truncated 单独提示）
            if rdeg >= 5 {
                had_irreducible = true;
            }
        } else if rdeg == 1 {
            let a = remain[1].clone();
            let b = remain[0].clone();
            factors.push(FactorRep::Poly(poly_linear(vars, &[a, b]), 1));
        } else if rdeg == 0 {
            c0 = remain[0].clone();
        }
    } else if rem_deg == 1 {
        let a = coeffs[1].clone();
        let b = coeffs[0].clone();
        factors.push(FactorRep::Poly(poly_linear(vars, &[a, b]), 1));
    } else if rem_deg == 0 {
        // 常数并入 content
        c0 = coeffs[0].clone();
    }

    // 组装最终 content：
    // 原式 = content * c0 * Pi(x - r_i)（整数化后求得）
    // 显示将 (x - p/q) 归一为 (q*x - p) 引入倍数 Pi(q) = ratio，
    // 因此最终常数因子 = content * c0 / ratio * inv_lcm
    let final_content = &content * &c0 / &ratio * &inv_lcm;
    let mut out = Vec::new();
    // 零也必须排除：poly_const(0) 经 poly_of 过滤后会得到空 Poly（terms 为空），
    // 后续格式化里 `is_constant` 判真而 `terms[0]` 越界。零多项式本身没有可显示的因子。
    if !final_content.is_one() && !final_content.is_zero() {
        out.push(FactorRep::Poly(poly_const(&final_content), 1));
    }
    out.extend(factors);
    FactorOutcome {
        factors: out,
        has_irreducible: had_irreducible,
        truncated,
    }
}

/// 是否齐次（所有项总次数相同）
fn is_homogeneous(p: &Poly) -> bool {
    let d = p.total_deg();
    p.terms.iter().all(|t| mono_total(&t.mono) == d)
}

/// 齐次二元多项式提升：一元因子 t^k → x^k * y^(d-k)
fn lift_univariate(poly: &Poly) -> Poly {
    let d = poly.total_deg() as usize;
    let items: Vec<(Mono, BigRational)> = poly
        .terms
        .iter()
        .map(|t| {
            let k = t.mono[0] as usize;
            (vec![k as u32, (d - k) as u32], t.coeff.clone())
        })
        .collect();
    poly_of(items)
}

/// 多元分解: 提取 content、公因子、线性因式；二元齐次时降维为一元分解
fn factor_multivariate(p: &Poly, vars: &[char], mode: DisplayMode) -> FactorOutcome {
    let (content, p1) = extract_content(p);
    let (common, p2) = extract_common_monomial(&p1, vars.len());

    let mut factors: Vec<FactorRep> = Vec::new();
    if !content.is_one() {
        factors.push(FactorRep::Poly(poly_const(&content), 1));
    }
    if mono_total(&common) > 0 {
        factors.push(FactorRep::Poly(poly_of(vec![(common.clone(), BigRational::one())]), 1));
    }
    let mut cur = p2;

    // 齐次二元 → 降维到一元分解（f(x,y) = y^d * F(x/y, 1)）
    if vars.len() == 2 && is_homogeneous(&cur) && !is_constant(&cur) {
        let d = cur.total_deg() as usize;
        if d >= 2 {
            let mut ucoe: Vec<BigRational> = vec![BigRational::zero(); d + 1];
            for t in &cur.terms {
                ucoe[t.mono[0] as usize] += &t.coeff;
            }
            let u_items: Vec<(Mono, BigRational)> = ucoe
                .iter()
                .enumerate()
                .filter(|(_, c)| !c.is_zero())
                .map(|(i, c)| (vec![i as u32], c.clone()))
                .collect();
            let u_poly = poly_of(u_items);
            let uni = factor_univariate(&u_poly, &['t'], mode);
            let mut all_liftable = true;
            for f in &uni.factors {
                match f {
                    FactorRep::Poly(fp, _) => {
                        if is_constant(fp) && !fp.terms[0].coeff.is_integer() {
                            // 常数因子为分数时无法保证 y^d 齐次提升后乘积不变，放弃齐次路径
                            all_liftable = false;
                            break;
                        }
                    }
                    FactorRep::SqrtPair { .. } => {
                        // 根式因子无法以二元显示，放弃齐次路径
                        all_liftable = false;
                        break;
                    }
                }
            }
            if all_liftable {
                for f in &uni.factors {
                    match f {
                        FactorRep::Poly(fp, exp) => {
                            if is_constant(fp) {
                                factors.push(FactorRep::Poly(poly_const(&fp.terms[0].coeff), *exp));
                            } else {
                                let lifted = lift_univariate(fp);
                                factors.push(FactorRep::Poly(lifted, *exp));
                            }
                        }
                        FactorRep::SqrtPair { .. } => {}
                    }
                }
                return FactorOutcome {
                    factors,
                    has_irreducible: uni.has_irreducible,
                    truncated: uni.truncated,
                };
            }
        }
    }

    loop {
        match find_linear_factor(&cur, vars) {
            Some((lin, pivot)) => {
                if let Some((quo, _)) = div_linear_by_pivot(&cur, &lin, pivot) {
                    let mut merged = false;
                    for f in factors.iter_mut() {
                        if let FactorRep::Poly(fp, exp) = f {
                            if poly_equal(fp, &lin) {
                                *exp += 1;
                                merged = true;
                                break;
                            }
                        }
                    }
                    if !merged {
                        factors.push(FactorRep::Poly(lin, 1));
                    }
                    cur = quo;
                } else {
                    break;
                }
            }
            None => break,
        }
    }

    let mut had_irreducible = false;
    if !cur.terms.is_empty() {
        if is_constant(&cur) {
            let c = cur.terms[0].coeff.clone();
            if !c.is_one() {
                factors.push(FactorRep::Poly(poly_const(&c), 1));
            }
        } else {
            // 二元的二次保留视为已完整处理（对齐一元规则）；更高次才提示
            factors.push(FactorRep::Poly(cur.clone(), 1));
            if cur.total_deg() >= 3 {
                had_irreducible = true;
            }
        }
    }
    FactorOutcome {
        factors,
        has_irreducible: had_irreducible,
        truncated: false,
    }
}

/// 小有理数系数候选集合
fn small_coeff_set() -> Vec<BigRational> {
    let mut s = Vec::new();
    for n in 0i64..=3 {
        s.push(BigRational::from_integer(BigInt::from(n)));
        if n > 0 {
            s.push(BigRational::from_integer(BigInt::from(-n)));
            for d in [2i64, 3i64] {
                s.push(BigRational::new(BigInt::from(n), BigInt::from(d)));
                s.push(BigRational::new(BigInt::from(-n), BigInt::from(d)));
            }
        }
    }
    s
}

/// 将 P 投影到单变量轴: P(v_axis, 0, ..., 0) 的升幂系数
fn projection_on_axis(p: &Poly, axis: usize) -> Vec<BigRational> {
    let deg = p.total_deg() as usize;
    let mut coeffs = vec![BigRational::zero(); deg + 1];
    for t in &p.terms {
        let mut on_axis = true;
        for (i, e) in t.mono.iter().enumerate() {
            if i != axis && *e != 0 {
                on_axis = false;
                break;
            }
        }
        if !on_axis {
            continue;
        }
        coeffs[t.mono[axis] as usize] = &coeffs[t.mono[axis] as usize] + &t.coeff;
    }
    while coeffs.len() > 1 && coeffs[coeffs.len() - 1].is_zero() {
        coeffs.pop();
    }
    coeffs
}

/// 多元线性因式发现：返回 (因式多项式, 主变量下标)
fn find_linear_factor(p: &Poly, vars: &[char]) -> Option<(Poly, usize)> {
    let n = vars.len();
    if n < 2 || p.terms.is_empty() {
        return None;
    }
    let mut candidates: Vec<(Poly, usize)> = Vec::new();

    // 各轴投影的有理根
    let mut axis_roots: Vec<Vec<BigRational>> = Vec::new();
    for i in 0..n {
        axis_roots.push(rational_roots(&projection_on_axis(p, i)));
    }

    // 1) 投影法: 主变量 i，常数 c = -r_i（r_i 是 i 轴根），其它变量系数由对应轴根推导
    for i in 0..n {
        for r in &axis_roots[i] {
            let c = -r;
            let mut others: Vec<Vec<BigRational>> = Vec::new();
            for j in 0..n {
                if j == i {
                    continue;
                }
                let mut cands = vec![BigRational::zero()];
                for rj in &axis_roots[j] {
                    if !rj.is_zero() {
                        cands.push(&c / -rj);
                    }
                }
                others.push(cands);
            }
            // 叉积
            let total: usize = others.iter().map(|o| o.len()).product();
            let mut idx = vec![0usize; others.len()];
            for _ in 0..total.min(40) {
                let mut coeffs = vec![BigRational::zero(); n + 1];
                coeffs[i] = BigRational::one();
                coeffs[n] = c.clone();
                let mut k = 0;
                for j in 0..n {
                    if j == i {
                        continue;
                    }
                    coeffs[j] = others[k][idx[k]].clone();
                    k += 1;
                }
                candidates.push((poly_linear(vars, &coeffs), i));
                // 进位
                let mut pos = others.len();
                while pos > 0 {
                    pos -= 1;
                    idx[pos] += 1;
                    if idx[pos] < others[pos].len() {
                        break;
                    }
                    idx[pos] = 0;
                }
                if pos == 0 && idx.iter().all(|&x| x == 0) {
                    break;
                }
            }
        }
    }

    // 2) 小集合枚举（覆盖齐次线性因子，如 x - y）
    let sc = small_coeff_set();
    let pick = sc.len();
    let mut code_limit = 1usize;
    for _ in 0..(n - 1).min(2) {
        code_limit = code_limit.saturating_mul(pick);
    }
    if code_limit > 1000 {
        code_limit = 1000;
    }
    for i in 0..n {
        for ci in [0usize, 1, 2] {
            // 常数项: 0, +1, -1
            let const_vals = [BigRational::zero(), BigRational::one(), BigRational::from_integer(BigInt::from(-1))];
            let cconst = const_vals[ci % 3].clone();
            for code in 0..code_limit {
                let mut other_idx = vec![0usize; n - 1];
                let mut tmp = code;
                for slot in 0..(n - 1).min(2) {
                    other_idx[slot] = tmp % pick;
                    tmp /= pick;
                }
                let mut coeffs = vec![BigRational::zero(); n + 1];
                coeffs[i] = BigRational::one();
                coeffs[n] = cconst.clone();
                let mut slot = 0;
                for j in 0..n {
                    if j == i {
                        continue;
                    }
                    coeffs[j] = sc[other_idx[slot]].clone();
                    slot += 1;
                }
                let lin = poly_linear(vars, &coeffs);
                candidates.push((lin, i));
            }
        }
    }

    // 验证（去重，且要求真正整除：乘回验证）
    let mut seen: Vec<Poly> = Vec::new();
    for (lin, pivot) in candidates {
        if lin.terms.is_empty() || seen.iter().any(|s| poly_equal(s, &lin)) {
            continue;
        }
        seen.push(lin.clone());
        if let Some((quo, _)) = div_linear_by_pivot(p, &lin, pivot) {
            if poly_equal(&poly_mul(&quo, &lin), p) {
                return Some((lin, pivot));
            }
        }
    }
    None
}

/* ---------------- AST 提取 ---------------- */

fn rational_of_number(n: &Number) -> Option<BigRational> {
    match n {
        Number::Exact(expr) => expr.as_rational().or_else(|| expr.as_integer().map(BigRational::from_integer)),
        Number::Approx(_) => None,
       Number::Complex(_) => None,
    }
}

fn expr_to_poly(expr: &Expr, vars: &[char], evaluator: &Evaluator) -> Option<Poly> {
    let mut terms: Vec<(Mono, Number)> = Vec::new();
    collect_expr_terms(expr, vars, evaluator, &mut terms)?;
    let mut items: Vec<(Mono, BigRational)> = Vec::new();
    for (m, c) in terms {
        items.push((m, rational_of_number(&c)?));
    }
    Some(poly_of(items))
}

fn collect_expr_terms(
    expr: &Expr,
    vars: &[char],
    evaluator: &Evaluator,
    terms: &mut Vec<(Mono, Number)>,
) -> Option<()> {
    match expr {
        Expr::Number(n) => {
            terms.push((vec![0u32; vars.len()], n.clone()));
            Some(())
        }
        Expr::Variable(name) => {
            // ans 与已存储的全大写变量（/let）按**当前数值**代入常数项，
            // 与方程求解的"过滤已存储变量"规则保持一致。
            // 旧实现把 ans 当成 0、把存储变量当成未知量，结果容易误读。
            if name == "ans" {
                terms.push((vec![0u32; vars.len()], evaluator.ans.clone()));
            } else if let Some(val) = evaluator.vars.get(name) {
                terms.push((vec![0u32; vars.len()], val.clone()));
            } else if name.len() == 1 {
                let ch = name.chars().next().unwrap();
                if let Some(pos) = vars.iter().position(|&v| v == ch) {
                    let mut mono = vec![0u32; vars.len()];
                    mono[pos] = 1;
                    terms.push((mono, Number::from_int(1)));
                } else {
                    return None;
                }
            } else {
                return None;
            }
            Some(())
        }
        Expr::Binary(left, op, right) => {
            let mut lt = Vec::new();
            let mut rt = Vec::new();
            collect_expr_terms(left, vars, evaluator, &mut lt)?;
            collect_expr_terms(right, vars, evaluator, &mut rt)?;
            match op {
                crate::parser::BinOp::Add => {
                    terms.extend(lt);
                    terms.extend(rt);
                }
                crate::parser::BinOp::Sub => {
                    terms.extend(lt);
                    for (m, c) in rt {
                        terms.push((m, c.neg()));
                    }
                }
                crate::parser::BinOp::Mul => {
                    for (m1, c1) in &lt {
                        for (m2, c2) in &rt {
                            let mono: Mono = m1.iter().zip(m2.iter()).map(|(a, b)| a + b).collect();
                            terms.push((mono, c1.mul(c2)));
                        }
                    }
                }
                crate::parser::BinOp::Div => {
                    // 支持"表达式 / 非零常数"（系数除以常数）；分母含变量则非多项式
                    if rt.iter().all(|(m, _)| m.iter().all(|&e| e == 0)) {
                        let mut den = BigRational::zero();
                        for (_, c) in rt {
                            den += &rational_of_number(&c)?;
                        }
                        if den.is_zero() {
                            return None;
                        }
                        let den_num = Number::from_rational(den);
                        for (m, c) in lt {
                            terms.push((m, c.div(&den_num)));
                        }
                    } else {
                        return None;
                    }
                }
            }
            Some(())
        }
        Expr::Unary(op, e) => {
            let mut inner = Vec::new();
            collect_expr_terms(e, vars, evaluator, &mut inner)?;
            match op {
                crate::parser::UnaryOp::Pos => terms.extend(inner),
                crate::parser::UnaryOp::Neg => {
                    for (m, c) in inner {
                        terms.push((m, c.neg()));
                    }
                }
            }
            Some(())
        }
        Expr::Pow(base, exp) => {
            if let Expr::Variable(name) = base.as_ref() {
                if name.len() == 1 {
                    let ch = name.chars().next().unwrap();
                    if let Some(pos) = vars.iter().position(|&v| v == ch) {
                        if let Expr::Number(n) = exp.as_ref() {
                            if let Some(r) = n.as_rational() {
                                if r.is_integer() && r.is_positive() {
                                    if let Some(e) = r.to_integer().to_u32() {
                                        let mut mono = vec![0u32; vars.len()];
                                        mono[pos] = e;
                                        terms.push((mono, Number::from_int(1)));
                                        return Some(());
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // 一般多项式底的小整数幂：折叠自乘（每轮合并同类项，避免 AST 指数爆炸）
            if let Expr::Number(n) = exp.as_ref() {
                if let Some(r) = n.as_rational() {
                    if r.is_integer() {
                        let k = r.to_integer();
                        if k.is_zero() {
                            terms.push((vec![0u32; vars.len()], Number::from_int(1)));
                            return Some(());
                        }
                        if let Some(k) = k.to_u32() {
                            if (1..=64).contains(&k) {
                                let mut base_terms = Vec::new();
                                collect_expr_terms(base, vars, evaluator, &mut base_terms)?;
                                let mut acc = base_terms.clone();
                                for _ in 1..k {
                                    let mut new_acc: Vec<(Mono, Number)> = Vec::new();
                                    for (m1, c1) in &acc {
                                        for (m2, c2) in &base_terms {
                                            let mono: Mono = m1
                                                .iter()
                                                .zip(m2.iter())
                                                .map(|(a, b)| a + b)
                                                .collect();
                                            new_acc.push((mono, c1.mul(c2)));
                                        }
                                    }
                                    acc = merge_raw_terms(new_acc);
                                }
                                terms.extend(acc);
                                return Some(());
                            }
                        }
                    }
                }
            }
            None
        }
        _ => None,
    }
}

/// 合并同类单项式（BTreeMap 归并系数）
fn merge_raw_terms(items: Vec<(Mono, Number)>) -> Vec<(Mono, Number)> {
    let mut map: std::collections::BTreeMap<Mono, Number> = std::collections::BTreeMap::new();
    for (m, c) in items {
        if c.is_zero() {
            continue;
        }
        map.entry(m)
            .and_modify(|x| *x = x.add(&c))
            .or_insert(c);
    }
    map.into_iter().collect()
}

/* ---------------- 公共接口 ---------------- */

/// Q 域完整分解：供方程求解复用。
/// 输入升幂系数（有理数），返回 (不可约因子升幂系数, 重数) 列表，
/// 以及是否含 deg>=3 仍无法精确分解的剩余（需数值求根）。
pub fn factor_univariate_coeffs(
    coeffs: &[BigRational],
) -> (Vec<(Vec<BigRational>, u32)>, bool) {
    let items: Vec<(Mono, BigRational)> = coeffs
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.is_zero())
        .map(|(i, c)| (vec![i as u32], c.clone()))
        .collect();
    let poly = poly_of(items);
    let outcome = factor_univariate(&poly, &['x'], DisplayMode::LineIO);
    let mut out = Vec::new();
    for f in &outcome.factors {
        if let FactorRep::Poly(fp, exp) = f {
            if is_constant(fp) {
                continue; // 常数因子不影响根
            }
            let deg = fp.total_deg() as usize;
            let mut fc: Vec<BigRational> = vec![BigRational::zero(); deg + 1];
            for t in &fp.terms {
                fc[t.mono[0] as usize] = t.coeff.clone();
            }
            out.push((fc, *exp));
        }
    }
    (out, outcome.has_irreducible)
}

/// 对表达式做因式分解并返回格式化字符串。
/// 需要 `Evaluator` 以代入 `ans` 与 `/let` 存储变量（与方程求解保持一致的取值规则）。
pub fn factor_expr(
    evaluator: &Evaluator,
    expr: &Expr,
    mode: DisplayMode,
) -> Result<String, String> {
    let info = crate::equation::EquationInfo::extract(expr);
    // 已存储的全大写变量视为已知常数，不作为分解变量
    let vars: Vec<char> = info
        .variables
        .into_iter()
        .filter(|v| !evaluator.vars.contains_key(&v.to_string()))
        .collect();
    if vars.len() > 3 {
        return Err("factor 目前仅支持 1~3 个变量".to_string());
    }

    let poly = expr_to_poly(expr, &vars, evaluator)
        .ok_or_else(|| "无法因式分解：仅支持有理数系数的多项式".to_string())?;

    if poly.terms.is_empty() {
        return Ok("0".to_string());
    }
    if vars.is_empty() {
        return Ok(format_rat(&poly.terms[0].coeff, mode));
    }

    let outcome = if vars.len() == 1 {
        factor_univariate(&poly, &vars, mode)
    } else {
        factor_multivariate(&poly, &vars, mode)
    };
    let factors = outcome.factors;
    let had_irreducible = outcome.has_irreducible;
    let truncated = outcome.truncated;

    let mut parts: Vec<String> = Vec::new();
    for f in &factors {
        let fs = match f {
            FactorRep::Poly(p, exp) => {
                // 空多项式（零多项式）直接显示 0：此时 is_constant 为真但 terms 为空，
                // 取 p.terms[0] 会越界 panic（防御性检查，正常路径不会出现）
                if p.terms.is_empty() {
                    parts.push("0".to_string());
                    continue;
                }
                let body = if is_constant(p) {
                    format_rat(&p.terms[0].coeff, mode)
                } else if is_linear(p) && p.terms.len() <= vars.len() + 1 {
                    format_linear_factor(p, &vars)
                } else if p.terms.len() == 1 {
                    format_mono(&p.terms[0].mono, &vars)
                } else {
                    format_poly(p, &vars, mode)
                };
                if is_constant(p) {
                    body // 常数因子不加括号
                } else if *exp > 1 {
                    format!("({})^{}", body, exp)
                } else {
                    format!("({})", body)
                }
            }
            FactorRep::SqrtPair { p, q, d, exp } => {
                let body = format_sqrt_pair(p, q, d, mode);
                if *exp > 1 {
                    format!("({})^{}", body, exp)
                } else {
                    body
                }
            }
        };
        parts.push(fs);
    }

    let mut out = parts.join(" * ");
    if out.is_empty() {
        out = "1".to_string();
    }

    if truncated {
        // 候选/枚举被规模上限截断：结果**可能不完整**，必须明确提示
        out.push(' ');
        out.push_str("（候选枚举超出规模上限，结果可能不完整）");
    } else if had_irreducible {
        out.push(' ');
        out.push_str(match mode {
            DisplayMode::MathIO => "（剩余高次因式未作进一步精确分解）",
            DisplayMode::LineIO => "（有理数域内不可再分解）",
        });
    }
    Ok(out)
}