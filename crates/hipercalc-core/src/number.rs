use num_bigint::BigInt;
use num_integer::Integer;
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive, Zero};

use crate::bigfloat::{self, BigFloat};

/// 精确表达式的项
#[derive(Debug, Clone)]
pub enum ExactTerm {
    /// 有理数
    Rational(BigRational),
    /// 系数 * sqrt(被开方数), 如 3*sqrt(2)
    Sqrt(BigRational, BigInt),
    /// 系数 * pi
    Pi(BigRational),
    /// 系数 * e
    E(BigRational),
    /// 系数 * π^k（k 是有理数），如 Γ(1/2) = π^(1/2)、ζ(2) 的闭式里的 π²
    ///
    /// 为什么需要它：项目的精确表示只认「有理数 × √整数」，而 π 的幂不属于这种形式。
    /// k=1/2 对应 √π（伽马函数的半整数点），k=2、4、6… 对应 ζ 的偶数点闭式。
    /// 指数取有理数：半次幂是实际用得到的最小粒度。
    /// 表示不了的组合（如 π^π）仍落到既有的「先精确后数值回退」。
    /// 注：`Pi` 保留为 k=1 的特例（`pi` 这个常量走它，改动面最小）。
    PiPow(BigRational, BigRational),
}

/// 精确表达式（各项之和）/ 分母
#[derive(Debug, Clone)]
pub struct ExactExpr {
    pub terms: Vec<ExactTerm>,
    /// 分母必须为正整数
    pub denominator: BigInt,
}

/// 数值的内部表示
#[derive(Debug, Clone)]
pub enum Number {
    /// 符号精确值
    Exact(ExactExpr),
    /// 高精度近似值
    Approx(BigFloat),
    /// 复数 `re + im·i`（实部虚部各自是 `Exact` 或 `Approx`，因此精确/近似统一处理）
    Complex(Box<crate::complex::ComplexNum>),
}

impl Number {
    /// 是否为复数（实部虚部都在 Complex 变体里）
    pub fn is_complex(&self) -> bool {
        matches!(self, Number::Complex(_))
    }

    /// 把实数提升为复数（虚部 0）
    pub fn to_complex(&self) -> crate::complex::ComplexNum {
        match self {
            Number::Complex(z) => (**z).clone(),
            _ => crate::complex::ComplexNum::new(self.clone(), Number::from_int(0)),
        }
    }

    /// 用复数构造（虚部为 0 时自动退化回实数，保证显示与判定一致）
    pub fn from_complex(z: crate::complex::ComplexNum) -> Number {
        if z.im.is_zero() {
            z.re
        } else {
            Number::Complex(Box::new(z))
        }
    }

    /// 从整数创建
    pub fn from_int(n: i64) -> Self {
        Number::Exact(ExactExpr {
            terms: vec![ExactTerm::Rational(BigRational::from_integer(
                BigInt::from(n),
            ))],
            denominator: BigInt::one(),
        })
    }

    /// 从 BigInt 创建
    pub fn from_bigint(n: BigInt) -> Self {
        Number::Exact(ExactExpr {
            terms: vec![ExactTerm::Rational(BigRational::from_integer(n))],
            denominator: BigInt::one(),
        })
    }

    /// 从有理数创建精确值
    pub fn from_rational(r: BigRational) -> Self {
        Number::Exact(ExactExpr {
            terms: vec![ExactTerm::Rational(r)],
            denominator: BigInt::one(),
        })
    }

    /// 创建 pi 的有理数倍
    pub fn from_pi_times(r: BigRational) -> Self {
        if r.is_zero() {
            return Number::from_int(0);
        }
        Number::Exact(ExactExpr {
            terms: vec![ExactTerm::Pi(r)],
            denominator: BigInt::one(),
        })
    }

    /// 创建 e 的有理数倍
    pub fn from_e_times(r: BigRational) -> Self {
        if r.is_zero() {
            return Number::from_int(0);
        }
        Number::Exact(ExactExpr {
            terms: vec![ExactTerm::E(r)],
            denominator: BigInt::one(),
        })
    }

    /// 转换为 BigFloat 近似值
    pub fn to_approx(&self) -> BigFloat {
        match self {
            Number::Approx(f) => f.clone(),
            Number::Exact(expr) => expr.to_bigfloat(),
            // 复数没有单一实数值：调用方（parser）必须先用 is_complex() 分流。
            // 这里取实部近似并在 debug 下断言，避免静默丢掉虚部。
            Number::Complex(z) => {
                debug_assert!(false, "复数值应先用 is_complex() 分流，不能直接取近似");
                z.re.to_approx()
            }
        }
    }

    /// 判断是否为零
    pub fn is_zero(&self) -> bool {
        match self {
            Number::Exact(expr) => expr.is_zero(),
            Number::Approx(f) => f.is_zero(),
            Number::Complex(z) => z.is_zero(),
        }
    }

    /// 判断是否为负
    pub fn is_negative(&self) -> bool {
        match self {
            // 复数没有全序，不参与"负数"判定（开方/取模等都由 parser 先行分流）
            Number::Complex(_) => false,
            Number::Exact(expr) => {
                if expr.is_zero() {
                    return false;
                }
                let bf = expr.to_bigfloat();
                bf.value.is_negative()
            }
            Number::Approx(f) => f.value.is_negative(),
        }
    }

    /// 取相反数
    pub fn neg(&self) -> Number {
        match self {
            Number::Exact(expr) => Number::Exact(expr.neg()),
            Number::Approx(f) => Number::Approx(f.neg()),
            Number::Complex(z) => Number::Complex(Box::new(z.neg())),
        }
    }

    /// 取绝对值（复数取模 |z|）
    pub fn abs(&self) -> Number {
        if let Number::Complex(z) = self {
            return z.abs();
        }
        if self.is_negative() {
            self.neg()
        } else {
            self.clone()
        }
    }

    /// 向下取整：≤ x 的最大整数
    pub fn floor(&self) -> Number {
        if self.is_complex() {
            debug_assert!(false, "复数不支持 floor（parser 已先行拦截）");
            return self.clone();
        }
        match self {
            Number::Exact(e) => {
                if let Some(r) = e.as_rational() {
                    return Number::from_bigint(rational_floor(&r));
                }
                Number::from_bigint(bigfloat_floor(&self.to_approx()))
            }
            Number::Approx(b) => Number::from_bigint(bigfloat_floor(b)),
            Number::Complex(_) => unreachable!("floor 已在入口拦截复数"),
        }
    }

