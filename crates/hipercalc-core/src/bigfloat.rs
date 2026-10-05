use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use num_bigint::{BigInt, Sign};
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};
use std::fmt;
use std::ops::{Add, Div, Mul, Neg, Sub};

/// 内部计算精度（小数位数）
pub const DEFAULT_PRECISION: usize = 80;
/// 运算额外余量
const MARGIN: usize = 20;
/// 输出时有效数字位数（lineio 模式）
pub const DEFAULT_DISPLAY_DIGITS: usize = 20;

/// 运行期工作精度（小数位数）：默认 80，可由 `/mode prec N` 修改。
/// 之所以做成运行期开关：精度相关的常数缓存都以目标精度为键，切换精度后自动按新精度重算并各自缓存。
static PRECISION_RT: AtomicUsize = AtomicUsize::new(DEFAULT_PRECISION);

/// 运行期显示有效位数：默认 20，可由 `/mode digits N` 修改
static DISPLAY_DIGITS_RT: AtomicUsize = AtomicUsize::new(DEFAULT_DISPLAY_DIGITS);

/// 当前工作精度（小数位数）
pub fn precision() -> usize {
    PRECISION_RT.load(Ordering::Relaxed)
}

/// 设置工作精度（小数位数）；调用方负责范围校验
pub fn set_precision(p: usize) {
    PRECISION_RT.store(p, Ordering::Relaxed);
}

/// 是否允许"整数部分超过显示位数"时改用科学计数法（`/mode sci off` 关闭，改为完整写出）
static SCI_ALLOWED: AtomicBool = AtomicBool::new(true);

/// 是否给整数部分加千分位分隔（`/mode group on`；仅显示层）
static GROUP_DIGITS: AtomicBool = AtomicBool::new(false);

/// 当前是否允许科学计数法
pub fn sci_allowed() -> bool {
    SCI_ALLOWED.load(Ordering::Relaxed)
}

/// 设置是否允许科学计数法
pub fn set_sci_allowed(on: bool) {
    SCI_ALLOWED.store(on, Ordering::Relaxed);
}

/// 当前是否输出千分位
pub fn group_enabled() -> bool {
    GROUP_DIGITS.load(Ordering::Relaxed)
}

/// 设置是否输出千分位
pub fn set_group_enabled(on: bool) {
    GROUP_DIGITS.store(on, Ordering::Relaxed);
}

/// 若字符串是纯十进制数（`[-]digits[.digits]`）且 `/mode group on`，给整数部分加千分位；
/// 其它形式（分数 `1 / 2`、根式、科学计数法、含单位的串）原样返回。
pub fn group_integer_part(s: &str) -> String {
    if !group_enabled() {
        return s.to_string();
    }
    let (sign, rest) = match s.strip_prefix('-') {
        Some(r) => ("-", r),
        None => ("", s),
    };
    let (int_part, frac_part) = match rest.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (rest, None),
    };
    let all_digits = |t: &str| t.bytes().all(|b| b.is_ascii_digit());
    if int_part.is_empty() || !all_digits(int_part) {
        return s.to_string();
    }
    if let Some(f) = frac_part {
        if !all_digits(f) {
            return s.to_string();
        }
    }
    if int_part.len() <= 3 {
        return s.to_string();
    }
    let mut grouped = String::with_capacity(s.len() + s.len() / 3 + 1);
    grouped.push_str(sign);
    let bytes = int_part.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(*b as char);
    }
    if let Some(f) = frac_part {
        grouped.push('.');
        grouped.push_str(f);
    }
    grouped
}

/// 当前显示有效位数
pub fn display_digits() -> usize {
    DISPLAY_DIGITS_RT.load(Ordering::Relaxed)
}

/// 设置显示有效位数；调用方负责范围校验
pub fn set_display_digits(n: usize) {
    DISPLAY_DIGITS_RT.store(n, Ordering::Relaxed);
}
/// `exp` 参数量级上限（log10）。|x| 超过 10^6 时结果位数失控，予以拒绝。
/// 说明：exp(x) 的整数部分位数约为 |x|·log10(e)，10^6 量级参数对应约 43 万位结果。
pub const EXP_ARG_LIMIT_LOG10: f64 = 6.0;
/// 幂运算/阶乘等结果十进制位数的上限（规模保护，避免内存与耗时失控）
pub const MAX_RESULT_DIGITS: f64 = 1_000_000.0;

/// 常数缓存的键：目标精度（小数位数）→ 已算好的常数
type ConstantCache = std::sync::Mutex<std::collections::HashMap<usize, BigFloat>>;

/// π / e / ln2 三个常数的按精度缓存。
/// 说明：这三者都是级数/迭代计算（π 为 Gauss-Legendre、e 与 ln2 为无穷级数），
/// 而牛顿迭代求解方程时每轮求值都会用到它们（pow 走 exp(b·ln a)、三角归约走 π），
/// 重复计算是 debug 构建下方程求解慢的主因；单线程 REPL 下用互斥锁保护即可。
fn pi_cache() -> &'static ConstantCache {
    static CACHE: std::sync::OnceLock<ConstantCache> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

fn e_cache() -> &'static ConstantCache {
    static CACHE: std::sync::OnceLock<ConstantCache> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

fn ln2_cache() -> &'static ConstantCache {
    static CACHE: std::sync::OnceLock<ConstantCache> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// 查缓存（锁中毒或未命中返回 None，调用方照常现算）
fn cache_get(cache: &'static ConstantCache, prec: usize) -> Option<BigFloat> {
    cache.lock().ok()?.get(&prec).cloned()
}

/// 写缓存（锁中毒时静默跳过，不影响正确性）
fn cache_put(cache: &'static ConstantCache, prec: usize, value: BigFloat) {
    if let Ok(mut map) = cache.lock() {
        map.insert(prec, value);
    }
}

/// `ln(小整数)` 缓存：键 (底数, 目标精度) → 已算好的值。
/// 说明：`pow(a, b) = exp(b·ln a)` 是幂运算与指数方程的主要开销，
/// 而牛顿迭代求解 `a^x = c` 时底数 a 固定不变、却每轮都要重算 `ln(a)`
/// （atanh 级数展开约百项，实测 `3^x=27` 因此比 `2^x=8` 慢 5 倍，后者
/// 命中 ln2 缓存）；这里只缓存**小整数**底数（避免任意参数污染缓存，
/// 如牛顿迭代中变化的 `ln(1+x)` 不会入表），容量满时整体清空。
type IntLnCache = std::sync::Mutex<std::collections::HashMap<(BigInt, usize), BigFloat>>;

