use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, Zero};

use crate::number::{ExactExpr, ExactTerm, Number};

/// 角度模式
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AngleMode {
    Radian,
    Degree,
}

/// 计算角度的正弦值（精确优先）
pub fn sin_number(angle: &Number, mode: AngleMode) -> Number {
    if let Some(result) = try_exact_sin(angle, mode) {
        return result;
    }
    // 回退到数值计算
    let rad = to_radians(angle, mode);
    let bf = rad.to_approx();
    Number::Approx(bf.sin(crate::bigfloat::precision()))
}

/// 计算角度的余弦值
pub fn cos_number(angle: &Number, mode: AngleMode) -> Number {
    if let Some(result) = try_exact_cos(angle, mode) {
        return result;
    }
    let rad = to_radians(angle, mode);
    let bf = rad.to_approx();
    Number::Approx(bf.cos(crate::bigfloat::precision()))
}

/// 计算角度的正切值（角度处未定义时返回错误）
pub fn tan_number(angle: &Number, mode: AngleMode) -> Result<Number, String> {
    if let Some(result) = try_exact_tan(angle, mode) {
        return Ok(result);
    }
    // 检测精确的 cos = 0（如 tan(pi/2)、tan(90°)），此时未定义
    let cos = cos_number(angle, mode);
    if cos.is_zero() {
        return Err("tan 在此角度未定义".to_string());
    }
    let rad = to_radians(angle, mode);
    let bf = rad.to_approx();
    Ok(Number::Approx(bf.tan(crate::bigfloat::precision())))
}

/// 计算角度的余切值（角度处未定义时返回错误）
pub fn cot_number(angle: &Number, mode: AngleMode) -> Result<Number, String> {
    let sin = sin_number(angle, mode);
    let cos = cos_number(angle, mode);
    if sin.is_zero() {
        return Err("cot 在此角度未定义".to_string());
    }
    Ok(cos.div(&sin))
}

/// 计算角度的正割值（角度处未定义时返回错误）
pub fn sec_number(angle: &Number, mode: AngleMode) -> Result<Number, String> {
    let cos = cos_number(angle, mode);
    if cos.is_zero() {
        return Err("sec 在此角度未定义".to_string());
    }
    Ok(Number::from_int(1).div(&cos))
}

/// 计算角度的余割值（角度处未定义时返回错误）
pub fn csc_number(angle: &Number, mode: AngleMode) -> Result<Number, String> {
    let sin = sin_number(angle, mode);
    if sin.is_zero() {
        return Err("csc 在此角度未定义".to_string());
    }
    Ok(Number::from_int(1).div(&sin))
}

/// 将角度转换为弧度（数值计算用）
fn to_radians(angle: &Number, mode: AngleMode) -> Number {
    match mode {
        AngleMode::Radian => angle.clone(),
        AngleMode::Degree => {
            let pi_180 = BigRational::new(BigInt::from(1), BigInt::from(180));
            let pi_factor = Number::from_pi_times(pi_180);
            angle.mul(&pi_factor)
        }
    }
}

/// 尝试获取精确正弦值
fn try_exact_sin(angle: &Number, mode: AngleMode) -> Option<Number> {
    match mode {
        AngleMode::Degree => {
            if let Some(deg) = extract_degrees(angle) {
                return sin_exact_deg(&deg);
            }
        }
        AngleMode::Radian => {
            if let Some(pi_coeff) = angle.as_pi_multiple() {
                return sin_exact_pi_multiple(&pi_coeff);
            }
        }
    }
    None
}

fn try_exact_cos(angle: &Number, mode: AngleMode) -> Option<Number> {
    match mode {
        AngleMode::Degree => {
            if let Some(deg) = extract_degrees(angle) {
                return cos_exact_deg(&deg);
            }
        }
        AngleMode::Radian => {
            if let Some(pi_coeff) = angle.as_pi_multiple() {
                return cos_exact_pi_multiple(&pi_coeff);
            }
        }
    }
    None
}

fn try_exact_tan(angle: &Number, mode: AngleMode) -> Option<Number> {
    match mode {
        AngleMode::Degree => {
            if let Some(deg) = extract_degrees(angle) {
                return tan_exact_deg(&deg);
            }
        }
        AngleMode::Radian => {
            if let Some(pi_coeff) = angle.as_pi_multiple() {
                return tan_exact_pi_multiple(&pi_coeff);
            }
        }
    }
    None
}