    /// 向上取整：≥ x 的最小整数
    pub fn ceil(&self) -> Number {
        if self.is_complex() {
            debug_assert!(false, "复数不支持 ceil（parser 已先行拦截）");
            return self.clone();
        }
        match self {
            Number::Exact(e) => {
                if let Some(r) = e.as_rational() {
                    return Number::from_bigint(rational_ceil(&r));
                }
                Number::from_bigint(bigfloat_ceil(&self.to_approx()))
            }
            Number::Approx(b) => Number::from_bigint(bigfloat_ceil(b)),
            Number::Complex(_) => unreachable!("ceil 已在入口拦截复数"),
        }
    }

    /// 四舍五入到最近整数（.5 远离零进位）
    pub fn round(&self) -> Number {
        if self.is_complex() {
            debug_assert!(false, "复数不支持 round（parser 已先行拦截）");
            return self.clone();
        }
        Number::from_bigint(bigfloat_round(&self.to_approx()))
    }

    /// 小数部分：x - floor(x)，取值范围 [0, 1)
    pub fn frac(&self) -> Number {
        let fl = self.floor();
        self.sub(&fl)
    }

    /// 符号函数：负数 → -1，零 → 0，正数 → 1
    pub fn sign(&self) -> Number {
        if self.is_zero() {
            Number::from_int(0)
        } else if self.is_negative() {
            Number::from_int(-1)
        } else {
            Number::from_int(1)
        }
    }

    /// 有理数检测（用于特殊角度匹配）
    pub fn as_rational(&self) -> Option<BigRational> {
        match self {
            Number::Exact(expr) => expr.as_rational(),
            Number::Approx(_) => None,
            Number::Complex(_) => None,
        }
    }

    /// 检测是否为 pi 的有理数倍（用于弧度模式 trig）
    pub fn as_pi_multiple(&self) -> Option<BigRational> {
        match self {
            Number::Exact(expr) => expr.as_pi_multiple(),
            Number::Approx(_) => None,
            Number::Complex(_) => None,
        }
    }

    /// 加法
    pub fn add(&self, other: &Number) -> Number {
        if self.is_complex() || other.is_complex() {
            let (a, b) = (self.to_complex(), other.to_complex());
            return Number::from_complex(a.add(&b));
        }
        match (self, other) {
            (Number::Exact(a), Number::Exact(b)) => match a.add_exact(b) {
                Some(result) => Number::Exact(result),
                None => Number::Approx(BigFloat::add(
                    &a.to_bigfloat(),
                    &b.to_bigfloat(),
                    bigfloat::precision(),
                )),
            },
            _ => {
                let a = self.to_approx();
                let b = other.to_approx();
                Number::Approx(BigFloat::add(&a, &b, bigfloat::precision()))
            }
        }
    }

    /// 减法
    pub fn sub(&self, other: &Number) -> Number {
        self.add(&other.neg())
    }

    /// 乘法
    pub fn mul(&self, other: &Number) -> Number {
        if self.is_complex() || other.is_complex() {
            let (a, b) = (self.to_complex(), other.to_complex());
            return Number::from_complex(a.mul(&b));
        }
        match (self, other) {
            (Number::Exact(a), Number::Exact(b)) => match a.mul_exact(b) {
                Some(result) => Number::Exact(result),
                None => Number::Approx(BigFloat::mul(
                    &a.to_bigfloat(),
                    &b.to_bigfloat(),
                    bigfloat::precision(),
                )),
            },
            _ => {
                let a = self.to_approx();
                let b = other.to_approx();
                Number::Approx(BigFloat::mul(&a, &b, bigfloat::precision()))
            }
        }
    }

    /// 除法
    pub fn div(&self, other: &Number) -> Number {
        if self.is_complex() || other.is_complex() {
            let (a, b) = (self.to_complex(), other.to_complex());
            return match a.div(&b) {
                Ok(z) => Number::from_complex(z),
                Err(_) => {
                    debug_assert!(false, "复数除法：除数为零，调用方应先校验 is_zero()");
                    Number::Approx(BigFloat::from_u64(0))
                }
            };
        }
        if other.is_zero() {
            // 除数为零：REPL 路径已由 parser 拦截（`BinOp::Div` 先判 is_zero），
            // 这里只做兜底。不再调用 BigFloat::div —— 其中 `assert!(!b.is_zero())`
            // 会让整个进程 panic；改为断言 + 返回 0，便于在 debug 构建下暴露调用点。
            debug_assert!(false, "Number::div: 除数为零，调用方应先校验 is_zero()");
            return Number::Approx(BigFloat::from_u64(0));
        }
        match (self, other) {
            (Number::Exact(a), Number::Exact(b)) => match a.div_exact(b) {
                Some(result) => Number::Exact(result),
                None => Number::Approx(BigFloat::div(
                    &a.to_bigfloat(),
                    &b.to_bigfloat(),
                    bigfloat::precision(),
                )),
            },
            _ => {
                let a = self.to_approx();
                let b = other.to_approx();
                Number::Approx(BigFloat::div(&a, &b, bigfloat::precision()))
            }
        }
    }

    /// 幂运算规模保护：估算 |self|^exp_abs 的十进制位数，超限直接报错。
    /// 避免 `2^1000000000` 这类输入把内存/耗时拖爆。死算模式 (/mode deep) 下不设上限。
    fn check_pow_scale(&self, exp_abs: f64) -> Result<(), String> {
        if crate::calc_mode::is_deep() {
            return Ok(());
        }
        // 底数为 0 时结果恒为 0（正指数），无需规模保护；
        // 否则 magnitude_log10() 对零返回 -inf，估算值会变成 inf 而误报"约有 inf 位十进制"
        // （旧实现下 `0^2`、`0^3` 这类最普通的输入都会报错）
        if self.is_zero() {
            return Ok(());
        }
        let digits = exp_abs * self.to_approx().magnitude_log10().abs();
        if digits > bigfloat::MAX_RESULT_DIGITS {
            return Err(format!(
                "幂运算结果约有 {:.0} 位十进制，超过支持上限（约 10^{:.0} 位）",
                digits,
                bigfloat::MAX_RESULT_DIGITS.log10()
            ));
        }
        Ok(())
    }