/// 小整数 ln 缓存的容量上限（超出即清空；条目很小，仅防无界增长）
const INT_LN_CACHE_CAP: usize = 64;

fn int_ln_cache() -> &'static IntLnCache {
    static CACHE: std::sync::OnceLock<IntLnCache> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// 查小整数 ln 缓存（锁中毒或未命中返回 None）
fn int_ln_get(n: &BigInt, prec: usize) -> Option<BigFloat> {
    int_ln_cache().lock().ok()?.get(&(n.clone(), prec)).cloned()
}

/// 写小整数 ln 缓存（锁中毒时静默跳过；容量满时先清空）
fn int_ln_put(n: BigInt, prec: usize, value: BigFloat) {
    if let Ok(mut map) = int_ln_cache().lock() {
        if map.len() >= INT_LN_CACHE_CAP {
            map.clear();
        }
        map.insert((n, prec), value);
    }
}

/// 高精度浮点数：值 = value / 10^precision
#[derive(Debug, Clone)]
pub struct BigFloat {
    pub value: BigInt,
    pub precision: usize,
}

impl BigFloat {
    /// 从整数创建（精度为 0）
    pub fn from_int(n: &BigInt) -> Self {
        Self {
            value: n.clone(),
            precision: 0,
        }
    }

    pub fn from_u64(n: u64) -> Self {
        Self {
            value: BigInt::from(n),
            precision: 0,
        }
    }

    pub fn from_i64(n: i64) -> Self {
        Self {
            value: BigInt::from(n),
            precision: 0,
        }
    }

    pub fn from_big_rational(num: &num_rational::BigRational) -> BigFloat {
        Self::div(
            &BigFloat::from_int(num.numer()),
            &BigFloat::from_int(num.denom()),
            precision(),
        )
    }

    /// 是否为零
    pub fn is_zero(&self) -> bool {
        self.value.is_zero()
    }

    /// 符号：正返回 true，负返回 false
    pub fn is_positive(&self) -> bool {
        self.value.is_positive()
    }

    /// |self| 的十进制量级估计：返回 log10|x|（零返回负无穷）。
    /// 用途：规模保护（判断幂/指数的结果位数会不会失控），不做精确计算。
    pub fn magnitude_log10(&self) -> f64 {
        if self.is_zero() {
            return f64::NEG_INFINITY;
        }
        // value = |x|·10^precision ⇒ log2|x| = bits(value) - precision·log2(10)
        const LOG2_10: f64 = 3.321_928_094_887_362;
        let bits = self.value.abs().bits() as f64;
        (bits - self.precision as f64 * LOG2_10) / LOG2_10
    }

    /// 取相反数
    pub fn neg(&self) -> BigFloat {
        BigFloat {
            value: -&self.value,
            precision: self.precision,
        }
    }

    /// 规范化：移除尾部的零
    pub fn normalize(&mut self) {
        if self.value.is_zero() {
            self.precision = 0;
            return;
        }
        let ten = BigInt::from(10);
        while self.precision > 0 {
            let (q, r) = self.value.div_rem(&ten);
            if r.is_zero() {
                self.value = q;
                self.precision -= 1;
            } else {
                break;
            }
        }
    }

    /// 将 self 调整到指定小数位数（不足补零，多余四舍五入）
    pub fn round_to(&mut self, target_prec: usize) {
        if self.precision == target_prec {
            return;
        }
        if self.precision < target_prec {
            let diff = target_prec - self.precision;
            self.value = &self.value * BigInt::from(10).pow(diff as u32);
            self.precision = target_prec;
            return;
        }

        let diff = self.precision - target_prec;
        let pow10 = BigInt::from(10).pow(diff as u32);
        let is_neg = self.value.is_negative();
        let abs_val = self.value.abs();

        let (q, r) = abs_val.div_rem(&pow10);
        let mut new_abs = q;
        if (r * 2) >= pow10 {
            new_abs += 1u32;
        }
        self.value = if is_neg { -new_abs } else { new_abs };
        self.precision = target_prec;
    }

    /// 四舍五入到指定位数并返回
    pub fn rounded(&self, target_prec: usize) -> Self {
        let mut result = self.clone();
        result.round_to(target_prec);
        result
    }

    /// 把两个数对齐到共同精度（max(精度)+1），供 `PartialEq` / `PartialOrd` 复用。
    /// 对齐后两者精度必然相同（`round_to` 会把精度设为目标值），因此可直接比较尾数。
    fn aligned_pair(a: &BigFloat, b: &BigFloat) -> (BigFloat, BigFloat) {
        let prec = std::cmp::max(a.precision, b.precision) + 1;
        (a.rounded(prec), b.rounded(prec))
    }

    /// 高精度加法
    pub fn add(a: &BigFloat, b: &BigFloat, target_prec: usize) -> BigFloat {
        let prec = std::cmp::max(
            std::cmp::max(a.precision, b.precision),
            target_prec + MARGIN / 2,
        );
        let ten = BigInt::from(10);
        let a_val = &a.value * ten.pow((prec - a.precision) as u32);
        let b_val = &b.value * ten.pow((prec - b.precision) as u32);
        let mut result = BigFloat {
            value: a_val + b_val,
            precision: prec,
        };
        result.round_to(target_prec);
        result
    }

    /// 高精度减法
    pub fn sub(a: &BigFloat, b: &BigFloat, target_prec: usize) -> BigFloat {
        Self::add(a, &b.neg(), target_prec)
    }

    /// 高精度乘法
    pub fn mul(a: &BigFloat, b: &BigFloat, target_prec: usize) -> BigFloat {
        let value = &a.value * &b.value;
        let prec = a.precision + b.precision;
        let mut result = BigFloat {
            value,
            precision: prec,
        };
        result.round_to(target_prec);
        result
    }

