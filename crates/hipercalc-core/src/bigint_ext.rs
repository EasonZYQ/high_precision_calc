//! 大整数补充运算：绕开 num-bigint 0.4.7 的 Burnikel-Ziegler 除法断言缺陷。
//!
//! 背景（实测复现）：num-bigint 0.4.7 在"被除数约为除数平方"的形状下，
//! `biguint/division.rs:259` 的 `debug_assert!(ah < b)` 会失败并 panic 退出：
//!   - `sqr(10^100000)`、`sqr(10^100000+1)`；
//!   - `(10^200000+pi)/(10^100000+pi)`、`(10^100000+pi)/(10^50000+pi)`。
//! 该断言属于 debug 断言（dev profile 下 `debug-assertions` 默认开启，第三方依赖也不例外），
//! 触发后进程直接 panic；若断言被关掉，则会继续跑在有问题的分支上，风险更大。
//!
//! BZ 路径的启用条件是"被除数 > 128 个 64 位 limb 且除数 > 64 个 limb"
//! （`BURNIKEL_ZIEGLER_THRESHOLD = 64`，见 num-bigint 源码）。因此这里按规模分派：
//! - **小规模**（绝大多数运算）：仍走 num-bigint 的原生除法，速度不受影响；
//! - **大规模**：改用自带的 Knuth Algorithm D 教材式长除法（O(n·m)，比 BZ 慢但结果稳定）。
//!
//! 整数平方根同理：`BigInt::sqrt` 内部是牛顿迭代（每一步都要除以约 √n 的数，
//! 正好落在上面的坏形状里），故小值用内置实现，大值用自带的牛顿迭代 + 自带除法。

use num_bigint::{BigInt, BigUint, Sign};
use num_integer::Integer;
use num_traits::{One, Signed, Zero};

/// 触发 num-bigint BZ 除法的被除数 limb 数阈值（u64 limb）
const BZ_DIVIDEND_LIMBS: usize = 128;
/// 触发 num-bigint BZ 除法的除数 limb 数阈值
const BZ_DIVISOR_LIMBS: usize = 64;
/// 自带整数平方根启用的比特数阈值。
/// `sqrt(n)` 的牛顿迭代会除以约 √n 的数：n ≤ 4096 bit 时该除数 ≤ 2048 bit（32 个 limb），
/// 远低于 BZ 的 64 limb 门槛，可以安全使用内置 `BigInt::sqrt`。
const SELF_SQRT_BITS: u64 = 4_096;

/// 二进制 limb 数（等于 num-bigint 内部 `data.len()`）
fn limbs(n: &BigInt) -> usize {
    (n.bits() as usize).div_ceil(64)
}

/// 判断是否必须绕开 num-bigint 的 BZ 除法路径
fn needs_custom_div(a: &BigInt, b: &BigInt) -> bool {
    limbs(a) > BZ_DIVIDEND_LIMBS && limbs(b) > BZ_DIVISOR_LIMBS
}

/// 整数除法（向零取整）：大规模自动绕开 num-bigint 的 BZ 路径
pub fn div(a: &BigInt, b: &BigInt) -> BigInt {
    debug_assert!(!b.is_zero(), "整数除法：除数为零");
    if !b.is_zero() && needs_custom_div(a, b) {
        div_rem_schoolbook(a, b).0
    } else {
        a / b
    }
}

/// 整数平方根 floor(√n)（n ≤ 0 返回 0）
pub fn int_sqrt(n: &BigInt) -> BigInt {
    if !n.is_positive() {
        return BigInt::zero();
    }
    if n.bits() <= SELF_SQRT_BITS {
        // 小值：内置实现（不经过 BZ，快速且经过充分验证）
        return n.sqrt();
    }
    // 大值：自带牛顿迭代
    // 初值取 2^ceil(bits/2) ≥ √n ⇒ 迭代序列单调递减，收敛到 floor(√n)
    let mut x = BigInt::one() << (n.bits() + 1).div_ceil(2);
    loop {
        let q = div(n, &x);
        if q >= x {
            break;
        }
        x = (&x + q) >> 1;
    }
    // 收尾校验（正常路径无需修正，仅作保险）
    while &x * &x > *n {
        x -= BigInt::one();
    }
    while (&x + BigInt::one()) * (&x + BigInt::one()) <= *n {
        x += BigInt::one();
    }
    x
}

