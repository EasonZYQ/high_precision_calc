//! 整数的素因数分解（供 `primefac(...)` 与 `fac`/`factor` 的整数入参使用）。
//!
//! # 算法
//!
//! ```text
//! prime_factorize(n)
//!   ├─ n 必须为正（调用方先取绝对值）
//!   ├─ 小素数表试除（1000 以内，168 个）        ← 覆盖绝大多数输入
//!   ├─ 6k±1 试除，直到 d > TRIAL_MAX 或 d² > m  ← 覆盖中等素因子；6k±1 省掉 1/3 无效试除
//!   ├─ is_prime(m)？是素数就收下、直接结束      ← 关键：**先判素**，素数不必试除到 √m
//!   └─ 仍是合数 ⇒ Brent 版 Pollard's rho 找因子，递归分解两半
//! ```
//!
//! # 两个模式的区别
//!
//! **不再是"试除上限"**（试除上限两个模式一致取 10^6——超过它以后 Pollard 更快，把 √m
//! 全部试除一遍纯属浪费；这正是"同一个数 Deep 比 Fast 慢"的旧 bug 根因）。
//! 现在 `/mode deep` 放大的是 **Pollard 的迭代预算**，这更贴合"死算"的语义。
//!
//! # 复杂度
//!
//! Pollard's rho 期望 O(n^(1/4))：10^12 的半素数期望约 10^3 次迭代、10^18 约 3×10^4 次，
//! 因此 64 位以内的合数在 Fast 预算（2^17）内基本都能分解；`/mode deep` 的 2^24 预算
//! 可以再往上够到 10^24~10^30 量级。

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};

/// 6k±1 试除的上限。与 `solver_factor::int_divisors` **采用同一套"预算 + 明确报错"策略**
/// （那边的上限是它自己的 10^13；这里单独取 10^6，因为试除代价随候选数线性增长，
/// 而超过 10^6 之后交给 Pollard's rho 更划算）。
const TRIAL_MAX: u64 = 1_000_000;

/// Fast 模式 Pollard 的迭代预算（10^18 的半素数期望约 3×10^4 次，留 4 倍余量）
const POLLARD_ITERS_FAST: u64 = 1 << 17;
/// Deep 模式放大到 2^24（够到 10^24 量级的半素数）
const POLLARD_ITERS_DEEP: u64 = 1 << 24;

/// 预算用尽（Pollard 也没能拆开）时的统一报错文案
const INCOMPLETE_MSG: &str =
    "素因数分解超出预算（Fast 模式 Pollard 迭代上限 2^17）；如确认需要继续，请先执行 /mode deep";

/// 1000 以内的素数表（惰性筛一次）。手写 168 个常量容易出错，这里按需生成。
fn small_primes() -> &'static [u64] {
    static CACHE: std::sync::OnceLock<Vec<u64>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| {
        const N: usize = 1000;
        let mut is_prime = [true; N];
        let mut out = Vec::new();
        for i in 2..N {
            if is_prime[i] {
                out.push(i as u64);
                let mut j = i * i;
                while j < N {
                    is_prime[j] = false;
                    j += i;
                }
            }
        }
        out
    })
}

/// 素因数分解：返回按素数**升序**排列的 `(素数, 指数)` 列表。
///
/// `n` 必须为**正整数**——`0` 与负数在这里是调用错误，返回 `Err` 而不是 panic
/// （用户侧的 `primefac(0)` 由 `parser` 在更早的地方拦下并给出更贴切的文案）。
/// `1` 没有素因数 ⇒ 返回空列表。
pub fn prime_factorize(n: &BigInt) -> Result<Vec<(BigInt, u32)>, String> {
    if !n.is_positive() {
        return Err("prime_factorize 只接受正整数（调用方应先取绝对值）".to_string());
    }
    // 先收集"拍平"的素因子列表，最后统一排序合并——Pollard 找到的因子顺序不单调，
    // 不能依赖"相同因子必然紧邻"来当场合并指数。
    let mut flat: Vec<BigInt> = Vec::new();
    if let Some(m) = n.to_u64() {
        factor_u64(m, &mut flat)?;
    } else {
        factor_bigint(n, &mut flat)?;
    }
    flat.sort();
    let mut out: Vec<(BigInt, u32)> = Vec::new();
    for f in flat {
        match out.last_mut() {
            Some(last) if last.0 == f => last.1 += 1,
            _ => out.push((f, 1)),
        }
    }
    Ok(out)
}