    /// 高精度除法
    pub fn div(a: &BigFloat, b: &BigFloat, target_prec: usize) -> BigFloat {
        assert!(!b.is_zero(), "除以零错误");

        let total_prec = target_prec + MARGIN + b.precision;
        let ten = BigInt::from(10);
        let scale = if a.precision < total_prec {
            total_prec - a.precision
        } else {
            0
        };

        let numerator = a.value.abs() * ten.pow(scale as u32);
        let denominator = b.value.abs();
        // 走 bigint_ext 分派：两个操作数都很大时绕开 num-bigint 的 BZ 除法断言缺陷
        // （实测 (10^200000+pi)/(10^100000+pi) 会 panic），小规模仍用原生除法
        let quotient = crate::bigint_ext::div(&numerator, &denominator);

        let result_sign = if a.value.sign() == b.value.sign() {
            Sign::Plus
        } else {
            Sign::Minus
        };
        let result_value =
            BigInt::from_biguint(result_sign, quotient.to_biguint().unwrap_or_default());

        // 修正精度: a/b = (A/B) * 10^(pb-pa)
        // quotient = A * 10^scale / B
        // result = quotient / 10^P ≈ a/b
        // P = scale + pa - pb
        // P 恒非负：scale > 0 时 P = target+MARGIN+b.precision-b.precision = target+MARGIN；
        //          scale == 0 时必有 a.precision ≥ target+MARGIN+b.precision ≥ b.precision。
        // 因此不存在"需要反向缩放"的分支（旧代码的 else 分支不可达，且量级少乘 10^b.precision）。
        let result_precision = a.precision + scale - b.precision;
        let mut result = BigFloat {
            value: result_value,
            precision: result_precision,
        };
        result.round_to(target_prec);
        result
    }

    /// 平方根（牛顿迭代法）
    pub fn sqrt(&self, target_prec: usize) -> BigFloat {
        assert!(!self.value.is_negative(), "负数不能开平方");
        if self.is_zero() {
            return BigFloat::from_u64(0);
        }

        let work_prec = target_prec + MARGIN;

        // 使用整数平方根获得更好的初始猜测
        let extra = if self.precision % 2 == 1 { 1 } else { 0 };
        let shifted_value = self.value.abs() * BigInt::from(10).pow(extra as u32);
        let shifted_prec = self.precision + extra;
        let guess_val = crate::bigint_ext::int_sqrt(&shifted_value);
        let guess_prec = shifted_prec / 2;
        let mut x = BigFloat {
            value: BigInt::from_biguint(Sign::Plus, guess_val.to_biguint().unwrap()),
            precision: guess_prec,
        };
        // 将初始猜测扩展到足够精度
        x = x.rounded(work_prec);

        let two = BigFloat::from_u64(2);
        let max_iter = 200;

        for _ in 0..max_iter {
            let s_over_x = BigFloat::div(self, &x, work_prec + 5);
            let sum = BigFloat::add(&x, &s_over_x, work_prec + 5);
            let x_next = BigFloat::div(&sum, &two, work_prec + 5);

            let diff = BigFloat::sub(&x_next, &x, work_prec);
            if diff.value.abs() <= BigInt::from(1) {
                x = x_next;
                break;
            }
            x = x_next;
        }

        x.rounded(target_prec)
    }

    /// k 次根（k ≥ 2，负数由调用方先取绝对值）：先用 `exp(ln(x)/k)` 取初值，
    /// 再牛顿迭代 `y ← ((k-1)y + x/y^(k-1))/k` 细化。
    /// 不直接把 exp(ln) 当结果——那条路在尾部会丢几位有效数字，而牛顿迭代能收敛到目标精度。
    pub fn nroot(&self, k: u32, target_prec: usize) -> Result<BigFloat, String> {
        assert!(!self.value.is_negative(), "nroot 只接受非负数");
        if self.is_zero() {
            return Ok(BigFloat::from_u64(0));
        }
        if k <= 1 {
            return Ok(self.rounded(target_prec));
        }
        if k == 2 {
            return Ok(self.sqrt(target_prec));
        }
        let work_prec = target_prec + MARGIN;
        // 初值：exp(ln(x)/k)（量级正确即可，精度由牛顿迭代补齐）
        let mut y = {
            let ln_x = self.ln(work_prec);
            let kf = BigFloat::from_u64(k as u64);
            let arg = BigFloat::div(&ln_x, &kf, work_prec);
            arg.exp(work_prec)?
        };
        let km1 = BigFloat::from_u64((k - 1) as u64);
        let kf = BigFloat::from_u64(k as u64);
        for _ in 0..40 {
            let p = y.pow(&km1, work_prec)?;
            if p.is_zero() {
                break;
            }
            let q = BigFloat::div(self, &p, work_prec);
            let t = BigFloat::add(&BigFloat::mul(&km1, &y, work_prec), &q, work_prec);
            let y_next = BigFloat::div(&t, &kf, work_prec);
            let diff = BigFloat::sub(&y_next, &y, work_prec);
            y = y_next;
            if diff.value.abs() <= BigInt::from(1) {
                break;
            }
        }
        Ok(y.rounded(target_prec))
    }