    /// 幂运算
    pub fn pow(&self, exponent: &Number) -> Result<Number, String> {
        // 复数底数或指数：只支持整数指数（快速幂，精确）；非整数指数暂不支持
        if self.is_complex() || exponent.is_complex() {
            let Some(exp_r) = exponent.as_rational() else {
                return Err("复数的非整数次幂暂不支持".to_string());
            };
            if !exp_r.is_integer() {
                return Err("复数的非整数次幂暂不支持".to_string());
            }
            let n = exp_r.to_integer();
            let neg = n.is_negative();
            let k = n
                .abs()
                .to_u32()
                .ok_or_else(|| "复数的幂指数过大".to_string())?;
            let z = self.to_complex().int_pow(k)?;
            if neg {
                if z.is_zero() {
                    return Err("0 的负次幂未定义".to_string());
                }
                let one = crate::complex::ComplexNum::from_int_pair(1, 0);
                return Ok(Number::from_complex(one.div(&z)?));
            }
            return Ok(Number::from_complex(z));
        }

        // 尝试精确计算
        if let (Number::Exact(_base), Number::Exact(exp)) = (self, exponent) {
            // 检查指数是否为有理数
            if let Some(exp_r) = exp.as_rational() {
                // 指数为整数
                if exp_r.is_integer() {
                    let exp_int = exp_r.to_integer();
                    if let Some(exp_u32) = exp_int.to_u32() {
                        self.check_pow_scale(exp_u32 as f64)?;
                        return self.int_pow(exp_u32);
                    }
                    if exp_int.is_negative() {
                        // 0 的负次幂未定义
                        if self.is_zero() {
                            return Err("0 的负次幂未定义".to_string());
                        }
                        if let Some(pos_exp) = (-exp_int).to_u32() {
                            self.check_pow_scale(pos_exp as f64)?;
                            let pos_pow = self.int_pow(pos_exp)?;
                            return Ok(Number::from_int(1).div(&pos_pow));
                        }
                    }
                }

                // 指数为 ±1/2（平方根/倒数平方根）
                if exp_r.numer().abs() == BigInt::from(1) && *exp_r.denom() == BigInt::from(2) {
                    if self.is_negative() {
                        return Err("负数的平方根在实数范围内无定义".to_string());
                    }
                    if exp_r.is_negative() {
                        if self.is_zero() {
                            return Err("0 的负次幂未定义".to_string());
                        }
                        let sqrt_val = self.sqrt();
                        return Ok(Number::from_int(1).div(&sqrt_val));
                    }
                    return Ok(self.sqrt());
                }
            }
        }

        // 负数底数的非整数次幂：实数域需要特殊处理
        if self.is_negative() {
            if let Some(exp_r) = exponent.as_rational() {
                // 最简分数 p/q：q 为奇数时 (-x)^(p/q) 在实数域有定义
                if !exp_r.is_integer() && exp_r.denom().is_odd() {
                    let abs_pow = self.abs().pow(exponent)?;
                    return Ok(if exp_r.numer().is_even() {
                        abs_pow
                    } else {
                        abs_pow.neg()
                    });
                }
                if !exp_r.is_integer() {
                    // 分母为偶数（如 (-4)^(1/2)）
                    return Err("负数的非整数次幂在实数范围内无定义".to_string());
                }
                // 整数指数（Approx 底数情形）继续走数值幂，BigFloat 支持负底整数幂
            } else {
                // 无理数指数，如 (-2)^pi
                return Err("负数的非整数次幂在实数范围内无定义".to_string());
            }
        }

        // 半整数指数（p/2）可直接走 sqrt 组合，避免通用 ln/exp 慢路径
        if let Some(exp_r) = exponent.as_rational() {
            if *exp_r.denom() == BigInt::from(2) && exp_r.numer().abs() <= BigInt::from(100) {
                if self.is_negative() {
                    return Err("负数的非整数次幂在实数范围内无定义".to_string());
                }
                let p = exp_r.numer();
                if p.is_zero() {
                    return Ok(Number::from_int(1));
                }
                // 规模保护：结果为 |self|^(±|p|/2)，十进制位数 ≈ (|p|/2)·log10|self|。
                // 其他整数指数分支都会先过 check_pow_scale，唯独这个半整数分支漏了，
                // 于是 (10^100000)^(99/2) 会真的去构造 10^4950000（约 500 万位）。
                let exp_abs = p.abs().to_f64().unwrap_or(0.0) / 2.0;
                self.check_pow_scale(exp_abs)?;
                let s = self.sqrt();
                let mut acc = Number::from_int(1);
                let mut base_pow = s.clone();
                let mut k = p.abs();
                while !k.is_zero() {
                    if k.is_odd() {
                        acc = acc.mul(&base_pow);
                    }
                    k /= 2u32;
                    if !k.is_zero() {
                        base_pow = base_pow.mul(&base_pow);
                    }
                }
                if p.is_negative() {
                    if acc.is_zero() {
                        return Err("0 的负次幂未定义".to_string());
                    }
                    acc = Number::from_int(1).div(&acc);
                }
                return Ok(acc);
            }
        }

        // 回退到数值计算
        let base = self.to_approx();
        let exp = exponent.to_approx();
        Ok(Number::Approx(BigFloat::pow(
            &base,
            &exp,
            bigfloat::precision(),
        )?))
    }

    /// 整数次幂
    fn int_pow(&self, exp: u32) -> Result<Number, String> {
        if exp == 0 {
            return Ok(Number::from_int(1));
        }
        if exp == 1 {
            return Ok(self.clone());
        }

        match self {
            Number::Complex(z) => Ok(Number::Complex(Box::new(z.int_pow(exp)?))),
            Number::Exact(expr) => {
                if let Some(int_val) = expr.as_integer() {
                    return Ok(Number::from_bigint(int_val.pow(exp)));
                }
                // 展开 sqrt：a 必须按**有理数**参与运算
                //   n = 2m 偶：(a√b)^(2m) = a^(2m)·b^m
                //   n = 2m+1 奇：(a√b)^(2m+1) = a^(2m+1)·b^m·√b
                // 旧实现把 a 用 numer/denom 整数截断（1/2 → 0），
                // 且偶数次幂少乘一次 a（(2√3)^2 得 6 而非 12），均已修正。
                if expr.terms.len() == 1 && expr.denominator == BigInt::one() {
                    if let ExactTerm::Sqrt(coeff, rad) = &expr.terms[0] {
                        let half_exp = if exp % 2 == 0 { exp / 2 } else { (exp - 1) / 2 };
                        let new_coeff =
                            coeff.pow(exp as i32) * BigRational::from_integer(rad.pow(half_exp));

                        if exp % 2 == 0 {
                            return Ok(Number::from_rational(new_coeff));
                        }
                        return Ok(Number::Exact(ExactExpr {
                            terms: vec![ExactTerm::Sqrt(new_coeff, rad.clone())],
                            denominator: BigInt::one(),
                        }));
                    }
                }
                // 回退
                Ok(Number::Approx(BigFloat::pow(
                    &expr.to_bigfloat(),
                    &BigFloat::from_u64(exp as u64),
                    bigfloat::precision(),
                )?))
            }
            Number::Approx(f) => Ok(Number::Approx(BigFloat::pow(
                f,
                &BigFloat::from_u64(exp as u64),
                bigfloat::precision(),
            )?)),
        }
    }

