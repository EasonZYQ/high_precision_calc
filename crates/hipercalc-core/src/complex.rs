//! 复数：`z = re + im·i`，实部与虚部都是 `Number`。
//!
//! 设计取舍：
//! - 复用 `Number` 的**精确运算管线**（加减乘除、整数幂、负实数的开方都能给精确结果，
//!   例如 `(2+3i)*(1-i) = 5+i`、`sqrt(-4) = 2i`、`i^4 = 1`）；
//! - 超越函数（`exp`/`ln`/三角）走既有高精度数值层（`BigFloat`，弧度制），结果自动落近似值；
//! - 其余函数（`floor`/`mod`/`gcd`/`nCr`…）在 `parser` 层直接报"复数不支持该函数"，
//!   不在这里做无意义的兜底。
//!
//! 注意：`BigFloat` 的三角/反三角都是**弧度制**，与用户的 `/mode deg|rad` 无关——
//! 角度制只影响 `parser` 层的用户函数，不影响内部数值计算。

use crate::bigfloat::{self, BigFloat};
use crate::number::Number;

/// 复数：实部 + 虚部（`im` 是 i 的系数）
#[derive(Debug, Clone)]
pub struct ComplexNum {
    pub re: Number,
    pub im: Number,
}

impl ComplexNum {
    pub fn new(re: Number, im: Number) -> Self {
        ComplexNum { re, im }
    }

    /// 由两个整数构造
    pub fn from_int_pair(re: i64, im: i64) -> Self {
        ComplexNum::new(Number::from_int(re), Number::from_int(im))
    }

    /// 虚数单位 i
    pub fn i_unit() -> Self {
        ComplexNum::from_int_pair(0, 1)
    }

    /// 是否退化为实数（虚部为 0）
    pub fn is_real(&self) -> bool {
        self.im.is_zero()
    }

    pub fn is_zero(&self) -> bool {
        self.re.is_zero() && self.im.is_zero()
    }

    pub fn neg(&self) -> Self {
        ComplexNum::new(self.re.neg(), self.im.neg())
    }

    /// 共轭：a + bi → a - bi
    pub fn conj(&self) -> Self {
        ComplexNum::new(self.re.clone(), self.im.neg())
    }

    /// |z|² = re² + im²（精确）
    pub fn abs2(&self) -> Number {
        self.re.mul(&self.re).add(&self.im.mul(&self.im))
    }

    /// |z|（完全平方时精确，否则数值）
    pub fn abs(&self) -> Number {
        self.abs2().sqrt()
    }

    pub fn add(&self, o: &Self) -> Self {
        ComplexNum::new(self.re.add(&o.re), self.im.add(&o.im))
    }

    /// (a+bi)(c+di) = (ac - bd) + (ad + bc)i
    pub fn mul(&self, o: &Self) -> Self {
        let ac = self.re.mul(&o.re);
        let bd = self.im.mul(&o.im);
        let ad = self.re.mul(&o.im);
        let bc = self.im.mul(&o.re);
        ComplexNum::new(ac.sub(&bd), ad.add(&bc))
    }

    /// (a+bi)/(c+di) = (a+bi)(c-di) / (c²+d²)，除数为 0 报错
    pub fn div(&self, o: &Self) -> Result<Self, String> {
        if o.is_zero() {
            return Err("除以零错误".to_string());
        }
        let d = o.abs2();
        let n = self.mul(&o.conj());
        Ok(ComplexNum::new(n.re.div(&d), n.im.div(&d)))
    }

    /// 整数幂（快速幂，失败只可能来自底数为 0 的负幂）
    pub fn int_pow(&self, n: u32) -> Result<Self, String> {
        let mut acc = ComplexNum::from_int_pair(1, 0);
        let mut base = self.clone();
        let mut k = n;
        while k > 0 {
            if k & 1 == 1 {
                acc = acc.mul(&base);
            }
            k >>= 1;
            if k > 0 {
                base = base.mul(&base);
            }
        }
        Ok(acc)
    }

    /// 数值化：实部、虚部的近似值
    pub fn approx_pair(&self) -> (BigFloat, BigFloat) {
        (self.re.to_approx(), self.im.to_approx())
    }

    /// exp(a+bi) = e^a·(cos b + i·sin b)
    pub fn exp(&self) -> Result<Self, String> {
        let prec = bigfloat::precision();
        let (a, b) = self.approx_pair();
        let ea = a.exp(prec)?;
        let re = BigFloat::mul(&ea, &b.cos(prec), prec);
        let im = BigFloat::mul(&ea, &b.sin(prec), prec);
        Ok(ComplexNum::new(Number::Approx(re), Number::Approx(im)))
    }