    /// 幂运算：a^b = exp(b * ln(a))
    ///
    /// 带规模保护：超大批次幂（如 2^10000000000）会因结果位数失控而拒绝，
    /// 不再静默退化为 exp 级数的截断值（历史缺陷：返回完全无关的量级）。
    pub fn pow(&self, exponent: &BigFloat, target_prec: usize) -> Result<BigFloat, String> {
        if self.is_zero() {
            if exponent.is_zero() {
                return Ok(BigFloat::from_u64(1));
            }
            if exponent.value.is_negative() {
                return Err("0 的负次幂未定义".to_string());
            }
            return Ok(BigFloat::from_u64(0));
        }
        if exponent.is_zero() {
            return Ok(BigFloat::from_u64(1));
        }
        // 实数域内负底数只能配整数指数（Number 层已拦截，这里是兜底）
        if self.value.is_negative() {
            let is_int = exponent.precision == 0
                || BigFloat::sub(
                    exponent,
                    &BigFloat::from_int(
                        &exponent
                            .value
                            .div_floor(&BigInt::from(10).pow(exponent.precision as u32)),
                    ),
                    target_prec + MARGIN,
                )
                .is_zero();
            if !is_int {
                return Err("负数的非整数次幂在实数范围内无定义".to_string());
            }
        }

        let work_prec = target_prec + MARGIN;

        // 检查指数是否为整数
        if exponent.precision == 0
            || (exponent.precision > 0
                && BigFloat::sub(
                    exponent,
                    &BigFloat::from_int(
                        &exponent
                            .value
                            .div_floor(&BigInt::from(10).pow((exponent.precision) as u32)),
                    ),
                    work_prec,
                )
                .is_zero())
        {
            let exp_int = &exponent.value / BigInt::from(10).pow(exponent.precision as u32);
            if let Some(exp_u32) = exp_int.to_u32() {
                self.check_pow_scale(exp_u32 as f64)?;
                return Ok(self.int_pow(exp_u32, target_prec));
            }
            if let Some(exp_i32) = exp_int.to_i32() {
                if exp_i32 < 0 {
                    self.check_pow_scale((-exp_i32) as f64)?;
                    let pos_pow = self.int_pow((-exp_i32) as u32, work_prec);
                    return Ok(BigFloat::div(&BigFloat::from_u64(1), &pos_pow, target_prec));
                }
            }
            // 指数超出 u32/i32 表示范围：不能走整数快速幂，交给下面的 exp/ln 路径前先做规模检查
        }

        // 一般情况：a^b = exp(b * ln(a))
        let ln_a = self.ln(work_prec);
        let product = BigFloat::mul(exponent, &ln_a, work_prec);
        let arg_log10 = product.magnitude_log10();
        // 死算模式 (/mode deep) 取消量级上限
        if arg_log10 > EXP_ARG_LIMIT_LOG10 && !crate::calc_mode::is_deep() {
            return Err(format!(
                "幂运算结果超出支持范围（指数 × ln(底数) ≈ 10^{:.0}，结果约 10^{:.0} 位十进制）",
                arg_log10,
                // 结果位数 ≈ 10^arg_log10·log10(e) ⇒ 其量级为 10^(arg_log10 + log10(log10 e))
                arg_log10 - 0.362_215_683_792_409_6
            ));
        }
        product.exp(target_prec)
    }

    /// 幂运算规模保护：估算 |self|^exp 的十进制位数，超限报错。
    /// 死算模式 (/mode deep) 下不设上限。
    fn check_pow_scale(&self, exp_abs: f64) -> Result<(), String> {
        if crate::calc_mode::is_deep() {
            return Ok(());
        }
        let digits = exp_abs * self.magnitude_log10().abs();
        if digits > MAX_RESULT_DIGITS {
            return Err(format!(
                "幂运算结果约有 {:.0} 位十进制，超过支持上限（约 10^{:.0} 位）",
                digits,
                MAX_RESULT_DIGITS.log10()
            ));
        }
        Ok(())
    }

    /// 整数次幂（快速幂）
    fn int_pow(&self, exp: u32, target_prec: usize) -> BigFloat {
        if exp == 0 {
            return BigFloat::from_u64(1);
        }
        if exp == 1 {
            return self.rounded(target_prec);
        }

        let mut base = self.rounded(target_prec + MARGIN);
        let mut result = BigFloat::from_u64(1);
        let mut e = exp;

        while e > 0 {
            if e & 1 == 1 {
                result = BigFloat::mul(&result, &base, target_prec + MARGIN);
            }
            e >>= 1;
            if e > 0 {
                base = BigFloat::mul(&base, &base, target_prec + MARGIN);
            }
        }
        result.rounded(target_prec)
    }

    /// 自然指数 exp(x) = Σ x^k/k!
    ///
    /// 参数缩半：exp(x) = exp(x/2^k)^(2^k)，先把 |参数| 压进 [0,1] 再展级数，
    /// 使级数项 x^k/k! 单调下降，从而可以用**相对**判据收敛。
    /// 旧实现对 |x| 很大时永远不满足绝对判据 |term| ≤ 1，只能跑满 5000 次迭代，
    /// 而每项的整数位数随 |x|·k 增长（BigFloat 是 value/10^prec），会卡死并返回错误结果。
    pub fn exp(&self, target_prec: usize) -> Result<BigFloat, String> {
        if self.is_zero() {
            return Ok(BigFloat::from_u64(1));
        }
        let arg_log10 = self.magnitude_log10();
        // 死算模式 (/mode deep) 取消参数上限，交给用户承担耗时/内存
        if arg_log10 > EXP_ARG_LIMIT_LOG10 && !crate::calc_mode::is_deep() {
            return Err(format!(
                "exp 参数过大（约 10^{:.0}），结果将超过 10^{:.0} 位十进制，超出支持范围",
                arg_log10,
                // 结果位数 ≈ 10^arg_log10·log10(e) ⇒ 其量级为 10^(arg_log10 + log10(log10 e))
                arg_log10 - 0.362_215_683_792_409_6
            ));
        }

        let work_prec = target_prec + MARGIN;
        let two = BigFloat::from_u64(2);
        let one = BigFloat::from_u64(1).rounded(work_prec);

        // 参数缩半到 [-1, 1]
        let mut arg = self.rounded(work_prec);
        let mut halvings: u32 = 0;
        while arg.value.abs() > one.value.abs() {
            arg = BigFloat::div(&arg, &two, work_prec);
            halvings += 1;
            if halvings > 4096 {
                return Err("exp 参数过大".to_string());
            }
        }

        // 级数展开（|arg| ≤ 1 时约 100 项即收敛到 work_prec 位）
        let scale = BigInt::from(10).pow(work_prec as u32);
        let mut result = BigFloat::from_u64(1);
        let mut term = BigFloat::from_u64(1);
        // 计数器用 u64（旧实现每轮 `&k + &step` 都要做一次 BigInt 加法并克隆，
        // 而 exp 在牛顿迭代里被反复调用，这里省掉即可）
        let mut k: u64 = 1;
        for _ in 0..2000 {
            term = BigFloat::div(
                &BigFloat::mul(&term, &arg, work_prec),
                &BigFloat::from_u64(k),
                work_prec,
            );
            result = BigFloat::add(&result, &term, work_prec);

            // 相对判据：|term| ≤ |result|·10^-work_prec（两者精度均为 work_prec，可整数直比）
            if &term.value.abs() * &scale <= result.value.abs() {
                break;
            }
            k += 1;
        }

        // 平方 halvings 次还原：exp(arg)^(2^halvings) = exp(x)
        for _ in 0..halvings {
            result = BigFloat::mul(&result, &result, work_prec);
        }
        Ok(result.rounded(target_prec))
    }

