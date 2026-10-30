//! **定宽栈上大数**（512 位）：热循环里**零堆分配**。
//!
//! ## 为什么
//!
//! change_log122 的实测结论：`num_bigint::BigInt` 的**每次运算都堆分配**，
//! 而 80 位精度下数字只有 ~9 个 limb —— 算术只要 100ns，分配却要几百 ns。
//! 换个进制（十进制→二进制）只省了除法，分配一分没省，所以整段只有 1.16×。
//!
//! 这里改成**定宽数组**（`[u64; 8]`）+ 符号位，全程在栈上：
//! `poly_eval` 一次求值的 400 次运算不再产生任何分配。
//!
//! ## 表示
//!
//! `value = (−1)^neg · M / 2^FRAC`，`M` 是 512 位无符号整数（`w[0]` 最低位）。
//! `FRAC = 268`：`80 × log2(10) ≈ 265.8`，留 2 位余量 —— 与 `BigFloat` 的
//! "80 位十进制"精度对齐。
//!
//! ## 适用范围（**不是**通用大数）
//!
//! 只服务求根/积分这类"数值在 1 附近、要几百次小运算"的场景：
//! - 溢出（|M| ≥ 2^512）时**饱和**并置 `over` 标志，由调用方决定是否退回 `BigFloat`；
//! - 没有除法（除法仍用 `BigFloat`，它只占每轮 O(n) 次，不影响大局）。

use num_bigint::BigInt;
use num_traits::{Signed, Zero};

/// 512 位 = 8 个 64 位 limb
pub const LIMBS: usize = 8;
/// 小数位数：80 位十进制 ≈ 265.8 位二进制，取 268 留余量
pub const FRAC: usize = 268;

/// 定宽有符号定点数（栈上，零分配）
#[derive(Clone, Copy, Debug)]
pub struct W {
    /// 幅值（小端：`w[0]` 最低 64 位）
    pub w: [u64; LIMBS],
    pub neg: bool,
    /// 溢出标志：置位后本值不可信，调用方应退回 `BigFloat`
    pub over: bool,
}

impl W {
    pub fn zero() -> Self {
        W {
            w: [0; LIMBS],
            neg: false,
            over: false,
        }
    }

    pub fn is_zero(&self) -> bool {
        self.w.iter().all(|x| *x == 0)
    }

    /// 从 i64
    pub fn from_i64(v: i64) -> Self {
        let neg = v < 0;
        let m = v.unsigned_abs();
        let mut w = [0u64; LIMBS];
        w[0] = m;
        // 整数 1.0 = 2^FRAC
        let mut out = W {
            w,
            neg,
            over: false,
        };
        out.shl(FRAC);
        out
    }

    /// 左移（整数 → 定点）
    fn shl(&mut self, bits: usize) {
        let words = bits / 64;
        let b = bits % 64;
        if b == 0 {
            if words > 0 {
                for i in (0..LIMBS).rev() {
                    self.w[i] = if i >= words { self.w[i - words] } else { 0 };
                }
            }
            return;
        }
        for i in (0..LIMBS).rev() {
            let lo = if i > words {
                self.w[i - words - 1] >> (64 - b)
            } else {
                0
            };
            let hi = if i >= words { self.w[i - words] << b } else { 0 };
            self.w[i] = lo | hi;
        }
    }

    /// 幅值相加（不进符号）
    fn add_mag(a: &[u64; LIMBS], b: &[u64; LIMBS]) -> ([u64; LIMBS], bool) {
        let mut out = [0u64; LIMBS];
        let mut carry = 0u64;
        for i in 0..LIMBS {
            let (s1, c1) = a[i].overflowing_add(b[i]);
            let (s2, c2) = s1.overflowing_add(carry);
            out[i] = s2;
            carry = u64::from(c1) + u64::from(c2);
        }
        (out, carry != 0)
    }

    /// 幅值比较：`a < b`
    fn mag_lt(a: &[u64; LIMBS], b: &[u64; LIMBS]) -> bool {
        for i in (0..LIMBS).rev() {
            if a[i] != b[i] {
                return a[i] < b[i];
            }
        }
        false
    }

    /// 幅值相减（要求 a ≥ b）
    fn sub_mag(a: &[u64; LIMBS], b: &[u64; LIMBS]) -> [u64; LIMBS] {
        let mut out = [0u64; LIMBS];
        let mut borrow = 0u64;
        for i in 0..LIMBS {
            let (d1, b1) = a[i].overflowing_sub(b[i]);
            let (d2, b2) = d1.overflowing_sub(borrow);
            out[i] = d2;
            borrow = u64::from(b1) + u64::from(b2);
        }
        out
    }