/// 整数 k 次根 floor(n^(1/k))（n ≥ 0、k ≥ 1）。
/// 牛顿迭代 `y ← ((k-1)y + n/y^(k-1))/k`，初值 `2^ceil(bits/k) ≥ 真值` ⇒ 单调下降；
/// 除法走本模块的 `div`（大数时自动绕开 num-bigint 的 BZ 断言缺陷）。
pub fn int_nth_root(n: &BigInt, k: u32) -> BigInt {
    if n.is_zero() || n.is_negative() {
        return BigInt::zero();
    }
    if k <= 1 {
        return n.clone();
    }
    if k == 2 {
        return int_sqrt(n);
    }
    let bits = n.bits();
    // k 大于位数时根必为 1（n ≥ 2）：提前返回，否则下面 x.pow(k-1) 会构造天文级大数
    if (k as u64) > bits {
        return BigInt::one();
    }
    let mut x = BigInt::one() << (bits.div_ceil(k as u64));
    let kk = BigInt::from(k);
    let km1 = BigInt::from(k - 1);
    loop {
        let p = x.pow(k - 1);
        let q = div(n, &p);
        if q >= x {
            break;
        }
        x = (&x * &km1 + &q) / &kk;
    }
    // 收尾修正（正常路径无需修正，仅作保险）
    while x.pow(k) > *n {
        x -= BigInt::one();
    }
    while (x.clone() + BigInt::one()).pow(k) <= *n {
        x += BigInt::one();
    }
    x
}

/// 素性判定：小素数试除 + Miller-Rabin。
/// 不用"试除到 √n"（10^13 就要 3×10^6 次循环、约 3 秒），改用 12 个底的 Miller-Rabin
/// （对 n < 3.3×10^24 是确定性判定，更大为强概率判定）。
pub fn is_prime(n: &BigInt) -> bool {
    if n < &BigInt::from(2u32) {
        return false;
    }
    const SMALL: [u64; 25] = [
        2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53, 59, 61, 67, 71, 73, 79, 83, 89,
        97,
    ];
    for p in SMALL {
        let pb = BigInt::from(p);
        if n == &pb {
            return true;
        }
        if (n % &pb).is_zero() {
            return false;
        }
    }
    // 小素数之后的试除上限按工作量折算（位数越大每次取模越贵）
    let words = (n.bits() as usize).div_ceil(64).max(1);
    let max_p = (200_000usize / words).clamp(101, 200_000) as u64;
    let mut p = 101u64;
    while p <= max_p {
        let pb = BigInt::from(p);
        if &pb >= n {
            // 试除因子已经不小 n 本身：n 是素数（否则早被更小的因子判掉了）
            break;
        }
        if (n % &pb).is_zero() {
            return false;
        }
        p += 2;
    }
    miller_rabin(n)
}

/// Miller-Rabin：底取 2..37 共 12 个
fn miller_rabin(n: &BigInt) -> bool {
    let one = BigInt::one();
    let n_minus_1 = n - &one;
    let mut d = n_minus_1.clone();
    let mut r = 0u32;
    while d.is_even() {
        d /= 2u32;
        r += 1;
    }
    let two = BigInt::from(2u32);
    'outer: for a in [2u64, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37] {
        let a = BigInt::from(a);
        if &a >= n {
            continue;
        }
        let mut x = a.modpow(&d, n);
        if x == one || x == n_minus_1 {
            continue;
        }
        for _ in 0..r.saturating_sub(1) {
            x = x.modpow(&two, n);
            if x == n_minus_1 {
                continue 'outer;
            }
        }
        return false;
    }
    true
}

/// 大于 n 的最小素数（n < 2 时返回 2）
pub fn next_prime(n: &BigInt) -> BigInt {
    let two = BigInt::from(2u32);
    if n < &two {
        return two;
    }
    let mut cand = if n.is_even() {
        n + BigInt::one()
    } else {
        n + &two
    };
    loop {
        if is_prime(&cand) {
            return cand;
        }
        cand += &two;
    }
}

/* ---------------- 自带教材式长除法（Knuth Algorithm D） ---------------- */