    /// 黄金比 φ = (1+√5)/2，用精确形式表示（`1/2 + (1/2)√5`）。
    /// 走现成的 add/mul/sqrt 管线，保证与其它精确运算同一套归一化规则。
    pub fn phi() -> Number {
        let half = Number::from_rational(BigRational::new(BigInt::one(), BigInt::from(2u32)));
        let sqrt5 = Number::from_int(5).sqrt();
        half.add(&half.mul(&sqrt5))
    }

    /// k 次根（k ≥ 2；**调用方保证非负**，负号由调用方处理）：
    /// 完全 k 次幂给精确有理数，否则数值（`BigFloat::nroot` 的牛顿迭代）。
    pub fn nth_root(&self, k: u32) -> Result<Number, String> {
        if let Some(r) = self.as_rational() {
            let num = r.numer().abs();
            let den = r.denom().abs();
            let rn = crate::bigint_ext::int_nth_root(&num, k);
            let rd = crate::bigint_ext::int_nth_root(&den, k);
            if rn.clone().pow(k) == num && rd.clone().pow(k) == den {
                return Ok(Number::from_rational(BigRational::new(rn, rd)));
            }
        }
        let b = self.to_approx();
        Ok(Number::Approx(b.nroot(k, bigfloat::precision())?))
    }

    /// 立方根（等价 `nth_root(3)`）
    pub fn cbrt(&self) -> Result<Number, String> {
        self.nth_root(3)
    }

    /// Euclidean 整除（对应 Rust 的 `div_euclid`）：满足 `a = b·q + r`、`0 ≤ r < |b|`。
    ///
    /// 注意 `b < 0` 时它**不等于** `floor(a/b)`：如 `idiv(7,-3) = -2`（`floor(-7/3) = -3` 会让余数变负）。
    /// 与 `bigint_ext::div` 的截断除法也不同（那里余数符号随被除数）。要求精确有理数、`b ≠ 0`。
    pub fn idiv(&self, b: &Number) -> Result<Number, String> {
        let (ra, rb) = match (self.as_rational(), b.as_rational()) {
            (Some(a), Some(b)) => (a, b),
            _ => return Err("取模与整除需要精确数值".to_string()),
        };
        if rb.is_zero() {
            return Err("取模与整除的除数不能为 0".to_string());
        }
        let r = euclid_rem(&ra, &rb);
        Ok(Number::from_rational((&ra - &r) / &rb))
    }

    /// Euclidean 取模（对应 Rust 的 `rem_euclid`）：结果恒满足 `0 ≤ r < |b|`
    pub fn modulo(&self, b: &Number) -> Result<Number, String> {
        let (ra, rb) = match (self.as_rational(), b.as_rational()) {
            (Some(a), Some(b)) => (a, b),
            _ => return Err("取模与整除需要精确数值".to_string()),
        };
        if rb.is_zero() {
            return Err("取模与整除的除数不能为 0".to_string());
        }
        Ok(Number::from_rational(euclid_rem(&ra, &rb)))
    }

    /// 平方根
    pub fn sqrt(&self) -> Number {
        if let Number::Complex(z) = self {
            // 复数的平方根（parser 的 sqr/sqrt 分支也会直接走这条路）
            return match z.sqrt() {
                Ok(root) => Number::from_complex(root),
                Err(e) => {
                    debug_assert!(false, "复数开方失败: {e}");
                    self.clone()
                }
            };
        }
        match self {
            Number::Exact(expr) => {
                // 检查是否为完全平方数
                if let Some(int_val) = expr.as_integer() {
                    if int_val >= BigInt::zero() {
                        // 用 bigint_ext 的整数平方根：大数时绕开 num-bigint 的 BZ 除法断言缺陷
                        // （实测 sqr(10^100000) 会在 num-bigint 内部 panic）
                        let root = crate::bigint_ext::int_sqrt(&int_val);
                        if &root * &root == int_val {
                            return Number::from_bigint(root);
                        }
                        // 简化 sqrt(n) = sqrt(简化)
                        let (coeff, rad) =
                            simplify_radical(BigRational::from_integer(BigInt::one()), int_val);
                        return Number::Exact(ExactExpr {
                            terms: vec![ExactTerm::Sqrt(coeff, rad)],
                            denominator: BigInt::one(),
                        });
                    }
                }

                // 检查有理数（负数直接跳过：实数域无平方根，交给数值路径按 BigFloat 的断言报错，
                // 不能像旧实现那样对分子取绝对值后静默返回正根）
                if let Some(rat) = expr.as_rational().filter(|r| !r.is_negative()) {
                    let num = rat.numer().abs();
                    let den = rat.denom().abs();
                    let num_root = crate::bigint_ext::int_sqrt(&num);
                    let den_root = crate::bigint_ext::int_sqrt(&den);
                    if &num_root * &num_root == num && &den_root * &den_root == den {
                        let sqrt_val = BigRational::new(num_root, den_root);
                        return Number::from_rational(sqrt_val);
                    }
                    // 非完全平方：sqrt(p/q) = sqrt(p·q)/q，先化简 sqrt(p·q) 再把 q 放进系数，
                    // 即可得到精确根式（旧实现算出结果后直接丢弃、退化为数值近似）
                    let prod = &num * &den;
                    let (c, r) = simplify_radical(BigRational::one(), prod);
                    let coeff = c / BigRational::from_integer(den);
                    if r == BigInt::one() {
                        return Number::from_rational(coeff);
                    }
                    return Number::Exact(ExactExpr {
                        terms: vec![ExactTerm::Sqrt(coeff, r)],
                        denominator: BigInt::one(),
                    });
                }

                // 回退到数值
                Number::Approx(expr.to_bigfloat().sqrt(bigfloat::precision()))
            }
            Number::Approx(f) => Number::Approx(f.sqrt(bigfloat::precision())),
            Number::Complex(_) => unreachable!("sqrt 已在入口处理复数"),
        }
    }
}