    /// 任意复数次幂（**主值**）：z^w = exp(w·ln z)。
    ///
    /// 用的是 `ln` 的主支（辐角 ∈ (−π, π]，由 `atan2` 给出）⇒ 结果是**主值**。
    /// 注意 z^w 本身是**多值**的（相差 e^{2πikw}），这里只给主值 —— 与常见数学软件一致。
    /// 整数指数仍走 `int_pow`（快速幂 + 精确），**不要**把它绕到这里来。
    pub fn pow(&self, w: &ComplexNum) -> Result<Self, String> {
        if self.is_zero() {
            return Err("0 的复数次幂未定义（0 的负次幂发散）".to_string());
        }
        let l = self.ln()?;
        w.mul(&l).exp()
    }

    /// ln(z) = ln|z| + i·arg(z)
    pub fn ln(&self) -> Result<Self, String> {
        if self.is_zero() {
            return Err("复数的对数要求 z != 0".to_string());
        }
        let prec = bigfloat::precision();
        let (a, b) = self.approx_pair();
        let ln_abs = self.abs().to_approx().ln(prec);
        let arg = atan2(&b, &a, prec);
        Ok(ComplexNum::new(Number::Approx(ln_abs), Number::Approx(arg)))
    }

    /// 平方根：负实数给精确虚根（`-4 → 2i`，虚部为完全平方时精确），其余走极坐标。
    pub fn sqrt(&self) -> Result<Self, String> {
        if self.is_zero() {
            return Ok(ComplexNum::from_int_pair(0, 0));
        }
        // 纯实数：非负直接走实数开方，负数的平方根是纯虚数
        if self.im.is_zero() {
            if !self.re.is_negative() {
                return Ok(ComplexNum::new(self.re.sqrt(), Number::from_int(0)));
            }
            let positive = self.re.neg();
            let root = positive.sqrt();
            if root.is_zero() {
                return Ok(ComplexNum::from_int_pair(0, 0));
            }
            return Ok(ComplexNum::new(Number::from_int(0), root));
        }
        // 一般情形：exp(ln(z)/2)
        let half = Number::from_rational(num_rational::BigRational::new(
            num_bigint::BigInt::from(1),
            num_bigint::BigInt::from(2),
        ));
        let l = self.ln()?;
        ComplexNum::new(l.re.mul(&half), l.im.mul(&half)).exp()
    }

    /// sin(a+bi) = sin a·cosh b + i·cos a·sinh b
    pub fn sin(&self) -> Result<Self, String> {
        let prec = bigfloat::precision();
        let (a, b) = self.approx_pair();
        let sh = sinh(&b, prec)?;
        let ch = cosh(&b, prec)?;
        Ok(ComplexNum::new(
            Number::Approx(BigFloat::mul(&a.sin(prec), &ch, prec)),
            Number::Approx(BigFloat::mul(&a.cos(prec), &sh, prec)),
        ))
    }

    /// cos(a+bi) = cos a·cosh b - i·sin a·sinh b
    pub fn cos(&self) -> Result<Self, String> {
        let prec = bigfloat::precision();
        let (a, b) = self.approx_pair();
        let sh = sinh(&b, prec)?;
        let ch = cosh(&b, prec)?;
        Ok(ComplexNum::new(
            Number::Approx(BigFloat::mul(&a.cos(prec), &ch, prec)),
            Number::Approx(BigFloat::neg(&BigFloat::mul(&a.sin(prec), &sh, prec))),
        ))
    }

    /// tan(z) = sin(z)/cos(z)
    pub fn tan(&self) -> Result<Self, String> {
        let s = self.sin()?;
        let c = self.cos()?;
        s.div(&c)
    }

    /// arg(z)（弧度，返回值在 (-π, π]）
    pub fn arg(&self) -> Result<Number, String> {
        if self.is_zero() {
            return Err("0 没有辐角".to_string());
        }
        let prec = bigfloat::precision();
        let (a, b) = self.approx_pair();
        Ok(Number::Approx(atan2(&b, &a, prec)))
    }
}

/// atan2(y, x)：按象限给出正确辐角（`BigFloat` 只有单参数 atan）
fn atan2(y: &BigFloat, x: &BigFloat, prec: usize) -> BigFloat {
    let zero = BigFloat::from_u64(0);
    let pi = BigFloat::pi(prec);
    if x.value > zero.value {
        return BigFloat::div(y, x, prec).atan(prec);
    }
    if x.value < zero.value {
        let base = BigFloat::div(y, x, prec).atan(prec);
        return if y.value >= zero.value {
            BigFloat::add(&base, &pi, prec)
        } else {
            BigFloat::sub(&base, &pi, prec)
        };
    }
    // x == 0：正负半虚轴
    let half_pi = BigFloat::div(&pi, &BigFloat::from_u64(2), prec);
    if y.value > zero.value {
        half_pi
    } else if y.value < zero.value {
        BigFloat::neg(&half_pi)
    } else {
        BigFloat::from_u64(0)
    }
}

/// sinh(x) = (e^x - e^-x)/2
fn sinh(x: &BigFloat, prec: usize) -> Result<BigFloat, String> {
    let ep = x.exp(prec)?;
    let en = BigFloat::neg(x).exp(prec)?;
    Ok(BigFloat::div(
        &BigFloat::sub(&ep, &en, prec),
        &BigFloat::from_u64(2),
        prec,
    ))
}