/// 大数除法：按符号拆开，内部用 2^64 进制长除法
fn div_rem_schoolbook(a: &BigInt, b: &BigInt) -> (BigInt, BigInt) {
    let ua = to_limbs(a.magnitude());
    let va = to_limbs(b.magnitude());
    let (q_limbs, r_limbs) = div_rem_mag(&ua, &va);
    // 商的符号 = 两数符号之积；余数符号随被除数（与 Rust 的 `/`、`%` 语义一致）
    let q_sign = if a.sign() == b.sign() {
        Sign::Plus
    } else {
        Sign::Minus
    };
    let q = BigInt::from_biguint(q_sign, from_limbs(&q_limbs));
    let r = BigInt::from_biguint(a.sign(), from_limbs(&r_limbs));
    (q, r)
}

/// BigUint → 小端 u64 limb 数组（去掉高位零）
fn to_limbs(n: &BigUint) -> Vec<u64> {
    let mut v = n.to_u64_digits();
    while matches!(v.last(), Some(0)) {
        v.pop();
    }
    v
}

/// 小端 u64 limb 数组 → BigUint（用字节序构造，避免平台 BigDigit 宽度差异）
fn from_limbs(limbs: &[u64]) -> BigUint {
    let mut bytes = Vec::with_capacity(limbs.len() * 8);
    for d in limbs {
        bytes.extend_from_slice(&d.to_le_bytes());
    }
    BigUint::from_bytes_le(&bytes)
}

/// 长除法核心：u、v 均为小端 u64 limb 数组（无高位零），v 非空
fn div_rem_mag(u: &[u64], v: &[u64]) -> (Vec<u64>, Vec<u64>) {
    let u = trim(u);
    let v = trim(v);
    if v.is_empty() {
        panic!("整数除法：除数为零");
    }
    if cmp_mag(u, v) == core::cmp::Ordering::Less {
        // 被除数小于除数
        return (Vec::new(), u.to_vec());
    }
    if v.len() == 1 {
        return div_rem_single(u, v[0]);
    }

    // Knuth Algorithm D（基 B = 2^64）
    const B: u128 = 1u128 << 64;
    const MASK: u128 = B - 1;
    let n = v.len();
    let m = u.len() - n;

    // 归一化：把除数最高位移到最高位，使商位的估计误差 ≤ 2
    let shift = v[n - 1].leading_zeros();
    let vn = shl_bits(v, shift);
    let mut un = shl_bits(u, shift);
    un.push(0); // 额外的高位 limb

    let mut q = vec![0u64; m + 1];
    for j in (0..=m).rev() {
        // 商位估计：取被除数高两位除以除数最高位
        let num = ((un[j + n] as u128) << 64) | un[j + n - 1] as u128;
        let mut qhat = num / vn[n - 1] as u128;
        let mut rhat = num % vn[n - 1] as u128;
        if qhat > MASK {
            // 仅当除数最高位为 2^63 时可能出现，退化为最大单位数
            qhat = MASK;
            rhat = num - qhat * vn[n - 1] as u128;
        }
        // 用除数的次高位修正估计（最多修正两次）
        while rhat <= MASK && qhat * vn[n - 2] as u128 > ((rhat << 64) | un[j + n - 2] as u128) {
            qhat -= 1;
            rhat += vn[n - 1] as u128;
        }

        // 乘减：un[j..=j+n] -= qhat * vn
        let mut borrow: i128 = 0;
        let mut carry: u128 = 0;
        for i in 0..n {
            let p = qhat * vn[i] as u128 + carry;
            carry = p >> 64;
            let diff = un[i + j] as i128 - (p & MASK) as i128 - borrow;
            un[i + j] = diff as u64;
            borrow = if diff < 0 { 1 } else { 0 };
        }
        let diff = un[j + n] as i128 - carry as i128 - borrow;
        un[j + n] = diff as u64;

        if diff < 0 {
            // 估计偏大：回加一次除数（qhat -= 1）
            q[j] = (qhat - 1) as u64;
            let mut carry2: u128 = 0;
            for i in 0..n {
                let s = un[i + j] as u128 + vn[i] as u128 + carry2;
                un[i + j] = s as u64;
                carry2 = s >> 64;
            }
            un[j + n] = (un[j + n] as u128 + carry2) as u64;
        } else {
            q[j] = qhat as u64;
        }
    }

    // 余数：取低 n 个 limb 再右移还原
    let mut r = un[..n].to_vec();
    r = shr_bits(&r, shift);
    (trim(&q).to_vec(), trim(&r).to_vec())
}