    /// 自然对数 ln(x)：将参数缩放为 2 的幂倍数（s = x/2^m ∈ [1,2)）后求级数，
    /// 避免大参数时 arctanh 级数在收敛半径边缘慢收敛导致的精度损失。
    /// 小整数底数（多项式/指数方程里最常出现的 2、3、5…）走按精度的专用缓存。
    pub fn ln(&self, target_prec: usize) -> BigFloat {
        assert!(self.is_positive(), "ln 参数必须为正数");

        let work_prec = target_prec + MARGIN;
        let one = BigFloat::from_u64(1);
        let two_f = BigFloat::from_u64(2);

        if self == &one {
            return BigFloat::from_u64(0);
        }

        // 小整数底数的缓存键：值必须是 2..=10^6 的整数。
        // 注意 AST 里的常数经 `to_bigfloat` 后形如 3·10^80（precision 不为 0），
        // 必须先按精度归一还原出整数值，不能直接用 `precision == 0` 判断。
        let cache_key = {
            let scale = BigInt::from(10).pow(self.precision as u32);
            let (q, r) = self.value.div_rem(&scale);
            if r.is_zero() && q > BigInt::from(1) && q <= BigInt::from(1_000_000) {
                Some(q)
            } else {
                None
            }
        };
        if let Some(k) = &cache_key {
            if let Some(v) = int_ln_get(k, target_prec) {
                return v;
            }
        }

        // 2 的幂缩放：log2(x) ≈ bits(|value|) - precision·log2(10)
        let bits = self.value.abs().bits() as f64;
        let p = self.precision as f64;
        let mut m = (bits - p * 3.321928094887362 - 4.0).floor() as i64;
        let mut scaled = if m >= 0 {
            let two_m = BigFloat::from_int(&BigInt::from(2).pow(m as u32));
            BigFloat::div(self, &two_m, work_prec)
        } else {
            let two_pm = BigFloat::from_int(&BigInt::from(2).pow((-m) as u32));
            BigFloat::mul(self, &two_pm, work_prec)
        };

        // 修正 m，使 scaled ∈ [1, 2)
        while scaled.rounded(work_prec).value >= two_f.rounded(work_prec).value {
            scaled = BigFloat::div(&scaled, &two_f, work_prec);
            m += 1;
        }
        while scaled.rounded(work_prec).value < one.rounded(work_prec).value {
            scaled = BigFloat::mul(&scaled, &two_f, work_prec);
            m -= 1;
        }

        // ln 核心：2·atanh((s-1)/(s+1))，s∈[1,2) → y<1/3，级数快速收敛
        let num = BigFloat::sub(&scaled, &one, work_prec);
        let den = BigFloat::add(&scaled, &one, work_prec);
        let y = BigFloat::div(&num, &den, work_prec);
        let y2 = BigFloat::mul(&y, &y, work_prec);

        let mut result = y.clone();
        let mut term = y.clone();
        let mut n = BigInt::from(3);
        for _ in 0..2000 {
            term = BigFloat::mul(&term, &y2, work_prec);
            let term_div = BigFloat::div(&term, &BigFloat::from_int(&n), work_prec);
            result = BigFloat::add(&result, &term_div, work_prec);

            if term.value.abs() <= BigInt::from(1) && term.precision >= work_prec {
                break;
            }
            n = &n + &BigInt::from(2);
        }
        let part = BigFloat::mul(&two_f, &result, work_prec);

        // ln2：由 2·atanh(1/3) 独立计算（避免递归依赖），并走按精度缓存
        let ln2 = Self::ln2_cached(work_prec);

        // ln(x) = m·ln2 + ln(s)
        let mln2 = BigFloat::mul(&BigFloat::from_i64(m), &ln2, work_prec);
        let result = BigFloat::add(&mln2, &part, target_prec);
        if let Some(k) = cache_key {
            int_ln_put(k, target_prec, result.clone());
        }
        result
    }

    /// 正弦 sin(x)
    pub fn sin(&self, target_prec: usize) -> BigFloat {
        let work_prec = target_prec + MARGIN;
        let pi = Self::pi(work_prec);
        let two_pi = BigFloat::mul(&pi, &BigFloat::from_u64(2), work_prec);

        // 归约到 [-pi, pi]
        let mut x = self.rounded(work_prec);
        if x.value.abs() > pi.value {
            let quot = BigFloat::div(&x, &two_pi, work_prec);
            let int_part = BigFloat::from_int(
                &quot
                    .value
                    .div_floor(&BigInt::from(10).pow(quot.precision as u32)),
            );
            x = BigFloat::sub(&x, &BigFloat::mul(&int_part, &two_pi, work_prec), work_prec);
        }

        // 使用 sin(x) 的泰勒级数
        let mut result = x.clone();
        let mut term = x.clone();
        let x2 = BigFloat::mul(&x, &x, work_prec);
        let mut n = BigInt::from(2);

        for _ in 0..2000 {
            term = BigFloat::mul(&term, &x2, work_prec);
            term = BigFloat::div(
                &term,
                &BigFloat::from_int(&(&n * &(&n + BigInt::from(1)))),
                work_prec,
            );
            term = term.neg();

            result = BigFloat::add(&result, &term, work_prec);

            if term.value.abs() <= BigInt::from(1) && term.precision >= work_prec {
                break;
            }
            n += BigInt::from(2);
        }
        result.rounded(target_prec)
    }

    /// 余弦 cos(x)
    pub fn cos(&self, target_prec: usize) -> BigFloat {
        let work_prec = target_prec + MARGIN;
        let pi = Self::pi(work_prec);
        let half_pi = BigFloat::div(&pi, &BigFloat::from_u64(2), work_prec);

        // sin(x + pi/2) = cos(x)
        let x_shifted = BigFloat::add(self, &half_pi, work_prec);
        x_shifted.sin(target_prec)
    }

    /// 正切 tan(x) = sin(x)/cos(x)
    pub fn tan(&self, target_prec: usize) -> BigFloat {
        let work_prec = target_prec + MARGIN;
        let s = self.sin(work_prec);
        let c = self.cos(work_prec);
        BigFloat::div(&s, &c, target_prec)
    }