impl ExactExpr {
    /// 转换为 BigFloat
    pub fn to_bigfloat(&self) -> BigFloat {
        let mut result = BigFloat::from_u64(0);

        for term in &self.terms {
            let term_float = match term {
                ExactTerm::Rational(r) => BigFloat::from_big_rational(r),
                ExactTerm::Sqrt(coeff, rad) => {
                    let c = BigFloat::from_big_rational(coeff);
                    let r = BigFloat::from_int(rad).sqrt(bigfloat::precision());
                    BigFloat::mul(&c, &r, bigfloat::precision())
                }
                ExactTerm::Pi(coeff) => {
                    let c = BigFloat::from_big_rational(coeff);
                    let pi = BigFloat::pi(bigfloat::precision());
                    BigFloat::mul(&c, &pi, bigfloat::precision())
                }
                ExactTerm::E(coeff) => {
                    let c = BigFloat::from_big_rational(coeff);
                    let e = BigFloat::e(bigfloat::precision());
                    BigFloat::mul(&c, &e, bigfloat::precision())
                }
                ExactTerm::PiPow(coeff, k) => {
                    // c · π^k：k 拆成「整数部分 + 0/半」，整数部分连乘、半次开方
                    let pr = bigfloat::precision();
                    let c = BigFloat::from_big_rational(coeff);
                    let pi = BigFloat::pi(pr);
                    let int_part = k.numer() / k.denom();
                    let mut pw = BigFloat::from_u64(1);
                    let mut i = BigInt::zero();
                    while i < int_part {
                        pw = BigFloat::mul(&pw, &pi, pr);
                        i += BigInt::one();
                    }
                    if k.denom() != &BigInt::one() {
                        pw = pw.sqrt(pr);
                    }
                    BigFloat::mul(&c, &pw, pr)
                }
            };
            result = BigFloat::add(&result, &term_float, bigfloat::precision());
        }

        if self.denominator != BigInt::one() {
            let den = BigFloat::from_int(&self.denominator);
            result = BigFloat::div(&result, &den, bigfloat::precision());
        }

        result
    }

    /// 判断是否为零
    pub fn is_zero(&self) -> bool {
        self.terms.is_empty()
            || self
                .terms
                .iter()
                .all(|t| matches!(t, ExactTerm::Rational(r) if r.is_zero()))
    }

    /// 取反
    pub fn neg(&self) -> ExactExpr {
        ExactExpr {
            terms: self
                .terms
                .iter()
                .map(|t| match t {
                    ExactTerm::Rational(r) => ExactTerm::Rational(-r),
                    ExactTerm::Sqrt(c, r) => ExactTerm::Sqrt(-c, r.clone()),
                    ExactTerm::PiPow(c, k) => ExactTerm::PiPow(-c, k.clone()),
                    ExactTerm::Pi(c) => ExactTerm::Pi(-c),
                    ExactTerm::E(c) => ExactTerm::E(-c),
                })
                .collect(),
            denominator: self.denominator.clone(),
        }
    }