/// 把分解结果格式化成等式的形式：`12 = 2^2 * 3`、`-12 = -2^2 * 3`、`7 = 7`、`1 = 1`。
///
/// - 指数为 1 时省略 `^1`；
/// - 负号按约定顶在**最前面**（`-12 = -2^2 * 3`）；
/// - `0` 没有素因数分解 ⇒ 明确的定义域错误（不是"超出预算"）。
pub fn format_prime_factorization(n: &BigInt) -> Result<String, String> {
    if n.is_zero() {
        return Err("0 没有素因数分解".to_string());
    }
    let neg = n.is_negative();
    let factors = prime_factorize(&n.abs())?;
    let body = if factors.is_empty() {
        // |n| == 1：没有素因数，等式两边就是它自己
        "1".to_string()
    } else {
        factors
            .iter()
            .map(|(p, e)| {
                if *e == 1 {
                    p.to_string()
                } else {
                    format!("{}^{}", p, e)
                }
            })
            .collect::<Vec<_>>()
            .join(" * ")
    };
    let rhs = if neg { format!("-{}", body) } else { body };
    Ok(format!("{} = {}", n, rhs))
}

/* ---------------- u64 路径（小整数，debug 下比 BigInt 快两个数量级） ---------------- */

/// 分解一个 `u64`（内部递归调用自身，因子拍平进 `out`）
fn factor_u64(mut m: u64, out: &mut Vec<BigInt>) -> Result<(), String> {
    // 1) 小素数表
    for &p in small_primes() {
        while m.is_multiple_of(p) {
            out.push(BigInt::from(p));
            m /= p;
        }
        if m == 1 {
            return Ok(());
        }
    }
    // 2) 6k±1 试除（跳过全部 2/3 的倍数；1000 以内的素数已在表里处理过，从 1001 起步）
    let mut d = 1001u64;
    let mut step = 2u64;
    loop {
        if m == 1 {
            return Ok(());
        }
        // d 超过 √m ⇒ 剩下的 m 必为素数；d 超过预算则交给 Pollard
        if d > TRIAL_MAX || d.checked_mul(d).is_none_or(|dd| dd > m) {
            break;
        }
        while m.is_multiple_of(d) {
            out.push(BigInt::from(d));
            m /= d;
        }
        d += step;
        step = 6 - step; // 2 → 4 → 2 → 4 …
    }
    if m == 1 {
        return Ok(());
    }
    // 3) **先判素**：否则 Deep 模式会把一个素数的 √m 全部试除一遍（旧实现 4 秒的根因）
    if is_prime_u64(m) {
        out.push(BigInt::from(m));
        return Ok(());
    }
    // 4) 合数且无小因子 ⇒ Pollard's rho
    split_u64(m, out)
}

/// 已经把小因子剥离干净的合数：用 Pollard's rho 拆成两半递归处理
fn split_u64(m: u64, out: &mut Vec<BigInt>) -> Result<(), String> {
    if m == 1 {
        return Ok(());
    }
    if is_prime_u64(m) {
        out.push(BigInt::from(m));
        return Ok(());
    }
    let budget = if hipercalc_core::calc_mode::is_deep() {
        POLLARD_ITERS_DEEP
    } else {
        POLLARD_ITERS_FAST
    };
    match pollard_u64(m, budget) {
        Some(d) => {
            split_u64(d, out)?;
            split_u64(m / d, out)
        }
        None => Err(INCOMPLETE_MSG.to_string()),
    }
}

/// `a * b mod m`（u64 会溢出，走 u128）
fn mul_mod(a: u64, b: u64, m: u64) -> u64 {
    ((a as u128 * b as u128) % m as u128) as u64
}

/// `base^exp mod m`
fn pow_mod(mut base: u64, mut exp: u64, m: u64) -> u64 {
    if m == 1 {
        return 0;
    }
    let mut r = 1u64;
    base %= m;
    while exp > 0 {
        if exp & 1 == 1 {
            r = mul_mod(r, base, m);
        }
        base = mul_mod(base, base, m);
        exp >>= 1;
    }
    r
}