    /// 反正弦 arcsin(x)
    pub fn asin(&self, target_prec: usize) -> BigFloat {
        let work_prec = target_prec + MARGIN;
        let one = BigFloat::from_u64(1);

        // 边界情况：|x| = 1
        let ten_pow = BigInt::from(10).pow(self.precision as u32);
        if self.value.abs() >= ten_pow {
            let pi = Self::pi(work_prec);
            let half_pi = BigFloat::div(&pi, &BigFloat::from_u64(2), work_prec);
            return if self.value.is_negative() {
                half_pi.neg().rounded(target_prec)
            } else {
                half_pi.rounded(target_prec)
            };
        }

        // arcsin(x) = arctan(x / sqrt(1 - x^2))
        let x2 = BigFloat::mul(self, self, work_prec);
        let sqrt_one_minus_x2 = BigFloat::sub(&one, &x2, work_prec).sqrt(work_prec);
        let quotient = BigFloat::div(self, &sqrt_one_minus_x2, work_prec);
        quotient.atan(target_prec)
    }

    /// 反余弦 arccos(x) = pi/2 - arcsin(x)
    pub fn acos(&self, target_prec: usize) -> BigFloat {
        let work_prec = target_prec + MARGIN;
        let pi = Self::pi(work_prec);
        let half_pi = BigFloat::div(&pi, &BigFloat::from_u64(2), work_prec);
        let asin_val = self.asin(work_prec);
        BigFloat::sub(&half_pi, &asin_val, target_prec)
    }

    /// 反正切 arctan(x)
    pub fn atan(&self, target_prec: usize) -> BigFloat {
        if self.is_zero() {
            return BigFloat::from_u64(0);
        }

        let work_prec = target_prec + MARGIN + 5;
        let one = BigFloat::from_u64(1);
        let two = BigFloat::from_u64(2);

        // 通过恒等式 atan(x) = 2 * atan(x / (1 + sqrt(1 + x^2))) 缩半参数
        let mut x = self.rounded(work_prec);
        let mut multiplier = BigFloat::from_u64(1);
        let quarter = BigFloat::div(&one, &BigFloat::from_u64(4), work_prec);

        loop {
            let x2 = BigFloat::mul(&x, &x, work_prec);
            let cmp = BigFloat::sub(&x2, &quarter, work_prec);
            if cmp.value.is_negative() || cmp.is_zero() {
                break;
            }
            let sqrt_term = BigFloat::add(&one, &x2, work_prec).sqrt(work_prec);
            let denom = BigFloat::add(&one, &sqrt_term, work_prec);
            x = BigFloat::div(&x, &denom, work_prec);
            multiplier = BigFloat::mul(&multiplier, &two, work_prec);
        }

        // 级数: atan(x) = x - x^3/3 + x^5/5 - x^7/7 + ...
        let y = x.clone();
        let y2 = BigFloat::mul(&y, &y, work_prec);
        let mut result = y.clone();
        let mut term = y.clone();
        let mut n = BigInt::from(3);

        for _ in 0..5000 {
            term = BigFloat::mul(&term, &y2, work_prec);
            term = term.neg();
            let term_div = BigFloat::div(&term, &BigFloat::from_int(&n), work_prec);
            result = BigFloat::add(&result, &term_div, work_prec);

            if term.value.abs() <= BigInt::from(1) && term.precision >= work_prec {
                break;
            }
            n += BigInt::from(2);
        }

        BigFloat::mul(&multiplier, &result, target_prec)
    }

    /// 反余切 arccot(x) = pi/2 - arctan(x)
    pub fn acot(&self, target_prec: usize) -> BigFloat {
        let work_prec = target_prec + MARGIN;
        let pi = Self::pi(work_prec);
        let half_pi = BigFloat::div(&pi, &BigFloat::from_u64(2), work_prec);
        BigFloat::sub(&half_pi, &self.atan(work_prec), target_prec)
    }

    /// 反正割 arcsec(x) = arccos(1/x)
    pub fn asec(&self, target_prec: usize) -> BigFloat {
        let work_prec = target_prec + MARGIN;
        BigFloat::div(&BigFloat::from_u64(1), self, work_prec).acos(target_prec)
    }

    /// 反余割 arccsc(x) = arcsin(1/x)
    pub fn acsc(&self, target_prec: usize) -> BigFloat {
        let work_prec = target_prec + MARGIN;
        BigFloat::div(&BigFloat::from_u64(1), self, work_prec).asin(target_prec)
    }

    /// 圆周率 pi（Gauss-Legendre 算法，按目标精度缓存）
    pub fn pi(target_prec: usize) -> BigFloat {
        if let Some(v) = cache_get(pi_cache(), target_prec) {
            return v;
        }
        let work_prec = target_prec + MARGIN;
        let two = BigFloat::from_u64(2);
        let four = BigFloat::from_u64(4);

        let mut a = BigFloat::from_u64(1);
        let sqrt2 = BigFloat::from_u64(2).sqrt(work_prec);
        let mut b = BigFloat::div(&BigFloat::from_u64(1), &sqrt2, work_prec);
        let mut t = BigFloat::from_int(&BigInt::from(1)).rounded(work_prec);
        t = BigFloat::div(&t, &BigFloat::from_u64(4), work_prec);
        let mut p = BigFloat::from_u64(1);

        for _ in 0..10 {
            let a_next = BigFloat::div(&BigFloat::add(&a, &b, work_prec), &two, work_prec);
            let b_next = BigFloat::mul(&a, &b, work_prec).sqrt(work_prec);
            let diff = BigFloat::sub(&a, &a_next, work_prec);
            let diff2 = BigFloat::mul(&diff, &diff, work_prec);
            let p_diff2 = BigFloat::mul(&p, &diff2, work_prec);
            let t_next = BigFloat::sub(&t, &p_diff2, work_prec);
            let p_next = BigFloat::mul(&p, &two, work_prec);

            a = a_next;
            b = b_next;
            t = t_next;
            p = p_next;

            let check = BigFloat::sub(&a, &b, work_prec);
            if check.value.abs() <= BigInt::from(1) && check.precision >= work_prec - 5 {
                break;
            }
        }

        let sum = BigFloat::add(&a, &b, work_prec);
        let numerator = BigFloat::mul(&sum, &sum, work_prec);
        let denominator = BigFloat::mul(&four, &t, work_prec);
        let value = BigFloat::div(&numerator, &denominator, target_prec);
        cache_put(pi_cache(), target_prec, value.clone());
        value
    }

