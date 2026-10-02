//! 整数的素因数分解（供 `primefac(...)` 与 `fac`/`factor` 的整数入参使用）。
//!
//! 两个约定：
//! - 只处理**非零整数**；符号由调用方处理（本模块内部始终对 `|n|` 分解）；
//! - 试除有**预算**：Fast 模式下试除数不超过 `TRIAL_MAX_FAST`，若剩余的合数因子还有
//!   更大的素因子，就明确报错而不是给出残缺结果（`/mode deep` 取消预算）。
//!   这与 `solver_factor::int_divisors` 的上限保持一致——试除每次都要取模，
//!   上限放到 10^13 就是万亿次循环，界面等效卡死。

use num_bigint::BigInt;
use num_traits::{One, Signed, ToPrimitive, Zero};

/// Fast 模式下的试除上限（试除数本身的上界）
const TRIAL_MAX_FAST: u64 = 1_000_000;

/// 试除预算用尽（已知剩余部分还是合数）时的统一报错文案
const INCOMPLETE_MSG: &str =
    "素因数分解超出试除预算（Fast 模式试除上限 10^6）；如确认需要继续，请先执行 /mode deep";

/// 素因数分解：返回按素数**升序**排列的 `(素数, 指数)` 列表。
///
/// `n` 必须为正（调用方先取绝对值）。`1` 没有素因数 ⇒ 返回空列表。
/// Fast 模式下若剩余合数含 > 10^6 的素因子，返回 `Err`。
pub fn prime_factorize(n: &BigInt) -> Result<Vec<(BigInt, u32)>, String> {
    debug_assert!(n.is_positive(), "prime_factorize 只接受正数");
    if n.is_zero() {
        return Err(INCOMPLETE_MSG.to_string());
    }
    let deep = crate::calc_mode::is_deep();
    // 能塞进 u64 就走 u64 试除：debug 下 u64 取模比 BigInt 快两个数量级
    // （与 `solver_factor::int_divisors` 的做法一致）
    if let Some(m) = n.to_u64() {
        return Ok(factor_u64(m, deep)?
            .into_iter()
            .map(|(p, e)| (BigInt::from(p), e))
            .collect());
    }
    factor_bigint(n, deep)
}

/// 把分解结果格式化成等式的形式：`12 = 2^2 * 3`、`-12 = -2^2 * 3`、`7 = 7`、`1 = 1`。
///
/// - 指数为 1 时省略 `^1`；
/// - 负号按约定顶在**最前面**（`-12 = -2^2 * 3`）；
/// - `0` 没有素因数分解 ⇒ `Err`。
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
    Ok(format!("{} = {}{}", n, if neg { "-" } else { "" }, body))
}

/// 追加一个素因子（升序，故相同因子必定紧邻，合并为指数）
fn push_factor<T: PartialEq>(out: &mut Vec<(T, u32)>, p: T) {
    if let Some(last) = out.last_mut() {
        if last.0 == p {
            last.1 += 1;
            return;
        }
    }
    out.push((p, 1));
}

/// `u64` 试除路径
fn factor_u64(mut m: u64, deep: bool) -> Result<Vec<(u64, u32)>, String> {
    let mut out: Vec<(u64, u32)> = Vec::new();
    while m % 2 == 0 {
        push_factor(&mut out, 2);
        m /= 2;
    }
    let mut d = 3u64;
    while m > 1 {
        // 试除数超过 √m ⇒ 剩下的 m 必为素数
        if d.checked_mul(d).is_none_or(|dd| dd > m) {
            break;
        }
        if !deep && d > TRIAL_MAX_FAST {
            break;
        }
        while m % d == 0 {
            push_factor(&mut out, d);
            m /= d;
        }
        d += 2;
    }
    if m > 1 {
        if crate::bigint_ext::is_prime(&BigInt::from(m)) {
            push_factor(&mut out, m);
        } else {
            return Err(INCOMPLETE_MSG.to_string());
        }
    }
    Ok(out)
}

/// `BigInt` 试除路径（超出 u64 的大数）
fn factor_bigint(n: &BigInt, deep: bool) -> Result<Vec<(BigInt, u32)>, String> {
    let mut out: Vec<(BigInt, u32)> = Vec::new();
    let mut m = n.clone();
    let two = BigInt::from(2u32);
    while (&m % &two).is_zero() {
        push_factor(&mut out, two.clone());
        m /= &two;
    }
    let bound = BigInt::from(TRIAL_MAX_FAST);
    let mut d = BigInt::from(3u32);
    while !m.is_one() {
        if &d * &d > m {
            break;
        }
        if !deep && d > bound {
            break;
        }
        while (&m % &d).is_zero() {
            push_factor(&mut out, d.clone());
            m /= &d;
        }
        d += &two;
    }
    if !m.is_one() {
        if crate::bigint_ext::is_prime(&m) {
            push_factor(&mut out, m);
        } else {
            return Err(INCOMPLETE_MSG.to_string());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(n: i64) -> String {
        format_prime_factorization(&BigInt::from(n)).unwrap()
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
        // 素因子必须升序，指数合并
        assert_eq!(fmt(360), "360 = 2^3 * 3^2 * 5");
        assert_eq!(fmt(97), "97 = 97");
        assert_eq!(fmt(100), "100 = 2^2 * 5^2");
    }

    #[test]
    fn factor_list_is_sorted_and_collapsed() {
        let f = prime_factorize(&BigInt::from(360)).unwrap();
        let got: Vec<(i64, u32)> = f.iter().map(|(p, e)| (p.to_i64().unwrap(), *e)).collect();
        assert_eq!(got, vec![(2, 3), (3, 2), (5, 1)]);
        assert!(prime_factorize(&BigInt::from(1)).unwrap().is_empty());
    }

    #[test]
    fn zero_is_rejected() {
        assert!(format_prime_factorization(&BigInt::from(0)).is_err());
    }

    #[test]
    fn large_prime_and_bigint_path() {
        // 10^12 附近的素数：走 u64 路径，试除到 √n 之前就会被 is_prime 收下
        assert_eq!(fmt(999_999_999_989), "999999999989 = 999999999989");
        // 超出 u64 的大数走 BigInt 路径（这里取 2^70，只有小素因子，很快）
        let n = BigInt::from(2u32).pow(70);
        assert_eq!(
            format_prime_factorization(&n).unwrap(),
            format!("{} = 2^70", n)
        );
    }

    #[test]
    fn hard_semiprime_reports_clearly_in_fast_mode() {
        // 两个 > 10^6 的素数之积 ⇒ Fast 下试除预算不够，必须明确报错而不是给残缺结果
        let p = 1_000_003u64;
        let q = 1_000_033u64;
        let n = BigInt::from(p * q);
        let err = format_prime_factorization(&n).unwrap_err();
        assert!(err.contains("试除预算"), "{err}");
    }
}