fn gcd_u64(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

/// 确定性 Miller-Rabin：这 12 个基对 `n < 3.3×10^24` 是确定的，覆盖整个 u64 范围；
/// 因此本函数对任何 u64 输入都给**确定性**结论（不是概率性判定）。
fn is_prime_u64(n: u64) -> bool {
    if n < 2 {
        return false;
    }
    for &p in small_primes() {
        if n.is_multiple_of(p) {
            return n == p;
        }
    }
    let mut d = n - 1;
    let mut r = 0u32;
    while d.is_multiple_of(2) {
        d /= 2;
        r += 1;
    }
    'bases: for &a in &[2u64, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37] {
        let mut x = pow_mod(a, d, n);
        if x == 1 || x == n - 1 {
            continue;
        }
        for _ in 0..r - 1 {
            x = mul_mod(x, x, n);
            if x == n - 1 {
                continue 'bases;
            }
        }
        return false;
    }
    true
}

/// Brent 版 Pollard's rho：返回 `n` 的一个非平凡因子（`1 < d < n`），
/// 预算耗尽或退化则返回 `None`（调用方换 `c` 重试由本函数内部负责）。
///
/// `n` 必须是**合数**且已排除小素因子。
fn pollard_u64(n: u64, budget: u64) -> Option<u64> {
    if n.is_multiple_of(2) {
        return Some(2);
    }
    let mut used = 0u64;
    let mut c = 1u64;
    'retry: loop {
        // f(x) = x² + c (mod n)
        let f = |x: u64| (mul_mod(x, x, n) + c) % n;
        let (mut y, mut r, mut q, mut g) = (2u64, 1u64, 1u64, 1u64);
        let mut x = 2u64;
        let mut ys = 2u64;
        const BLOCK: u64 = 128;
        while g == 1 {
            x = y;
            for _ in 0..r {
                y = f(y);
            }
            used += r;
            let mut k = 0u64;
            while k < r && g == 1 {
                ys = y;
                let lim = BLOCK.min(r - k);
                for _ in 0..lim {
                    y = f(y);
                    q = mul_mod(q, x.abs_diff(y), n);
                }
                used += lim;
                g = gcd_u64(q, n);
                k += BLOCK;
                if used > budget {
                    return None;
                }
            }
            r *= 2;
        }
        if g == n {
            // 退化（q 撞成 0）：从 ys 起逐项 gcd 找回
            loop {
                ys = f(ys);
                g = gcd_u64(x.abs_diff(ys), n);
                if g > 1 {
                    break;
                }
                used += 1;
                if used > budget {
                    return None;
                }
            }
        }
        if g != 1 && g != n {
            return Some(g);
        }
        // g == n：这个 c 值失败，换一个重试
        c += 1;
        if used > budget {
            return None;
        }
        if c > 64 {
            // 兜底：连续多个 c 都失败（几乎只可能发生在 n 是素数时）
            if is_prime_u64(n) {
                return None;
            }
        }
        continue 'retry;
    }
}

/* ---------------- BigInt 路径（超出 u64 的大数） ---------------- */

fn factor_bigint(n: &BigInt, out: &mut Vec<BigInt>) -> Result<(), String> {
    let mut m = n.clone();
    let two = BigInt::from(2u32);
    let three = BigInt::from(3u32);
    // 1) 小素数表
    for &p in small_primes() {
        let pb = BigInt::from(p);
        while (&m % &pb).is_zero() {
            out.push(pb.clone());
            m /= &pb;
        }
        if m.is_one() {
            return Ok(());
        }
    }
    // 2) 6k±1 试除（`is_even` 只读最低位 limb，比 `% 2` 少一次分配）
    while m.is_even() {
        out.push(two.clone());
        m /= &two;
    }
    while (&m % &three).is_zero() {
        out.push(three.clone());
        m /= &three;
    }
    let mut d = BigInt::from(1001u32);
    let mut step = BigInt::from(2u32);
    let six = BigInt::from(6u32);
    let trial_max = BigInt::from(TRIAL_MAX);
    loop {
        if m.is_one() {
            return Ok(());
        }
        if d > trial_max || &d * &d > m {
            break;
        }
        while (&m % &d).is_zero() {
            out.push(d.clone());
            m /= &d;
        }
        d += &step;
        step = &six - &step;
    }
    if m.is_one() {
        return Ok(());
    }
    // 3) 先判素（同 u64 路径：素数不该被一路试除掉）
    if hipercalc_core::bigint_ext::is_prime(&m) {
        out.push(m);
        return Ok(());
    }
    // 4) Pollard's rho
    split_bigint(m, out)
}