    /// 自然常数 e（按目标精度缓存）
    pub fn e(target_prec: usize) -> BigFloat {
        if let Some(v) = cache_get(e_cache(), target_prec) {
            return v;
        }
        let work_prec = target_prec + MARGIN;
        let mut result = BigFloat::from_u64(1);
        let mut term = BigFloat::from_u64(1);
        let one = BigInt::one();
        let mut k = BigInt::one();

        for _ in 0..1000 {
            term = BigFloat::div(&term, &BigFloat::from_int(&k), work_prec);
            result = BigFloat::add(&result, &term, work_prec);

            if term.value <= BigInt::from(1) && term.precision >= work_prec {
                break;
            }
            k = &k + &one;
        }
        let value = result.rounded(target_prec);
        cache_put(e_cache(), target_prec, value.clone());
        value
    }

    /// ln2 = 2·atanh(1/3)（按精度缓存）。
    /// 单独抽出是因为 `ln` 每次调用都要用它换算 2 的幂缩放量（ln x = m·ln2 + ln s），
    /// 而级数本身与参数无关，重复计算纯属浪费。
    fn ln2_cached(prec: usize) -> BigFloat {
        if let Some(v) = cache_get(ln2_cache(), prec) {
            return v;
        }
        let one = BigFloat::from_u64(1);
        let two = BigFloat::from_u64(2);
        let one_third = BigFloat::div(&one, &BigFloat::from_u64(3), prec);
        let y2 = BigFloat::mul(&one_third, &one_third, prec);
        let mut res = one_third.clone();
        let mut term = one_third.clone();
        let mut n = BigInt::from(3);
        for _ in 0..2000 {
            term = BigFloat::mul(&term, &y2, prec);
            res = BigFloat::add(
                &res,
                &BigFloat::div(&term, &BigFloat::from_int(&n), prec),
                prec,
            );
            if term.value.abs() <= BigInt::from(1) && term.precision >= prec {
                break;
            }
            n = &n + &BigInt::from(2);
        }
        let value = BigFloat::mul(&two, &res, prec);
        cache_put(ln2_cache(), prec, value.clone());
        value
    }

    /// 以有效数字格式化（对外的显示入口）：先按规则得到纯十进制串，再按 `/mode group` 决定是否加千分位
    pub fn to_significant_string(&self, max_digits: usize) -> String {
        group_integer_part(&self.format_significant(max_digits))
    }

    /// 以有效数字格式化（用于 lineio 模式）
    /// - **死算模式**：完整精度输出（整数位不截断、不用科学计数法、不做 20 位有效数字舍入）；
    /// - 小数位数不超过 max_digits：输出全部数字；
    /// - 整数部分位数 > max_digits：输出 max_digits 位有效数字的**科学计数法**（保留量级）；
    ///   可用 `/mode sci off` 关闭该分支（改为完整写出整数部分）；
    /// - 其余情况：四舍五入到 max_digits 位有效数字。
    fn format_significant(&self, max_digits: usize) -> String {
        let mut n = self.clone();
        n.normalize();
        let is_neg = n.value.is_negative();
        let abs_val = n.value.abs();
        let abs_str = abs_val.to_string();
        let total_len = abs_str.len();
        let prec = n.precision;

        if prec == 0 {
            return if is_neg {
                format!("-{}", abs_str)
            } else {
                abs_str
            };
        }

        let int_digits = if prec < total_len {
            total_len - prec
        } else {
            0
        };

        // 死算模式：完整十进制（整数部分全写 + 全部小数位），不截断、不用科学计数法
        if crate::calc_mode::is_deep() {
            let full = if prec >= total_len {
                format!("0.{}{}", "0".repeat(prec - total_len), abs_str)
            } else {
                format!(
                    "{}.{}",
                    &abs_str[..total_len - prec],
                    &abs_str[total_len - prec..]
                )
            };
            return if is_neg { format!("-{}", full) } else { full };
        }

        // 整数部分位数超过 max_digits 时改用科学计数法。
        // 旧实现直接按字符串截断，会把整数部分的低位（连同数量级）一起丢掉，
        // 例如 10^25/3 被显示成 3.3e19（错误 10^5 倍）。
        if int_digits > max_digits && sci_allowed() {
            return n.to_scientific_string(max_digits, &abs_str, int_digits, is_neg);
        }

        // 构造完整十进制字符串
        let full_decimal = if prec >= total_len {
            let padding = prec - total_len;
            format!("0.{}{}", "0".repeat(padding), abs_str)
        } else {
            let int_part = &abs_str[..total_len - prec];
            let frac_part = &abs_str[total_len - prec..];
            if int_part.is_empty() {
                format!("0.{}", frac_part)
            } else {
                format!("{}.{}", int_part, frac_part)
            }
        };

        // 如果小数位数不超过 max_digits，输出全部
        if n.precision <= max_digits {
            return if is_neg {
                format!("-{}", full_decimal)
            } else {
                full_decimal
            };
        }

        // 否则输出 max_digits 位有效数字（四舍五入）
        self.round_to_significant(&full_decimal, max_digits, is_neg)
    }

    /// 大数输出：`d.dddddd×10^n`（mantissa 最多 max_digits 位有效数字，尾零去掉）
    fn to_scientific_string(
        &self,
        max_digits: usize,
        abs_str: &str,
        int_digits: usize,
        is_neg: bool,
    ) -> String {
        // 对整数位串做 max_digits 位有效数字四舍五入（进位时长度会多 1 位）
        let rounded = self.round_to_significant(abs_str, max_digits, false);
        let len = rounded.len();
        // 数值 = rounded[0].rounded[1..] × 10^exp
        let exp = int_digits as i64 - max_digits as i64 + len as i64 - 1;
        let trimmed = rounded.trim_end_matches('0');
        let mantissa = if trimmed.len() <= 1 {
            trimmed.to_string()
        } else {
            format!("{}.{}", &trimmed[..1], &trimmed[1..])
        };
        let sign = if is_neg { "-" } else { "" };
        format!("{}{}×10^{}", sign, mantissa, exp)
    }