    /// 加法
    pub fn add(&self, o: &Self) -> Self {
        if self.over || o.over {
            return W { over: true, ..W::zero() };
        }
        if self.neg == o.neg {
            let (w, over) = Self::add_mag(&self.w, &o.w);
            return W {
                w,
                neg: self.neg,
                over,
            };
        }
        // 异号：大减小
        if Self::mag_lt(&self.w, &o.w) {
            W {
                w: Self::sub_mag(&o.w, &self.w),
                neg: o.neg,
                over: false,
            }
        } else {
            W {
                w: Self::sub_mag(&self.w, &o.w),
                neg: self.neg,
                over: false,
            }
        }
    }

    /// 减法
    pub fn sub(&self, o: &Self) -> Self {
        self.add(&W {
            w: o.w,
            neg: !o.neg && !o.is_zero(),
            over: o.over,
        })
    }

    /// 取负
    pub fn neg(&self) -> Self {
        W {
            w: self.w,
            neg: !self.neg,
            over: self.over,
        }
    }

    /// 乘法：512×512 → 1024 位中间结果，右移 `FRAC` 并四舍五入
    pub fn mul(&self, o: &Self) -> Self {
        if self.over || o.over {
            return W { over: true, ..W::zero() };
        }
        if self.is_zero() || o.is_zero() {
            return W::zero();
        }
        let mut prod = [0u64; 2 * LIMBS];
        for i in 0..LIMBS {
            if self.w[i] == 0 {
                continue;
            }
            let mut carry: u128 = 0;
            for j in 0..LIMBS {
                let t = prod[i + j] as u128 + (self.w[i] as u128) * (o.w[j] as u128) + carry;
                prod[i + j] = t as u64;
                carry = t >> 64;
            }
            let mut k = i + LIMBS;
            while carry != 0 && k < 2 * LIMBS {
                let t = prod[k] as u128 + carry;
                prod[k] = t as u64;
                carry = t >> 64;
                k += 1;
            }
        }
        // 四舍五入：被丢掉的最高位（第 FRAC-1 位）是 1 时，在**第 FRAC 位**加 1。
        //
        // ⚠️ 这里原来写的是 `carry = 1`（即在 limb FRAC/64 的**最低位**加 1 = 2^256），
        // 折回后相当于每次都注入 2^(256-268) = 2^-12 的偏差 —— 比目标精度 2^-268 大了 200 多个数量级，
        // 求根迭代永远收敛不了（这就是"W 版 DK 不发散但不收敛"的真正原因）。
        let (rb, rw) = ((FRAC - 1) / 64, (FRAC - 1) % 64);
        if prod[rb] & (1u64 << rw) != 0 {
            let mut i = FRAC / 64;
            let mut carry = 1u128 << (FRAC % 64);
            while i < 2 * LIMBS && carry != 0 {
                let t = prod[i] as u128 + carry;
                prod[i] = t as u64;
                carry = t >> 64;
                i += 1;
            }
        }
        // 右移 FRAC
        let words = FRAC / 64;
        let bits = FRAC % 64;
        let mut out = [0u64; LIMBS];
        for i in 0..LIMBS {
            let lo = prod[i + words] >> bits;
            let hi = if bits == 0 {
                0
            } else if i + words + 1 < 2 * LIMBS {
                prod[i + words + 1] << (64 - bits)
            } else {
                0
            };
            out[i] = lo | hi;
        }
        // 溢出：被移掉的高位非零，或结果最高位已满
        let mut over = false;
        for v in prod.iter().skip(LIMBS + words) {
            if *v != 0 {
                over = true;
            }
        }
        W {
            w: out,
            neg: self.neg != o.neg,
            over,
        }
    }