/// cosh(x) = (e^x + e^-x)/2
fn cosh(x: &BigFloat, prec: usize) -> Result<BigFloat, String> {
    let ep = x.exp(prec)?;
    let en = BigFloat::neg(x).exp(prec)?;
    Ok(BigFloat::div(
        &BigFloat::add(&ep, &en, prec),
        &BigFloat::from_u64(2),
        prec,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display;

    fn c(re: i64, im: i64) -> ComplexNum {
        ComplexNum::from_int_pair(re, im)
    }

    fn mathio(z: &ComplexNum) -> String {
        display::format_mathio(&Number::Complex(Box::new(z.clone())))
    }

    #[test]
    fn gaussian_arithmetic_is_exact() {
        // (2+3i)(1-i) = 5+i
        assert_eq!(mathio(&c(2, 3).mul(&c(1, -1))), "5 + i");
        // (1+i)^2 = 2i
        assert_eq!(mathio(&c(1, 1).mul(&c(1, 1))), "2i");
        // i^2 = -1、i^4 = 1
        assert_eq!(mathio(&c(0, 1).int_pow(2).unwrap()), "-1");
        assert_eq!(mathio(&c(0, 1).int_pow(4).unwrap()), "1");
        // 1/i = -i
        let one = ComplexNum::from_int_pair(1, 0);
        assert_eq!(mathio(&one.div(&c(0, 1)).unwrap()), "-i");
        // 除法：(5+i)/(1-i) = 2+3i
        assert_eq!(mathio(&c(5, 1).div(&c(1, -1)).unwrap()), "2 + 3i");
        // 除零
        assert!(c(1, 1).div(&c(0, 0)).is_err());
        // |3+4i| = 5（精确）
        assert_eq!(display::format_mathio(&c(3, 4).abs()), "5");
        // 共轭、减法与实部虚部
        assert_eq!(mathio(&c(2, -3).conj()), "2 + 3i");
        // 减法与 Number::sub 一致，走 add(neg)
        assert_eq!(mathio(&c(5, 4).add(&c(1, 1).neg())), "4 + 3i");
        assert!(c(3, 0).is_real() && !c(3, 1).is_real());
    }

    #[test]
    fn complex_sqrt_of_negative_is_exact() {
        // sqrt(-4) = 2i（精确）
        let s = c(-4, 0).sqrt().unwrap();
        assert_eq!(mathio(&s), "2i");
        // sqrt(-2) = sqrt(2)·i（精确根式）
        let s2 = c(-2, 0).sqrt().unwrap();
        assert!(mathio(&s2).contains("sqrt(2)"), "{}", mathio(&s2));
        // sqrt(4) = 2（退化为实数）
        assert_eq!(mathio(&c(4, 0).sqrt().unwrap()), "2");
        // sqrt(i) 走数值：≈ 0.7071 + 0.7071i，平方回验 ≈ i
        let si = c(0, 1).sqrt().unwrap();
        let back = si.mul(&si);
        assert!(
            display::format_lineio(&back.re).starts_with("0"),
            "re={}",
            display::format_lineio(&back.re)
        );
        assert!(
            display::format_lineio(&back.im).starts_with("1"),
            "im={}",
            display::format_lineio(&back.im)
        );
    }

    #[test]
    fn complex_transcendentals() {
        // exp(iπ) = -1（数值）
        let z = ComplexNum::new(
            Number::from_int(0),
            Number::Approx(BigFloat::pi(bigfloat::precision())),
        );
        let e = z.exp().unwrap();
        assert!(
            display::format_lineio(&e.re).starts_with("-1"),
            "{}",
            display::format_lineio(&e.re)
        );
        assert!(
            display::format_lineio(&e.im).starts_with("0"),
            "{}",
            display::format_lineio(&e.im)
        );
        // ln(-1) = iπ
        let l = c(-1, 0).ln().unwrap();
        assert!(
            display::format_lineio(&l.re).starts_with("0"),
            "{}",
            display::format_lineio(&l.re)
        );
        assert!(
            display::format_lineio(&l.im).starts_with("3.14159"),
            "{}",
            display::format_lineio(&l.im)
        );
        // sin(i) = i·sinh(1) ≈ 1.1752011936438014569 i
        let s = c(0, 1).sin().unwrap();
        assert!(
            display::format_lineio(&s.re).starts_with("0"),
            "{}",
            display::format_lineio(&s.re)
        );
        assert!(
            display::format_lineio(&s.im).starts_with("1.175201193643801456"),
            "{}",
            display::format_lineio(&s.im)
        );
        // ln(0) 报错
        assert!(c(0, 0).ln().is_err());
        // arg(i) = π/2
        assert!(display::format_lineio(&c(0, 1).arg().unwrap()).starts_with("1.57079"));
        // atan2 的象限：arg(-1+i) = 3π/4
        assert!(display::format_lineio(&c(-1, 1).arg().unwrap()).starts_with("2.35619"));
    }
}