    /// 将格式化的十进制字符串四舍五入到指定位有效数字
    fn round_to_significant(&self, decimal_str: &str, digits: usize, is_neg: bool) -> String {
        let mut chars: Vec<char> = decimal_str.chars().collect();

        // 找到第一个有效数字的位置
        let mut first_sig = None;
        for (i, &ch) in chars.iter().enumerate() {
            if ch != '0' && ch != '.' && ch != '-' {
                first_sig = Some(i);
                break;
            }
        }

        let first_sig = match first_sig {
            Some(i) => i,
            None => return "0".to_string(),
        };

        // 找到小数点的位置
        let _dot_pos = chars.iter().position(|&c| c == '.');

        // 计算第 digits 个有效数字的索引
        let mut sig_count = 0;
        let mut target_idx = first_sig;
        while sig_count < digits && target_idx < chars.len() {
            if chars[target_idx] != '.' {
                sig_count += 1;
            }
            if sig_count < digits {
                target_idx += 1;
            }
        }

        // 如果有效数字不够，补零
        if target_idx >= chars.len() {
            // 有效数字不够，直接返回（不截断）
            let s: String = chars.iter().collect();
            return if is_neg { format!("-{}", s) } else { s };
        }

        // 获取四舍五入所需的判断位
        let mut look_ahead = target_idx + 1;
        while look_ahead < chars.len() && chars[look_ahead] == '.' {
            look_ahead += 1;
        }
        let round_up = if look_ahead < chars.len() {
            chars[look_ahead] >= '5'
        } else {
            false
        };

        // 截取到 target_idx (含)
        chars.truncate(target_idx + 1);

        // 四舍五入
        if round_up {
            let mut i = target_idx;
            loop {
                if chars[i] == '.' {
                    if i == 0 {
                        chars.insert(0, '1');
                        break;
                    }
                    i -= 1;
                    continue;
                }
                if chars[i] < '9' {
                    chars[i] = (chars[i] as u8 + 1) as char;
                    break;
                }
                chars[i] = '0';
                if i == 0 {
                    chars.insert(0, '1');
                    // 调整小数点位置
                    if let Some(dp) = chars.iter().position(|&c| c == '.') {
                        if dp == 1 {
                            // 原来是 9.xxx -> 10.xxx, 需要调整
                            chars.remove(dp);
                            chars.insert(dp + 1, '.');
                        }
                    }
                    break;
                }
                i -= 1;
            }
        }

        // 去掉末尾零和多余的小数点
        let mut result: String = chars.iter().collect();
        if result.contains('.') {
            result = result.trim_end_matches('0').to_string();
            result = result.trim_end_matches('.').to_string();
        }

        if is_neg {
            format!("-{}", result)
        } else {
            result
        }
    }

    /// 输出普通字符串
    fn to_string_plain(&self, max_prec: usize) -> String {
        let is_neg = self.value.is_negative();
        let abs_val = self.value.abs();
        let abs_str = abs_val.to_string();
        let prec = self.precision.min(max_prec);

        if prec == 0 {
            return if is_neg {
                format!("-{}", abs_str)
            } else {
                abs_str
            };
        }

        let total_len = abs_str.len();
        if prec >= total_len {
            let padding = prec - total_len;
            let s = format!("0.{}{}", "0".repeat(padding), abs_str);
            if is_neg { format!("-{}", s) } else { s }
        } else {
            let int_part = &abs_str[..total_len - prec];
            let frac_part = &abs_str[total_len - prec..];
            let s = if int_part.is_empty() {
                format!("0.{}", frac_part)
            } else {
                format!("{}.{}", int_part, frac_part)
            };
            if is_neg { format!("-{}", s) } else { s }
        }
    }
}

impl fmt::Display for BigFloat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_string_plain(self.precision.min(30)))
    }
}

impl PartialEq for BigFloat {
    fn eq(&self, other: &Self) -> bool {
        let (a, b) = BigFloat::aligned_pair(self, other);
        a.value == b.value && a.precision == b.precision
    }
}

impl PartialOrd for BigFloat {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        let (a, b) = BigFloat::aligned_pair(self, other);
        // 对齐后两者精度相同，直接比较尾数即可
        Some(a.value.cmp(&b.value))
    }
}

impl Add for BigFloat {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self::add(&self, &other, precision())
    }
}

impl Sub for BigFloat {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self::sub(&self, &other, precision())
    }
}

impl Mul for BigFloat {
    type Output = Self;
    fn mul(self, other: Self) -> Self {
        Self::mul(&self, &other, precision())
    }
}

impl Div for BigFloat {
    type Output = Self;
    fn div(self, other: Self) -> Self {
        Self::div(&self, &other, precision())
    }
}

impl Neg for BigFloat {
    type Output = Self;
    fn neg(self) -> Self {
        Self::neg(&self)
    }
}

/// 由 BigInt 直接构造（精度 0）。供 `BigInt::…into()` 形式使用。
impl From<BigInt> for BigFloat {
    fn from(n: BigInt) -> Self {
        BigFloat {
            value: n,
            precision: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_group_integer_part() {
        // 先确保开关关闭时原样返回
        set_group_enabled(false);
        assert_eq!(group_integer_part("1234567"), "1234567");
        set_group_enabled(true);
        assert_eq!(group_integer_part("1234567"), "1,234,567");
        assert_eq!(group_integer_part("-1234567.89"), "-1,234,567.89");
        assert_eq!(group_integer_part("123"), "123"); // 三位以内不加
        assert_eq!(group_integer_part("0.5"), "0.5");
        // 非纯十进制形式原样返回（分数、根式、科学计数法、符号串）
        for s in ["1 / 2", "(1 / 2)*sqrt(2)", "1.5×10^20", "2*pi", "x = 3"] {
            assert_eq!(group_integer_part(s), s, "{s} 不应被加千分位");
        }
        set_group_enabled(false);
    }

    #[test]
    fn test_constants_computed_per_precision() {
        // 常数缓存按目标精度分桶：两个精度各自算、各自缓存，互不污染
        let p80 = BigFloat::pi(80);
        let p120 = BigFloat::pi(120);
        let s80 = p80.to_significant_string(20);
        let s120 = p120.to_significant_string(20);
        assert_eq!(s80, s120, "不同精度下 pi 的有效数字应一致");
        assert!(s80.starts_with("3.141592653589793238"), "{s80}");
        // 位数不同：120 位缓存的内部精度更高
        assert!(p120.precision >= 120 || p120.value.to_string().len() > 100);
        // 再次取值命中缓存，结果不变
        assert_eq!(
            BigFloat::pi(80).to_significant_string(30),
            p80.to_significant_string(30)
        );
    }
}