    /// 从 `BigFloat` 构造：`value = raw / 10^p` ⇒ `M = round(raw · 2^FRAC / 10^p)`
    ///
    /// 一次 BigInt 除法（进出各一次，相对于内层上万次运算可忽略）。
    /// 超出 512 位返回 `None`（调用方退回 BigFloat）。
    pub fn from_bigfloat(v: &crate::bigfloat::BigFloat) -> Option<Self> {
        use num_integer::Integer;
        if v.is_zero() {
            return Some(Self::zero());
        }
        let num = &v.value << FRAC;
        let den = BigInt::from(10).pow(v.precision as u32);
        let (q, r) = num.div_rem(&den);
        // 四舍五入
        let q = if &r * 2u32 >= den { q + 1u32 } else { q };
        let neg = q.is_negative();
        let abs = q.abs();
        if abs.bits() > (LIMBS * 64) as u64 {
            return None;
        }
        let mut w = [0u64; LIMBS];
        let mut cur = abs;
        let mask = BigInt::from(u64::MAX);
        for slot in w.iter_mut() {
            *slot = (&cur & &mask).iter_u64_digits().next().unwrap_or(0);
            cur >>= 64;
        }
        Some(W { w, neg, over: false })
    }

    /// 转回 `BigFloat`（按 `prec` 十进制位）：`q = M · 10^prec >> FRAC`
    pub fn to_bigfloat(&self, prec: usize) -> crate::bigfloat::BigFloat {
        use num_integer::Integer;
        let mut m = BigInt::from(0);
        for i in (0..LIMBS).rev() {
            m <<= 64;
            m += BigInt::from(self.w[i]);
        }
        if self.neg && !m.is_zero() {
            m = -m;
        }
        let num = m * BigInt::from(10).pow(prec as u32);
        let den = BigInt::from(1) << FRAC;
        let (q, r) = num.div_rem(&den);
        let q = if (&r * 2u32).abs() >= den { q + 1u32 } else { q };
        crate::bigfloat::BigFloat {
            value: q,
            precision: prec,
        }
    }

    /// 近似成 f64（**仅供调试/诊断**：量级对了就够用，别拿它做运算）
    pub fn to_f64_approx(&self) -> f64 {
        let hi = self.w[LIMBS - 1] as f64;
        let lo = if LIMBS >= 2 { self.w[LIMBS - 2] as f64 } else { 0.0 };
        let mag = hi * 2f64.powi((LIMBS as i32 - 1) * 64) + lo * 2f64.powi((LIMBS as i32 - 2) * 64);
        let v = mag * 2f64.powi(-(FRAC as i32));
        if self.neg { -v } else { v }
    }

    /// 量级的十进制对数（**仅诊断用**，例：`-20.0` 表示约 1e-20）
    pub fn mag_log10(&self) -> f64 {
        let mut n = 0usize;
        for i in (0..LIMBS).rev() {
            if self.w[i] != 0 {
                n = i * 64 + (64 - self.w[i].leading_zeros() as usize);
                break;
            }
        }
        if n == 0 {
            return f64::NEG_INFINITY;
        }
        n as f64 * std::f64::consts::LOG10_2 - FRAC as f64 * std::f64::consts::LOG10_2
    }

    /// 取绝对值
    pub fn abs(&self) -> Self {
        W {
            w: self.w,
            neg: false,
            over: self.over,
        }
    }

    /// 幅值比较（`|self| < |o|`）
    pub fn abs_lt(&self, o: &Self) -> bool {
        Self::mag_lt(&self.w, &o.w)
    }
}

#[cfg(test)]
mod tests {
    /// **`BigFloat ⇄ W` 往返**：相对误差 < 10^-70，且 `from_bigfloat` 不返回 None
    #[test]
    fn wide_bigfloat_roundtrip() {
        use crate::bigfloat::BigFloat;
        let prec = 80usize;
        let mut s = 99u64;
        for _ in 0..100 {
            let raw = BigInt::from(lcg(&mut s) % 1_000_000_000_000_000_000 + 1);
            let v = BigFloat::from_scaled(raw, 0, prec);
            let w = W::from_bigfloat(&v).expect("应当能放下");
            let back = w.to_bigfloat(prec);
            let diff = BigFloat::sub(&back, &v, prec);
            let rel = if diff.is_zero() {
                f64::NEG_INFINITY
            } else {
                diff.magnitude_log10() - v.magnitude_log10()
            };
            assert!(rel < -70.0, "往返误差 10^{rel:.1}");
        }
    }

    use super::*;
    use num_bigint::BigInt;
    use num_traits::{Signed, Zero};

    /// 确定性伪随机
    fn lcg(s: &mut u64) -> u64 {
        *s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        *s >> 11
    }