/// 单 limb 除法（除数 ≤ u64::MAX，用 u128 逐位长除）
fn div_rem_single(u: &[u64], d: u64) -> (Vec<u64>, Vec<u64>) {
    let d = d as u128;
    let mut q = vec![0u64; u.len()];
    let mut rem: u128 = 0;
    for i in (0..u.len()).rev() {
        let cur = (rem << 64) | u[i] as u128;
        q[i] = (cur / d) as u64;
        rem = cur % d;
    }
    let mut r = Vec::new();
    if rem != 0 {
        r.push(rem as u64);
    }
    (trim(&q).to_vec(), r)
}

/// 去掉高位零 limb
fn trim(a: &[u64]) -> &[u64] {
    let mut end = a.len();
    while end > 0 && a[end - 1] == 0 {
        end -= 1;
    }
    &a[..end]
}

/// 比较两个 limb 数组（高位在前比较）
fn cmp_mag(a: &[u64], b: &[u64]) -> core::cmp::Ordering {
    if a.len() != b.len() {
        return a.len().cmp(&b.len());
    }
    for i in (0..a.len()).rev() {
        if a[i] != b[i] {
            return a[i].cmp(&b[i]);
        }
    }
    core::cmp::Ordering::Equal
}

/// 左移若干位（< 64）
fn shl_bits(a: &[u64], shift: u32) -> Vec<u64> {
    if shift == 0 {
        return a.to_vec();
    }
    let mut out = vec![0u64; a.len() + 1];
    let mut carry = 0u64;
    for i in 0..a.len() {
        out[i] = (a[i] << shift) | carry;
        carry = a[i] >> (64 - shift);
    }
    out[a.len()] = carry;
    while matches!(out.last(), Some(0)) {
        out.pop();
    }
    out
}