fn split_bigint(m: BigInt, out: &mut Vec<BigInt>) -> Result<(), String> {
    if m.is_one() {
        return Ok(());
    }
    if hipercalc_core::bigint_ext::is_prime(&m) {
        out.push(m);
        return Ok(());
    }
    let budget = if hipercalc_core::calc_mode::is_deep() {
        POLLARD_ITERS_DEEP
    } else {
        POLLARD_ITERS_FAST
    };
    match pollard_bigint(&m, budget) {
        Some(d) => {
            let other = &m / &d;
            split_bigint(d, out)?;
            split_bigint(other, out)
        }
        None => Err(INCOMPLETE_MSG.to_string()),
    }
}

/// Brent 版 Pollard's rho（BigInt 版，结构与 `pollard_u64` 一一对应）
fn pollard_bigint(n: &BigInt, budget: u64) -> Option<BigInt> {
    if n.is_even() {
        return Some(BigInt::from(2u32));
    }
    let one = BigInt::one();
    let mut used = 0u64;
    let mut c = BigInt::one();
    'retry: loop {
        let f = |x: &BigInt| (x * x + &c) % n;
        let mut y = BigInt::from(2u32);
        let mut r = 1u64;
        let mut q = BigInt::one();
        let mut g = BigInt::one();
        let mut x = y.clone();
        let mut ys = y.clone();
        const BLOCK: u64 = 128;
        while g.is_one() {
            x = y.clone();
            for _ in 0..r {
                y = f(&y);
            }
            used += r;
            let mut k = 0u64;
            while k < r && g.is_one() {
                ys = y.clone();
                let lim = BLOCK.min(r - k);
                for _ in 0..lim {
                    y = f(&y);
                    q = (&q * (&x - &y).abs()) % n;
                }
                used += lim;
                g = q.gcd(n);
                k += BLOCK;
                if used > budget {
                    return None;
                }
            }
            r *= 2;
        }
        if g == *n {
            loop {
                ys = f(&ys);
                g = (&x - &ys).abs().gcd(n);
                if g > one {
                    break;
                }
                used += 1;
                if used > budget {
                    return None;
                }
            }
        }
        if g > one && g != *n {
            return Some(g);
        }
        c += 1u32;
        if used > budget {
            return None;
        }
        continue 'retry;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(n: i64) -> String {
        format_prime_factorization(&BigInt::from(n)).unwrap()
    }

    fn fac_i64(n: i64) -> Vec<(i64, u32)> {
        prime_factorize(&BigInt::from(n))
            .unwrap()
            .iter()
            .map(|(p, e)| (p.to_i64().unwrap(), *e))
            .collect()
    }

    #[test]
    fn formats_small_and_negative() {
        assert_eq!(fmt(12), "12 = 2^2 * 3");
        assert_eq!(fmt(-12), "-12 = -2^2 * 3");
        assert_eq!(fmt(7), "7 = 7");
        assert_eq!(fmt(-7), "-7 = -7");
        assert_eq!(fmt(2), "2 = 2");
        assert_eq!(fmt(4), "4 = 2^2");
        assert_eq!(fmt(-4), "-4 = -2^2");
        assert_eq!(fmt(8), "8 = 2^3");
        assert_eq!(fmt(1), "1 = 1");
        assert_eq!(fmt(-1), "-1 = -1");
        assert_eq!(fmt(360), "360 = 2^3 * 3^2 * 5");
        assert_eq!(fmt(97), "97 = 97");
        assert_eq!(fmt(100), "100 = 2^2 * 5^2");
        // 试除预算边界附近：1009 是 > 1000 的第一个素数（走 6k±1 轮）
        assert_eq!(fmt(1009), "1009 = 1009");
        assert_eq!(fmt(1013), "1013 = 1013");
        assert_eq!(fmt(10201), "10201 = 101^2");
    }

    #[test]
    fn factor_list_is_sorted_and_collapsed() {
        assert_eq!(fac_i64(360), vec![(2, 3), (3, 2), (5, 1)]);
        assert_eq!(fac_i64(1024), vec![(2, 10)]);
        assert!(prime_factorize(&BigInt::from(1)).unwrap().is_empty());
    }

    #[test]
    fn zero_and_negative_are_rejected_as_domain_errors() {
        // 0 与负数都是**定义域错误**，既不能借用"超预算"文案，也不能 panic。
        // 旧实现用 debug_assert + is_zero 特判：release 下负数会落到 BigInt 路径，
        // 而 `d² > m` 对负数恒真 ⇒ 直接把整个负数当成素数输出。
        let e0 = prime_factorize(&BigInt::from(0)).unwrap_err();
        assert!(e0.contains("正整数"), "{e0}");
        let en = prime_factorize(&BigInt::from(-12)).unwrap_err();
        assert!(en.contains("正整数"), "{en}");
        // 格式化入口对 0 给的是定义域文案，而不是"超出预算"
        let ef = format_prime_factorization(&BigInt::from(0)).unwrap_err();
        assert_eq!(ef, "0 没有素因数分解");
    }

    #[test]
    fn large_prime_is_fast_and_exact() {
        // 用户实测反馈的那个数：它是素数。修复点是"先判素"——
        // 否则 Deep 模式会为了它把 √n ≈ 1.17×10^9 全部试除一遍（旧实现 4 秒）。
        let n: i64 = 1_345_676_543_465_434_567;
        assert_eq!(fmt(n), format!("{} = {}", n, n));
        assert_eq!(fmt(999_999_999_989), "999999999989 = 999999999989");
    }

    #[test]
    fn hard_semiprimes_are_now_factorable() {
        // 旧实现在这里直接报"超出试除预算"；改用 Pollard's rho 后应当分解出来。
        // 1000003 × 1000033 ≈ 10^12
        let n = 1_000_003i64 * 1_000_033;
        let got = fac_i64(n);
        assert!(
            got.iter().map(|(p, _)| *p).collect::<Vec<_>>() == vec![1_000_003, 1_000_033],
            "{got:?}"
        );
        // 两个大素数之积（10^17 量级）
        let a = 999_999_937i64; // 素数
        let b = 1_000_000_007i64; // 素数
        let n = a * b;
        let got = fac_i64(n);
        assert_eq!(
            got.iter().map(|(p, _)| *p).collect::<Vec<_>>(),
            vec![a, b],
            "{got:?}"
        );
    }

    #[test]
    fn u64_miller_rabin_is_deterministic() {
        // 对 u64 范围，12 个基是确定性判定：既不能把合数放过去，也不能把素数判成合数
        for n in [
            2u64,
            3,
            4,
            5,
            97,
            561,
            1105,
            1729,
            2465,
            2821,
            6601,
            999_999_937,
        ] {
            let want = !matches!(n, 4 | 561 | 1105 | 1729 | 2465 | 2821 | 6601);
            assert_eq!(is_prime_u64(n), want, "is_prime_u64({n})");
        }
        assert!(is_prime_u64(1_345_676_543_465_434_567));
        assert!(!is_prime_u64(1_000_003 * 1_000_033));
    }

    #[test]
    fn bigint_path_handles_beyond_u64() {
        // 2^70 只有小素因子，很快
        let n = BigInt::from(2u32).pow(70);
        assert_eq!(
            format_prime_factorization(&n).unwrap(),
            format!("{} = 2^70", n)
        );
        // 超出 u64 的合数（含大素因子）走 BigInt 路径的 Pollard
        let p = BigInt::from(2u32).pow(40) + 15; // 合数
        let f = prime_factorize(&p).unwrap();
        let rebuilt: BigInt = f.iter().map(|(b, e)| b.pow(*e)).product();
        assert_eq!(rebuilt, p);
    }

    #[test]
    fn i64_min_is_handled() {
        // i64::MIN 的绝对值是 2^63，超出 i64 但正好落在 u64 内
        let n = BigInt::from(i64::MIN);
        assert_eq!(
            format_prime_factorization(&n).unwrap(),
            format!("{} = -2^63", n)
        );
    }
}