/// 从 Number 中提取角度度数（仅当为整数或有理数时）
fn extract_degrees(angle: &Number) -> Option<BigInt> {
    match angle {
        Number::Exact(expr) => expr.as_integer(),
        Number::Approx(_) | Number::Complex(_) => None,
    }
}

/// 将角度度数归一化到 [0, 360)
fn normalize_deg(deg: &BigInt) -> BigInt {
    let deg360 = BigInt::from(360);
    let mut d = deg % &deg360;
    if d.is_negative() {
        d += &deg360;
    }
    d
}

/// 将角度映射到 [0, 90] 参考角并返回 (参考角, sin符号, cos符号)
fn deg_to_ref(deg: &BigInt) -> (BigInt, bool, bool) {
    let d = normalize_deg(deg);
    let deg90 = BigInt::from(90);
    let deg180 = BigInt::from(180);
    let deg270 = BigInt::from(270);

    if d < deg90 {
        (d.clone(), true, true)
    } else if d < deg180 {
        (deg180 - &d, true, false)
    } else if d < deg270 {
        (&d - deg180, false, false)
    } else {
        (BigInt::from(360) - &d, false, true)
    }
}

/// 精确正弦（度数）
fn sin_exact_deg(deg: &BigInt) -> Option<Number> {
    let (ref_deg, sin_pos, _cos_pos) = deg_to_ref(deg);

    // 特判 0°
    if ref_deg == BigInt::zero() {
        return Some(Number::from_int(0));
    }

    // 查找已知角度的正弦值
    let base_sin = base_sin_deg(&ref_deg)?;
    if sin_pos {
        Some(base_sin)
    } else {
        Some(base_sin.neg())
    }
}

/// 精确余弦（度数）
fn cos_exact_deg(deg: &BigInt) -> Option<Number> {
    let (ref_deg, _sin_pos, cos_pos) = deg_to_ref(deg);

    let base_cos = base_cos_deg(&ref_deg)?;
    if cos_pos {
        Some(base_cos)
    } else {
        Some(base_cos.neg())
    }
}

/// 精确正切（度数）
fn tan_exact_deg(deg: &BigInt) -> Option<Number> {
    let sin = sin_exact_deg(deg)?;
    let cos = cos_exact_deg(deg)?;
    if cos.is_zero() {
        None // 无穷大
    } else {
        Some(sin.div(&cos))
    }
}

/// 已知参考角（0-90°）的正弦精确值
fn base_sin_deg(ref_deg: &BigInt) -> Option<Number> {
    if *ref_deg == BigInt::zero() {
        return Some(Number::from_int(0));
    }
    if *ref_deg == BigInt::from(30) {
        return Some(Number::from_rational(BigRational::new(
            BigInt::from(1),
            BigInt::from(2),
        )));
    }
    if *ref_deg == BigInt::from(45) {
        return Some(make_surd(0, 1, 2, 2)); // sqrt(2)/2
    }
    if *ref_deg == BigInt::from(60) {
        return Some(make_surd(0, 1, 3, 2)); // sqrt(3)/2
    }
    if *ref_deg == BigInt::from(90) {
        return Some(Number::from_int(1));
    }
    if *ref_deg == BigInt::from(15) {
        // sin(15°) = (sqrt(6) - sqrt(2)) / 4
        return Some(make_double_surd(0, 1, 6, -1, 2, 4));
    }
    if *ref_deg == BigInt::from(75) {
        // sin(75°) = (sqrt(6) + sqrt(2)) / 4
        return Some(make_double_surd(0, 1, 6, 1, 2, 4));
    }
    if *ref_deg == BigInt::from(18) {
        // sin(18°) = (sqrt(5) - 1) / 4
        return Some(make_surd(-1, 1, 5, 4));
    }
    if *ref_deg == BigInt::from(54) {
        // sin(54°) = (sqrt(5) + 1) / 4 = cos(36°)
        return Some(make_surd(1, 1, 5, 4));
    }
    None
}