    /// 尝试提取整数
    pub fn as_integer(&self) -> Option<BigInt> {
        if self.denominator != BigInt::one() {
            return None;
        }
        if self.terms.len() != 1 {
            return None;
        }
        match &self.terms[0] {
            ExactTerm::Rational(r) => {
                if r.is_integer() {
                    Some(r.to_integer())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// 尝试提取有理数
    pub fn as_rational(&self) -> Option<BigRational> {
        if self.terms.len() != 1 {
            return None;
        }
        if self.denominator != BigInt::one() {
            return None;
        }
        match &self.terms[0] {
            ExactTerm::Rational(r) => Some(r.clone()),
            _ => None,
        }
    }

    /// 检测是否为 pi 的有理倍
    pub fn as_pi_multiple(&self) -> Option<BigRational> {
        if self.terms.len() != 1 {
            return None;
        }
        if self.denominator != BigInt::one() {
            return None;
        }
        match &self.terms[0] {
            ExactTerm::Pi(r) => Some(r.clone()),
            _ => None,
        }
    }

    /// 精确加法（可能失败，返回 None 表示需要回退到数值）
    fn add_exact(&self, other: &ExactExpr) -> Option<ExactExpr> {
        // 通分
        let common_denom = self.denominator.lcm(&other.denominator);
        let self_factor = &common_denom / &self.denominator;
        let other_factor = &common_denom / &other.denominator;

        let mut terms: Vec<ExactTerm> = Vec::new();

        // 添加 self * self_factor 的项
        for t in &self.terms {
            let scaled = scale_term(t, &self_factor);
            terms.push(scaled);
        }

        // 添加 other * other_factor 的项
        for t in &other.terms {
            let scaled = scale_term(t, &other_factor);
            // 合并同类项
            merge_term(&mut terms, scaled);
        }

        // 移除零项
        terms.retain(|t| !is_term_zero(t));

        let mut result = ExactExpr {
            terms,
            denominator: common_denom,
        };

        // 归一化（Sqrt→Rational 转换）并约分
        normalize_terms(&mut result);
        simplify_expr(&mut result);

        Some(result)
    }

    /// 精确乘法
    fn mul_exact(&self, other: &ExactExpr) -> Option<ExactExpr> {
        let mut result_terms: Vec<ExactTerm> = Vec::new();

        for t1 in &self.terms {
            for t2 in &other.terms {
                let product = mul_terms(t1, t2)?;
                merge_term(&mut result_terms, product);
            }
        }

        let result_denom = &self.denominator * &other.denominator;

        // 移除零项（系数为零的项）
        result_terms.retain(|t| !is_term_zero(t));

        let mut result = ExactExpr {
            terms: result_terms,
            denominator: result_denom,
        };
        // 将 Sqrt(coeff, 1) 转换为 Rational(coeff)
        normalize_terms(&mut result);
        simplify_expr(&mut result);
        Some(result)
    }

    /// 精确除法
    fn div_exact(&self, other: &ExactExpr) -> Option<ExactExpr> {
        // a / b 转化为 a * (1/b)
        // 1/b 的分母有理化

        // 如果 other 有分母 ≠ 1，先提取：self / (num/d) = (self * d) / num
        if other.denominator != BigInt::one() {
            let other_num = ExactExpr {
                terms: other.terms.clone(),
                denominator: BigInt::one(),
            };
            // self * d
            let self_scaled = ExactExpr {
                terms: self
                    .terms
                    .iter()
                    .map(|t| match t {
                        ExactTerm::Rational(r) => ExactTerm::Rational(
                            r * &BigRational::from_integer(other.denominator.clone()),
                        ),
                        ExactTerm::Sqrt(c, rad) => ExactTerm::Sqrt(
                            c * &BigRational::from_integer(other.denominator.clone()),
                            rad.clone(),
                        ),
                        ExactTerm::PiPow(c, k) => ExactTerm::PiPow(
                            c * &BigRational::from_integer(other.denominator.clone()),
                            k.clone(),
                        ),
                        ExactTerm::Pi(c) => {
                            ExactTerm::Pi(c * &BigRational::from_integer(other.denominator.clone()))
                        }
                        ExactTerm::E(c) => {
                            ExactTerm::E(c * &BigRational::from_integer(other.denominator.clone()))
                        }
                    })
                    .collect(),
                denominator: self.denominator.clone(),
            };
            return self_scaled.div_exact(&other_num);
        }

        // 如果 other 是有理数，直接乘以倒数
        if let Some(rat) = other.as_rational() {
            let recip = rat.recip();
            // self * recip
            let mut terms: Vec<ExactTerm> = Vec::new();
            for t in &self.terms {
                let scaled = match t {
                    ExactTerm::Rational(r) => ExactTerm::Rational(r * &recip),
                    ExactTerm::Sqrt(c, rad) => ExactTerm::Sqrt(c * &recip, rad.clone()),
                    ExactTerm::PiPow(c, k) => ExactTerm::PiPow(c * &recip, k.clone()),
                    ExactTerm::Pi(c) => ExactTerm::Pi(c * &recip),
                    ExactTerm::E(c) => ExactTerm::E(c * &recip),
                };
                merge_term(&mut terms, scaled);
            }
            terms.retain(|t| !is_term_zero(t));
            let mut result = ExactExpr {
                terms,
                denominator: self.denominator.clone(),
            };
            simplify_expr(&mut result);
            return Some(result);
        }

        // 如果 other 是单个 sqrt 项（分母有理化）
        if other.terms.len() == 1 && other.denominator == BigInt::one() {
            if let ExactTerm::Sqrt(coeff, rad) = &other.terms[0] {
                // self / (coeff * sqrt(rad)) = self * sqrt(rad) * 1/(coeff * rad)
                // 注意：系数 coeff 必须按**有理数**参与（旧实现只取 coeff.numer()，
                // 丢掉分母后 1/(sqr(2)/2) 会少除以 2、1/(sqr(3)/3) 少除以 3）。
                // 1/(coeff*rad) 是有理因子，直接吸收进各项系数即可。
                let scale = BigRational::one() / (coeff * &BigRational::from_integer(rad.clone()));
                let mut new_terms: Vec<ExactTerm> = Vec::new();
                for t in &self.terms {
                    let product = mul_terms(t, &ExactTerm::Sqrt(BigRational::one(), rad.clone()))?;
                    merge_term(&mut new_terms, scale_term_rational(&product, &scale));
                }
                let mut result = ExactExpr {
                    terms: new_terms,
                    denominator: self.denominator.clone(),
                };
                simplify_expr(&mut result);
                return Some(result);
            }
        }

        // 如果 other 是 sqrt 项的线性组合，尝试分母有理化
        // (a + b*sqrt(c)) 的共轭是有理化因子
        if other.terms.len() <= 2 && other.denominator == BigInt::one() {
            let sqrt_terms: Vec<_> = other
                .terms
                .iter()
                .filter(|t| matches!(t, ExactTerm::Sqrt(_, _)))
                .collect();
            let rat_terms: Vec<_> = other
                .terms
                .iter()
                .filter(|t| matches!(t, ExactTerm::Rational(_)))
                .collect();

            if sqrt_terms.len() == 1 && rat_terms.len() <= 1 {
                // (a + b*sqrt(c)): 共轭 = (a - b*sqrt(c))
                let sqrt_term = &sqrt_terms[0];
                let rat_coeff = if rat_terms.is_empty() {
                    BigRational::zero()
                } else if let ExactTerm::Rational(r) = &rat_terms[0] {
                    r.clone()
                } else {
                    return None;
                };

                if let ExactTerm::Sqrt(sqrt_coeff, rad) = sqrt_term {
                    let mut conjugate = ExactExpr {
                        terms: vec![
                            ExactTerm::Rational(rat_coeff.clone()),
                            ExactTerm::Sqrt(-sqrt_coeff, rad.clone()),
                        ],
                        denominator: BigInt::one(),
                    };
                    // 移除零项
                    conjugate.terms.retain(|t| !is_term_zero(t));

                    // self * conjugate / (other * conjugate)
                    let num = self.mul_exact(&conjugate)?;
                    let den = other.mul_exact(&conjugate)?;
                    // den 应该有理化后为有理数
                    if let Some(den_rat) = den.as_rational() {
                        let recip = den_rat.recip();
                        let mut result_terms: Vec<ExactTerm> = Vec::new();
                        for t in &num.terms {
                            let scaled = match t {
                                ExactTerm::Rational(r) => ExactTerm::Rational(r * &recip),
                                ExactTerm::Sqrt(c, r) => ExactTerm::Sqrt(c * &recip, r.clone()),
                                ExactTerm::PiPow(c, k) => ExactTerm::PiPow(c * &recip, k.clone()),
                                ExactTerm::Pi(c) => ExactTerm::Pi(c * &recip),
                                ExactTerm::E(c) => ExactTerm::E(c * &recip),
                            };
                            merge_term(&mut result_terms, scaled);
                        }
                        result_terms.retain(|t| !is_term_zero(t));
                        let mut result = ExactExpr {
                            terms: result_terms,
                            denominator: num.denominator.clone(),
                        };
                        simplify_expr(&mut result);
                        return Some(result);
                    }
                }
            }
        }

        None
    }
}

fn scale_term(term: &ExactTerm, factor: &BigInt) -> ExactTerm {
    let f = BigRational::from_integer(factor.clone());
    match term {
        ExactTerm::Rational(r) => ExactTerm::Rational(r * &f),
        ExactTerm::Sqrt(c, rad) => ExactTerm::Sqrt(c * &f, rad.clone()),
        ExactTerm::PiPow(c, k) => ExactTerm::PiPow(c * &f, k.clone()),
        ExactTerm::Pi(c) => ExactTerm::Pi(c * &f),
        ExactTerm::E(c) => ExactTerm::E(c * &f),
    }
}

/// 把项的有理系数乘以 factor（项类型不变）。
/// 与 `scale_term` 的区别：factor 是**有理数**，乘进系数（含分母）而非表达式分母，
/// 因此不会像"分子单独缩放"那样丢失系数分母。
fn scale_term_rational(term: &ExactTerm, factor: &BigRational) -> ExactTerm {
    match term {
        ExactTerm::Rational(r) => ExactTerm::Rational(r * factor),
        ExactTerm::Sqrt(c, rad) => ExactTerm::Sqrt(c * factor, rad.clone()),
        ExactTerm::PiPow(c, k) => ExactTerm::PiPow(c * factor, k.clone()),
        ExactTerm::Pi(c) => ExactTerm::Pi(c * factor),
        ExactTerm::E(c) => ExactTerm::E(c * factor),
    }
}

fn merge_term(terms: &mut Vec<ExactTerm>, new_term: ExactTerm) {
    match &new_term {
        ExactTerm::Rational(new_r) => {
            for t in terms.iter_mut() {
                if let ExactTerm::Rational(existing) = t {
                    *existing = std::mem::replace(existing, BigRational::zero()) + new_r;
                    if existing.is_zero() {
                        // remove later
                    }
                    return;
                }
            }
            terms.push(new_term);
        }
        ExactTerm::Sqrt(new_c, new_rad) => {
            for t in terms.iter_mut() {
                if let ExactTerm::Sqrt(existing_c, existing_rad) = t {
                    if existing_rad == new_rad {
                        *existing_c = std::mem::replace(existing_c, BigRational::zero()) + new_c;
                        return;
                    }
                }
            }
            terms.push(new_term);
        }
        // 同底数的 π 幂**只在指数相同时**才能合并（π + π² 不是 2π）
        ExactTerm::PiPow(new_c, new_k) => {
            for t in terms.iter_mut() {
                if let ExactTerm::PiPow(existing_c, existing_k) = t {
                    if existing_k == new_k {
                        *existing_c = std::mem::replace(existing_c, BigRational::zero()) + new_c;
                        return;
                    }
                }
            }
            terms.push(new_term);
        }
        ExactTerm::Pi(new_c) => {
            for t in terms.iter_mut() {
                if let ExactTerm::Pi(existing_c) = t {
                    *existing_c = std::mem::replace(existing_c, BigRational::zero()) + new_c;
                    return;
                }
            }
            terms.push(new_term);
        }
        ExactTerm::E(new_c) => {
            for t in terms.iter_mut() {
                if let ExactTerm::E(existing_c) = t {
                    *existing_c = std::mem::replace(existing_c, BigRational::zero()) + new_c;
                    return;
                }
            }
            terms.push(new_term);
        }
    }
}

fn mul_terms(t1: &ExactTerm, t2: &ExactTerm) -> Option<ExactTerm> {
    match (t1, t2) {
        (ExactTerm::Rational(r1), ExactTerm::Rational(r2)) => Some(ExactTerm::Rational(r1 * r2)),
        (ExactTerm::Rational(r), ExactTerm::Sqrt(c, rad)) => {
            Some(ExactTerm::Sqrt(r * c, rad.clone()))
        }
        (ExactTerm::Sqrt(c, rad), ExactTerm::Rational(r)) => {
            Some(ExactTerm::Sqrt(c * r, rad.clone()))
        }
        (ExactTerm::Rational(r), ExactTerm::Pi(c)) => Some(ExactTerm::Pi(r * c)),
        (ExactTerm::Pi(c), ExactTerm::Rational(r)) => Some(ExactTerm::Pi(r * c)),
        (ExactTerm::Rational(r), ExactTerm::E(c)) => Some(ExactTerm::E(r * c)),
        (ExactTerm::E(c), ExactTerm::Rational(r)) => Some(ExactTerm::E(r * c)),
        (ExactTerm::Sqrt(c1, r1), ExactTerm::Sqrt(c2, r2)) => {
            // sqrt(a)*sqrt(b) = sqrt(a*b)
            let new_coeff = c1 * c2;
            let new_rad = r1 * r2;
            let (simplified_c, simplified_r) = simplify_radical(new_coeff, new_rad);
            Some(ExactTerm::Sqrt(simplified_c, simplified_r))
        }
        // π 的幂相乘：π^a · π^b = π^(a+b)（√π·√π = π 是 k=1/2 的特例）
        (ExactTerm::PiPow(c1, k1), ExactTerm::PiPow(c2, k2)) => {
            let k = k1 + k2;
            if k.is_one() {
                Some(ExactTerm::Pi(c1 * c2))
            } else {
                Some(ExactTerm::PiPow(c1 * c2, k))
            }
        }
        (ExactTerm::PiPow(c1, k1), ExactTerm::Pi(c2)) => {
            Some(ExactTerm::PiPow(c1 * c2, k1 + BigRational::one()))
        }
        (ExactTerm::Pi(c1), ExactTerm::PiPow(c2, k2)) => {
            Some(ExactTerm::PiPow(c1 * c2, k2 + BigRational::one()))
        }
        (ExactTerm::Rational(r), ExactTerm::PiPow(c, k)) => {
            Some(ExactTerm::PiPow(r * c, k.clone()))
        }
        (ExactTerm::PiPow(c, k), ExactTerm::Rational(r)) => {
            Some(ExactTerm::PiPow(c * r, k.clone()))
        }
        // 混合类型（如 sqrt * pi）不支持精确表示
        _ => None,
    }
}

fn is_term_zero(term: &ExactTerm) -> bool {
    match term {
        ExactTerm::Rational(r) => r.is_zero(),
        ExactTerm::Sqrt(c, _) => c.is_zero(),
        ExactTerm::PiPow(c, _) => c.is_zero(),
        ExactTerm::Pi(c) => c.is_zero(),
        ExactTerm::E(c) => c.is_zero(),
    }
}

/// Euclidean 余数：`a - |b|·floor(a/|b|)`，恒满足 `0 ≤ r < |b|`（b ≠ 0 由调用方保证）
fn euclid_rem(a: &BigRational, b: &BigRational) -> BigRational {
    let abs_b = b.abs();
    let q = (a / &abs_b).floor();
    a - &abs_b * q
}

/// 简化被开方数：移除平方因子
/// add * sqrt(radicand) = (add * sqrt(square_part)) * sqrt(reduced_rad)
fn simplify_radical(coeff: BigRational, radicand: BigInt) -> (BigRational, BigInt) {
    if radicand <= BigInt::one() {
        return (coeff, BigInt::one());
    }

    let mut n = radicand.clone();
    let mut outside = BigInt::one();

    // 试除上限：每次试除都要对当前 n 取模，单次代价 ≈ O(n 的 limb 数)。
    // 固定 1e6 次在大 n 上会白跑几十秒（实测 sqr(10^100000+1) 约 87 秒），
    // 因此按"总工作量约 2×10^6 次 limb 运算"折算 p 的上限：
    // 小整数仍可试到 1e6（原化简能力不变），大数只试小质数（2、3、5、7…几百以内）。
    // 死算模式 (/mode deep) 不设上限。
    let deep = crate::calc_mode::is_deep();
    let words = (n.bits() as usize).div_ceil(64).max(1);
    let max_p = (2_000_000usize / words).clamp(256, 1_000_000) as u64;

    let mut p: u64 = 2;
    loop {
        if !deep && p > max_p {
            break;
        }
        // p² 用 u64 比较（p ≤ 10^6 ⇒ p² ≤ 10^12 不溢出），避免每轮都做 BigInt 乘法+分配
        let p2_u64 = p * p;
        let too_big = match n.to_u64() {
            Some(nu) => p2_u64 > nu,
            None => BigInt::from(p2_u64) > n,
        };
        if too_big {
            break;
        }
        let p_big = BigInt::from(p);
        let p2 = BigInt::from(p2_u64);
        while &n % &p2 == BigInt::zero() {
            n /= &p2;
            outside *= &p_big;
        }
        p += 1;
    }

    (coeff * BigRational::from_integer(outside), n)
}

/// 归一化：将 Sqrt(coeff, 1) 转换为 Rational(coeff)，合并同类 Rational
fn normalize_terms(expr: &mut ExactExpr) {
    let mut new_terms: Vec<ExactTerm> = Vec::new();
    for term in &expr.terms {
        match term {
            ExactTerm::Sqrt(coeff, rad) if *rad == BigInt::one() => {
                // sqr(1) = 1，转换为有理数
                let r = BigRational::new(coeff.numer().clone(), coeff.denom().clone());
                merge_term(&mut new_terms, ExactTerm::Rational(r));
            }
            _ => {
                merge_term(&mut new_terms, term.clone());
            }
        }
    }
    new_terms.retain(|t| !is_term_zero(t));
    expr.terms = new_terms;
}

/// 约分化简
fn simplify_expr(expr: &mut ExactExpr) {
    // 先做归一化转换
    normalize_terms(expr);

    if expr.terms.is_empty() {
        expr.terms.push(ExactTerm::Rational(BigRational::zero()));
        expr.denominator = BigInt::one();
        return;
    }

    // 尝试提取公因子：只考虑系数分子与表达式分母的 GCD
    let mut nums: Vec<BigInt> = Vec::new();
    for t in &expr.terms {
        match t {
            ExactTerm::Rational(r) => {
                nums.push(r.numer().abs());
            }
            ExactTerm::Sqrt(c, _) => {
                nums.push(c.numer().abs());
            }
            ExactTerm::Pi(c) | ExactTerm::E(c) => {
                nums.push(c.numer().abs());
            }
            ExactTerm::PiPow(c, _) => {
                nums.push(c.numer().abs());
            }
        }
    }
    nums.push(expr.denominator.clone());

    if nums.is_empty() {
        return;
    }

    let mut gcd = nums[0].clone();
    for n in nums.iter().skip(1) {
        if !n.is_zero() {
            gcd = gcd.gcd(n);
        }
    }

    if gcd > BigInt::one() {
        for t in &mut expr.terms {
            match t {
                ExactTerm::Rational(r) => {
                    *r = BigRational::new(r.numer() / &gcd, r.denom().clone());
                }
                ExactTerm::Sqrt(c, _) => {
                    *c = BigRational::new(c.numer() / &gcd, c.denom().clone());
                }
                ExactTerm::Pi(c) => {
                    *c = BigRational::new(c.numer() / &gcd, c.denom().clone());
                }
                ExactTerm::PiPow(c, _) => {
                    *c = BigRational::new(c.numer() / &gcd, c.denom().clone());
                }
                ExactTerm::E(c) => {
                    *c = BigRational::new(c.numer() / &gcd, c.denom().clone());
                }
            }
        }
        expr.denominator = &expr.denominator / &gcd;
    }
}

/* ---------------- 取整辅助 ---------------- */

/// 有理数 p/q 向下取整
fn rational_floor(r: &BigRational) -> BigInt {
    let p = r.numer();
    let q = r.denom();
    let mut f = p / q;
    let rem = p % q;
    if rem.is_negative() {
        f -= 1;
    }
    f
}

/// 有理数 p/q 向上取整
fn rational_ceil(r: &BigRational) -> BigInt {
    let p = r.numer();
    let q = r.denom();
    let mut c = p / q;
    let rem = p % q;
    if rem.is_positive() {
        c += 1;
    }
    c
}

/// BigFloat 向下取整（按 value/10^precision 截断）
fn bigfloat_floor(b: &BigFloat) -> BigInt {
    let p10 = BigInt::from(10).pow(b.precision as u32);
    let mut f = &b.value / &p10;
    if (&b.value % &p10).is_negative() {
        f -= 1;
    }
    f
}

/// BigFloat 向上取整
fn bigfloat_ceil(b: &BigFloat) -> BigInt {
    let p10 = BigInt::from(10).pow(b.precision as u32);
    let mut c = &b.value / &p10;
    if (&b.value % &p10).is_positive() {
        c += 1;
    }
    c
}

/// BigFloat 四舍五入（.5 远离零进位）
fn bigfloat_round(b: &BigFloat) -> BigInt {
    let p10 = BigInt::from(10).pow(b.precision as u32);
    let half = &p10 / 2;
    let s: i64 = if b.value < BigInt::from(0) { -1 } else { 1 };
    let abs = b.value.abs();
    (abs + half) / p10 * s
}