/// 右移若干位（< 64）
fn shr_bits(a: &[u64], shift: u32) -> Vec<u64> {
    if shift == 0 {
        return a.to_vec();
    }
    let mut out = vec![0u64; a.len()];
    let mut carry = 0u64;
    for i in (0..a.len()).rev() {
        out[i] = (a[i] >> shift) | carry;
        carry = a[i] << (64 - shift);
    }
    while matches!(out.last(), Some(0)) {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_bigint::BigInt;

    fn bi(s: &str) -> BigInt {
        s.parse().unwrap()
    }

    /// 校验除法恒等式：a = q·b + r，|r| < |b|，且余数符号随被除数（与 Rust `/` 语义一致）
    fn check_div(a: &BigInt, b: &BigInt) {
        let q = div(a, b);
        let r = a - &q * b;
        assert!(r.abs() < b.abs(), "余数超界: {a} / {b} => r={r}");
        if !r.is_zero() {
            assert_eq!(r.sign(), a.sign(), "余数符号应随被除数: {a} / {b}");
        }
    }

    #[test]
    fn test_schoolbook_small_and_large() {
        // 小规模走原生路径
        check_div(&bi("123456789012345678901234567890"), &bi("987654321"));
        check_div(&bi("-123456789012345678901234567890"), &bi("987654321"));
        check_div(&bi("123456789012345678901234567890"), &bi("-987654321"));
        check_div(&bi("-123456789012345678901234567890"), &bi("-987654321"));
        // 大规模强制走自带实现（被除数 200 limb、除数 100 limb）
        let a = bi("1").mul_pow10(60);
        let b = bi("1").mul_pow10(30) + BigInt::from(7);
        check_div(&a, &b);
        check_div(&-a.clone(), &b);
        // 被除数 ≈ 除数²（正是 num-bigint BZ 断言缺陷的形状：除数 157 limb、被除数 313 limb）
        let s = bi("1").mul_pow10(3_000) + BigInt::from(1);
        check_div(&(&s * &s), &s);
        check_div(&(&s * &s), &(&s + BigInt::from(1)));
        check_div(&(&s * &s), &(-&s));
        // 非对称规模
        check_div(
            &bi("1").mul_pow10(5_000),
            &(bi("7").mul_pow10(2_500) + BigInt::from(3)),
        );
    }

    #[test]
    fn test_is_prime_and_next_prime() {
        // 小素数表内/外的素数
        for p in [2u32, 3, 5, 97, 101, 7919, 104729] {
            assert!(is_prime(&BigInt::from(p)), "{p} 应为素数");
        }
        // 合数、1、0、负数
        for c in [0u32, 1, 4, 100, 7917, 104729 * 3] {
            assert!(!is_prime(&BigInt::from(c)), "{c} 不应为素数");
        }
        assert!(!is_prime(&BigInt::from(-7i32)));
        // Carmichael 数（纯试除会误判为素数，Miller-Rabin 能识别）
        for k in [561u32, 1105, 1729, 2465, 6601] {
            assert!(!is_prime(&BigInt::from(k)), "Carmichael 数 {k} 不是素数");
        }
        // 梅森素数 2^61-1 与费马数 2^32+1（= 641×6700417，欧拉分解）
        let m61 = BigInt::from(2u32).pow(61) - BigInt::one();
        assert!(is_prime(&m61), "2^61-1 是素数");
        let f5 = BigInt::from(2u32).pow(32) + BigInt::one();
        assert!(!is_prime(&f5), "2^32+1 是合数");
        // next_prime
        assert_eq!(next_prime(&BigInt::from(0)), BigInt::from(2));
        assert_eq!(next_prime(&BigInt::from(2)), BigInt::from(3));
        assert_eq!(next_prime(&BigInt::from(100)), BigInt::from(101));
        assert_eq!(next_prime(&BigInt::from(101)), BigInt::from(103));
        // 单调且为素数
        let mut n = BigInt::from(1000);
        for _ in 0..5 {
            let np = next_prime(&n);
            assert!(np > n);
            assert!(is_prime(&np));
            n = np;
        }
    }

    #[test]
    fn test_int_nth_root() {
        // 完全立方/完全五次方
        assert_eq!(int_nth_root(&BigInt::from(27u32), 3), BigInt::from(3));
        assert_eq!(int_nth_root(&BigInt::from(26u32), 3), BigInt::from(2));
        assert_eq!(
            int_nth_root(&BigInt::from(1_000_000u32), 3),
            BigInt::from(100)
        );
        assert_eq!(int_nth_root(&BigInt::from(32u32), 5), BigInt::from(2));
        assert_eq!(int_nth_root(&BigInt::from(31u32), 5), BigInt::from(1));
        // 保序与回验：r^k ≤ n < (r+1)^k
        let n = BigInt::from(10u64).pow(30) + BigInt::from(7);
        for k in [3u32, 4, 5, 7] {
            let r = int_nth_root(&n, k);
            assert!(r.pow(k) <= n, "k={k} 下界不满足");
            assert!((r.clone() + BigInt::one()).pow(k) > n, "k={k} 上界不满足");
        }
        // 边界
        assert_eq!(int_nth_root(&BigInt::from(0), 3), BigInt::from(0));
        assert_eq!(int_nth_root(&BigInt::from(7), 1), BigInt::from(7));
    }

    #[test]
    fn test_int_sqrt_identities() {
        // 完全平方
        for k in [4u64, 100, 1_000, 5_000] {
            let s = BigInt::from(10u64).pow(k as u32) + BigInt::from(1);
            assert_eq!(int_sqrt(&(&s * &s)), s);
            assert_eq!(int_sqrt(&(&s * &s - BigInt::from(1))), &s - BigInt::from(1));
            assert_eq!(int_sqrt(&(&s * &s + BigInt::from(1))), s);
        }
        // 大规模非完全平方：满足 s² ≤ n < (s+1)²
        let n = BigInt::from(10u64).pow(20_000) * BigInt::from(2);
        let s = int_sqrt(&n);
        assert!(&s * &s <= n);
        assert!((&s + BigInt::from(1)) * (&s + BigInt::from(1)) > n);
        // 零与小数
        assert_eq!(int_sqrt(&BigInt::from(0)), BigInt::from(0));
        assert_eq!(int_sqrt(&BigInt::from(15)), BigInt::from(3));
    }
}

/// 测试辅助：BigInt 的 10^k（仅测试使用）
#[cfg(test)]
trait MulPow10 {
    fn mul_pow10(&self, k: u32) -> BigInt;
}

#[cfg(test)]
impl MulPow10 for BigInt {
    fn mul_pow10(&self, k: u32) -> BigInt {
        self * BigInt::from(10u64).pow(k)
    }
}