/// 已知参考角（0-90°）的余弦精确值
fn base_cos_deg(ref_deg: &BigInt) -> Option<Number> {
    if *ref_deg == BigInt::zero() {
        return Some(Number::from_int(1));
    }
    if *ref_deg == BigInt::from(30) {
        return Some(make_surd(0, 1, 3, 2)); // sqrt(3)/2
    }
    if *ref_deg == BigInt::from(45) {
        return Some(make_surd(0, 1, 2, 2)); // sqrt(2)/2
    }
    if *ref_deg == BigInt::from(60) {
        return Some(Number::from_rational(BigRational::new(
            BigInt::from(1),
            BigInt::from(2),
        )));
    }
    if *ref_deg == BigInt::from(90) {
        return Some(Number::from_int(0));
    }
    if *ref_deg == BigInt::from(15) {
        // cos(15°) = (sqrt(6) + sqrt(2)) / 4
        return Some(make_double_surd(0, 1, 6, 1, 2, 4));
    }
    if *ref_deg == BigInt::from(75) {
        // cos(75°) = (sqrt(6) - sqrt(2)) / 4
        return Some(make_double_surd(0, 1, 6, -1, 2, 4));
    }
    if *ref_deg == BigInt::from(18) {
        // cos(18°) = sqrt(10+2*sqrt(5))/4 -- 嵌套根号，不精确表示
        return None;
    }
    if *ref_deg == BigInt::from(54) {
        // cos(54°) = sqrt(10-2*sqrt(5))/4 -- 嵌套根号
        return None;
    }
    if *ref_deg == BigInt::from(36) {
        // cos(36°) = (sqrt(5) + 1) / 4
        return Some(make_surd(1, 1, 5, 4));
    }
    None
}

/// 精确正弦（pi 倍数）
fn sin_exact_pi_multiple(coeff: &BigRational) -> Option<Number> {
    let two = BigRational::from_integer(BigInt::from(2));
    let mut m = coeff % &two;
    if m.is_negative() {
        m += &two;
    }

    let one = BigRational::one();
    let half = BigRational::new(BigInt::from(1), BigInt::from(2));

    if m.is_zero() {
        return Some(Number::from_int(0));
    }

    // 判断象限
    let sin_pos = m <= one;
    let ref_m = if m <= half {
        m.clone()
    } else if m <= one {
        one.clone() - &m
    } else if m <= BigRational::new(BigInt::from(3), BigInt::from(2)) {
        &m - one.clone()
    } else {
        two.clone() - &m
    };

    let sin_pos = sin_pos || (m > BigRational::new(BigInt::from(3), BigInt::from(2)));

    let base = base_sin_pi_multiple(&ref_m)?;
    if sin_pos {
        Some(base)
    } else {
        Some(base.neg())
    }
}

fn cos_exact_pi_multiple(coeff: &BigRational) -> Option<Number> {
    // cos(x) = sin(x + pi/2)
    let pi_half = BigRational::new(BigInt::from(1), BigInt::from(2));
    let shifted = coeff + pi_half;
    sin_exact_pi_multiple(&shifted)
}

fn tan_exact_pi_multiple(coeff: &BigRational) -> Option<Number> {
    let sin = sin_exact_pi_multiple(coeff)?;
    let cos = cos_exact_pi_multiple(coeff)?;
    if cos.is_zero() {
        None
    } else {
        Some(sin.div(&cos))
    }
}

fn base_sin_pi_multiple(m: &BigRational) -> Option<Number> {
    if m.is_zero() {
        return Some(Number::from_int(0));
    }

    let one_sixth = BigRational::new(BigInt::from(1), BigInt::from(6));
    let one_quarter = BigRational::new(BigInt::from(1), BigInt::from(4));
    let one_third = BigRational::new(BigInt::from(1), BigInt::from(3));
    let one_half = BigRational::new(BigInt::from(1), BigInt::from(2));
    let one_twelfth = BigRational::new(BigInt::from(1), BigInt::from(12));
    let five_twelfths = BigRational::new(BigInt::from(5), BigInt::from(12));

    if *m == one_sixth {
        // sin(pi/6) = 1/2
        return Some(Number::from_rational(BigRational::new(
            BigInt::from(1),
            BigInt::from(2),
        )));
    }
    if *m == one_quarter {
        // sin(pi/4) = sqrt(2)/2
        return Some(make_surd(0, 1, 2, 2));
    }
    if *m == one_third {
        // sin(pi/3) = sqrt(3)/2
        return Some(make_surd(0, 1, 3, 2));
    }
    if *m == one_half {
        return Some(Number::from_int(1));
    }
    if *m == one_twelfth {
        // sin(pi/12) = (sqrt(6) - sqrt(2)) / 4
        return Some(make_double_surd(0, 1, 6, -1, 2, 4));
    }
    if *m == five_twelfths {
        // sin(5pi/12) = (sqrt(6) + sqrt(2)) / 4
        return Some(make_double_surd(0, 1, 6, 1, 2, 4));
    }
    None
}