    /// 造一个 0 < |值| < 1 附近的随机定点数
    fn rand_w(s: &mut u64) -> W {
        let mut w = [0u64; LIMBS];
        // 让幅值落在 2^267..2^268（即 0.5..1.0）
        w[4] = 1u64 << 12; // 2^(4*64+12) = 2^268
        for slot in w.iter_mut().take(4) {
            *slot = lcg(s);
        }
        let neg = lcg(s) % 2 == 0;
        W {
            w,
            neg,
            over: false,
        }
    }

    /// 转成 BigInt 便于比对（返回 M，调用方按 2^FRAC 解释）
    fn to_mag_big(w: &W) -> BigInt {
        let mut out = BigInt::from(0);
        for i in (0..LIMBS).rev() {
            out <<= 64;
            out += BigInt::from(w.w[i]);
        }
        if w.neg && !out.is_zero() {
            -out
        } else {
            out
        }
    }

    /// **等价性**：`W` 的加减乘与 `BigInt` 的精确结果一致（误差 ≤ 1 个最低位）
    #[test]
    fn wide_matches_bigint() {
        let mut s = 12345u64;
        for _ in 0..500 {
            let a = rand_w(&mut s);
            let b = rand_w(&mut s);
            let (am, bm) = (to_mag_big(&a), to_mag_big(&b));

            // 加
            let want = &am + &bm;
            let got = to_mag_big(&a.add(&b));
            assert!(!a.add(&b).over, "加法不该溢出");
            assert_eq!(got, want, "加法不一致");

            // 减
            let want = &am - &bm;
            assert_eq!(to_mag_big(&a.sub(&b)), want, "减法不一致");

            // 乘：期望 = round(am·bm / 2^FRAC)
            let prod = &am * &bm;
            let half = BigInt::from(1) << (FRAC - 1);
            let want = (&prod + &half) >> FRAC;
            let mut got = to_mag_big(&a.mul(&b));
            if got.is_negative() && want.is_zero() {
                got = BigInt::from(0);
            }
            let diff = (&got - &want).abs();
            assert!(diff <= BigInt::from(1), "乘法误差过大：{diff}");
        }
    }

    /// **最小复现**：三次数值求值的每一步有没有溢出
    #[test]
    fn wide_horner_no_overflow() {
        // 系数 c = [1, -6, 11, -6] ⇒ p(z) = c0 + c1·z + c2·z² + c3·z³
        // （Horner 从最高项起，实现在 `poly_eval_*` 里）
        let z = W::from_i64(2);
        let cf = [W::from_i64(1), W::from_i64(-6), W::from_i64(11), W::from_i64(-6)];
        let mut acc = W::zero();
        for c in cf.iter().rev() {
            let m = acc.mul(&z);
            assert!(!acc.over && !m.over, "乘法溢出：acc={acc:?}");
            acc = m.add(c);
            assert!(!acc.over, "加法溢出：acc={acc:?}");
        }
        // 系数顺序是 [c0..c3] ⇒ Horner 从 c3 起：p(z) = -6z³ + 11z² - 6z + 1
        // p(2) = -48 + 44 - 12 + 1 = -15
        let want = W::from_i64(15).neg();
        assert_eq!(acc.w, want.w, "p(2) 应为 -15，实际 {acc:?}");
        assert_eq!(acc.neg, want.neg, "符号不对：{acc:?}");
    }

    /// 单运算成本：`W` 的乘/加 vs `BigInt` 的对应运算（min-of-5，各 20000 次）
    #[test]
    #[ignore]
    fn wide_primitive_bench() {
        use std::time::Instant;
        let mut s = 7u64;
        let a = rand_w(&mut s);
        let b = rand_w(&mut s);
        let (am, bm) = (to_mag_big(&a), to_mag_big(&b));

        let bench = |name: &str, mut f: Box<dyn FnMut()>| {
            let mut best = std::time::Duration::MAX;
            for _ in 0..5 {
                let t = Instant::now();
                for _ in 0..20000 {
                    f();
                }
                best = best.min(t.elapsed());
            }
            println!(
                "{name:12} {:.3} µs/op",
                best.as_secs_f64() * 1e6 / 20000.0
            );
        };

        bench("W::mul", Box::new(move || {
            std::hint::black_box(a.mul(&b));
        }));
        bench("W::add", Box::new(move || {
            std::hint::black_box(a.add(&b));
        }));
        let (am2, bm2) = (am.clone(), bm.clone());
        bench("BigInt::mul", Box::new(move || {
            std::hint::black_box(&am * &bm);
        }));
        bench("BigInt::add", Box::new(move || {
            std::hint::black_box(&am2 + &bm2);
        }));
    }
}