/// 构造 surd: (a + b*sqrt(c)) / d
fn make_surd(a: i64, b: i64, c: u64, d: u64) -> Number {
    let a_int = BigInt::from(a);
    let b_int = BigInt::from(b);
    let rad = BigInt::from(c);
    let denom = BigInt::from(d);

    let mut terms = Vec::new();
    if a != 0 {
        terms.push(ExactTerm::Rational(BigRational::from_integer(a_int)));
    }
    if b != 0 {
        terms.push(ExactTerm::Sqrt(BigRational::from_integer(b_int), rad));
    }

    Number::Exact(ExactExpr {
        terms,
        denominator: denom,
    })
}

/// 构造双 surd: (a + b*sqrt(c1) + d*sqrt(c2)) / e
fn make_double_surd(a: i64, b: i64, c1: u64, d: i64, c2: u64, e: u64) -> Number {
    let a_int = BigInt::from(a);
    let b_int = BigInt::from(b);
    let rad1 = BigInt::from(c1);
    let d_int = BigInt::from(d);
    let rad2 = BigInt::from(c2);
    let denom = BigInt::from(e);

    let mut terms = Vec::new();
    if a != 0 {
        terms.push(ExactTerm::Rational(BigRational::from_integer(a_int)));
    }
    if b != 0 {
        terms.push(ExactTerm::Sqrt(BigRational::from_integer(b_int), rad1));
    }
    if d != 0 {
        terms.push(ExactTerm::Sqrt(BigRational::from_integer(d_int), rad2));
    }

    Number::Exact(ExactExpr {
        terms,
        denominator: denom,
    })
}

/// 尝试获取精确 arcsin 值
pub fn try_exact_arcsin(arg: &Number) -> Option<Number> {
    if arg.is_zero() {
        return Some(Number::from_int(0));
    }
    if let Some(rat) = arg.as_rational() {
        // arcsin(±1/2) = ±pi/6
        if rat.numer().abs() == BigInt::from(1) && *rat.denom() == BigInt::from(2) {
            let sign = if rat.is_negative() { -1 } else { 1 };
            return Some(Number::from_pi_times(BigRational::new(
                BigInt::from(sign),
                BigInt::from(6),
            )));
        }
        // arcsin(±1) = ±pi/2
        if rat.numer().abs() == BigInt::from(1) && *rat.denom() == BigInt::from(1) {
            let sign = if rat.is_negative() { -1 } else { 1 };
            return Some(Number::from_pi_times(BigRational::new(
                BigInt::from(sign),
                BigInt::from(2),
            )));
        }
    }
    // sqrt 比例（带负号处理：-sqr(2)/2 等）
    let neg = arg.is_negative();
    let arg_pos = if neg { arg.abs() } else { arg.clone() };
    if is_exact_sqrt_ratio(&arg_pos, 2, 2).is_some() {
        // arcsin(±sqr(2)/2) = ±pi/4
        let sign = if neg { -1 } else { 1 };
        return Some(Number::from_pi_times(BigRational::new(
            BigInt::from(sign),
            BigInt::from(4),
        )));
    }
    if is_exact_sqrt_ratio(&arg_pos, 3, 2).is_some() {
        // arcsin(±sqr(3)/2) = ±pi/3
        let sign = if neg { -1 } else { 1 };
        return Some(Number::from_pi_times(BigRational::new(
            BigInt::from(sign),
            BigInt::from(3),
        )));
    }
    None
}

pub fn try_exact_arccos(arg: &Number) -> Option<Number> {
    if arg.is_zero() {
        return Some(Number::from_pi_times(BigRational::new(
            BigInt::from(1),
            BigInt::from(2),
        )));
    }
    if let Some(rat) = arg.as_rational() {
        // arccos(1) = 0
        if *rat.numer() == BigInt::from(1) && *rat.denom() == BigInt::from(1) {
            return Some(Number::from_int(0));
        }
        // arccos(-1) = pi
        if *rat.numer() == BigInt::from(-1) && *rat.denom() == BigInt::from(1) {
            return Some(Number::from_pi_times(BigRational::one()));
        }
        // arccos(±1/2) = ±区间：arccos(1/2)=pi/3, arccos(-1/2)=2pi/3
        if rat.numer().abs() == BigInt::from(1) && *rat.denom() == BigInt::from(2) {
            return Some(if rat.is_negative() {
                Number::from_pi_times(BigRational::new(BigInt::from(2), BigInt::from(3)))
            } else {
                Number::from_pi_times(BigRational::new(BigInt::from(1), BigInt::from(3)))
            });
        }
    }
    // sqrt 比例（带负号处理）
    let neg = arg.is_negative();
    let arg_pos = if neg { arg.abs() } else { arg.clone() };
    if is_exact_sqrt_ratio(&arg_pos, 2, 2).is_some() {
        // arccos(sqr(2)/2)=pi/4, arccos(-sqr(2)/2)=3pi/4
        return Some(if neg {
            Number::from_pi_times(BigRational::new(BigInt::from(3), BigInt::from(4)))
        } else {
            Number::from_pi_times(BigRational::new(BigInt::from(1), BigInt::from(4)))
        });
    }
    if is_exact_sqrt_ratio(&arg_pos, 3, 2).is_some() {
        // arccos(sqr(3)/2)=pi/6, arccos(-sqr(3)/2)=5pi/6
        return Some(if neg {
            Number::from_pi_times(BigRational::new(BigInt::from(5), BigInt::from(6)))
        } else {
            Number::from_pi_times(BigRational::new(BigInt::from(1), BigInt::from(6)))
        });
    }
    None
}

pub fn try_exact_arctan(arg: &Number) -> Option<Number> {
    if arg.is_zero() {
        return Some(Number::from_int(0));
    }
    if let Some(rat) = arg.as_rational() {
        // arctan(±1) = ±pi/4
        if rat.numer().abs() == BigInt::from(1) && *rat.denom() == BigInt::from(1) {
            let sign = if rat.is_negative() { -1 } else { 1 };
            return Some(Number::from_pi_times(BigRational::new(
                BigInt::from(sign),
                BigInt::from(4),
            )));
        }
    }
    // sqrt 比例（带负号处理：-sqr(3) 等）
    let neg = arg.is_negative();
    let arg_pos = if neg { arg.abs() } else { arg.clone() };
    if is_exact_sqrt_ratio(&arg_pos, 3, 1).is_some() {
        // arctan(±sqr(3)) = ±pi/3
        let sign = if neg { -1 } else { 1 };
        return Some(Number::from_pi_times(BigRational::new(
            BigInt::from(sign),
            BigInt::from(3),
        )));
    }
    None
}

fn is_exact_sqrt_ratio(num: &Number, rad: u64, den: u64) -> Option<()> {
    // 复数不参与三角特殊值识别
    if num.is_complex() {
        return None;
    }
    match num {
        Number::Complex(_) => None,
        Number::Exact(expr) => {
            // 形式1: denom=den, coeff=1, rad=rad → sqrt(rad)/den
            if expr.denominator == BigInt::from(den) && expr.terms.len() == 1 {
                if let ExactTerm::Sqrt(coeff, r) = &expr.terms[0] {
                    if coeff.is_integer()
                        && *coeff.numer() == BigInt::from(1)
                        && *r == BigInt::from(rad)
                    {
                        return Some(());
                    }
                }
            }
            // 形式2: denom=1, coeff=1/den, rad=rad → (1/den)*sqrt(rad) = sqrt(rad)/den
            if expr.denominator == BigInt::one() && expr.terms.len() == 1 {
                if let ExactTerm::Sqrt(coeff, r) = &expr.terms[0] {
                    if *coeff.numer() == BigInt::from(1)
                        && *coeff.denom() == BigInt::from(den)
                        && *r == BigInt::from(rad)
                    {
                        return Some(());
                    }
                }
            }
            None
        }
        Number::Approx(_) => None,
    }
}
