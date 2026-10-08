//! 量纲（dimensional analysis）：**值 + 7 个 SI 基本量纲的指数**。
//!
//! # 为什么量纲要进值本身
//!
//! 第 6 项做的"单位后缀"是**解析期把 `3 km` 折成 `3000` 并丢掉单位** ——
//! 它能算数，但算不出"`3 km / 2 s = 1.5 m/s`"，也**拦不住**"`3 km + 2 s`"这种量纲不匹配的加法。
//! 真正有用的量纲系统必须让量纲**跟着值走** ⇒ 与矩阵一样，进 `Number` 成为一等值
//! （于是 `/let` 变量、`ans`、显示、持久化都会自动可用）。
//!
//! # 表示
//!
//! 指数用**定长的 `[i8; 7]`**（顺序：m, kg, s, A, K, mol, cd）：
//! - `Copy` + `Eq` ⇒ 比较/拷贝零开销，判断"量纲是否相同"就是数组比较；
//! - 与矩阵的元素类型一样，**不引入新的分配**；
//! - `i8` 的量程（±127）对物理量绰绰有余。
//!
//! # 运算规则（由 `Number` 的运算统一分发）
//!
//! | 运算 | 量纲行为 |
//! |---|---|
//! | `×` | 指数**相加**（`km × s = m·s`）|
//! | `÷` | 指数**相减**（`km ÷ s = m/s`）|
//! | `+` `−` | 指数必须**完全相同**，否则报错（`3 km + 2 s` 拒绝）|
//! | `^n`（整数）| 指数**乘以 n**（`m^2`）|
//! | 其它函数 | 不接受带量纲的值（`sin(3 km)` 报错），由各函数的参数检查拦下 |
//!
//! 加法/减法的"量纲必须相同"是**硬规则**：它正是量纲系统存在的理由，
//! 所以这条检查不能省。因为 `Number::add` 的签名没有 `Result`，
//! 该检查与矩阵的组合检查一样，放在**求值器的预检**里。

/// 带量纲的量：`value` 是**折成 SI 之后的数值部分**，`dim` 是量纲。
///
/// 例如 `3 km` 表示为 `Quantity { value: 3000, dim: LEN }`。
/// 放在 core 里与 `Number` 同层：`Number::Quantity` 只是它的一个包装，
/// 于是"量纲跟着值走"这点在类型上就成立了。
#[derive(Debug, Clone)]
pub struct Quantity {
    pub value: crate::number::Number,
    pub dim: Dim,
}

impl Quantity {
    pub fn new(value: crate::number::Number, dim: Dim) -> Self {
        Quantity { value, dim }
    }
}

/// SI 基本量纲的指数向量，顺序固定为：**m, kg, s, A, K, mol, cd**
pub type Dim = [i8; 7];

/// 无量纲
pub const DIMLESS: Dim = [0; 7];

// ── 常用量纲常量（顺序：m, kg, s, A, K, mol, cd）──
/// 长度 L
pub const LEN: Dim = [1, 0, 0, 0, 0, 0, 0];
/// 面积 L²
pub const AREA: Dim = [2, 0, 0, 0, 0, 0, 0];
/// 体积 L³
pub const VOLUME: Dim = [3, 0, 0, 0, 0, 0, 0];
/// 质量 M
pub const MASS: Dim = [0, 1, 0, 0, 0, 0, 0];
/// 时间 T
pub const TIME: Dim = [0, 0, 1, 0, 0, 0, 0];
/// 速度 L/T
pub const SPEED: Dim = [1, 0, -1, 0, 0, 0, 0];
/// 能量 M·L²/T²（焦耳）
pub const ENERGY: Dim = [2, 1, -2, 0, 0, 0, 0];
/// 压强 M/(L·T²)（帕斯卡）
pub const PRESSURE: Dim = [-1, 1, -2, 0, 0, 0, 0];

/// 量纲是否相同
pub fn same(a: &Dim, b: &Dim) -> bool {
    a == b
}

/// 乘/除：指数相加/相减
pub fn combine(a: &Dim, b: &Dim, sign: i8) -> Dim {
    let mut out = *a;
    for i in 0..7 {
        out[i] = out[i].saturating_add(sign.saturating_mul(b[i]));
    }
    out
}

/// 整数次幂：指数乘以 n
pub fn pow(d: &Dim, n: i32) -> Dim {
    let mut out = *d;
    for i in 0..7 {
        out[i] = (out[i] as i32).saturating_mul(n).clamp(-128, 127) as i8;
    }
    out
}

/// 量纲是否为"无量纲"（所有指数为 0）
pub fn is_dimensionless(d: &Dim) -> bool {
    *d == DIMLESS
}

/// 有没有带量纲（供各函数判断"这个值不该进数值函数"）
pub fn has_dimension(d: &Dim) -> bool {
    !is_dimensionless(d)
}

/// 把量纲渲染成符号形式：`m`、`m/s`、`m/s^2`、`kg*m^2/s^2`
///
/// 只用于显示；正指数排前面、负指数排分母，指数为 1 时省略 `^1`。
pub fn render(d: &Dim) -> String {
    const NAMES: [&str; 7] = ["m", "kg", "s", "A", "K", "mol", "cd"];
    let num: Vec<String> = (0..7)
        .filter(|&i| d[i] > 0)
        .map(|i| {
            if d[i] == 1 {
                NAMES[i].to_string()
            } else {
                format!("{}^{}", NAMES[i], d[i])
            }
        })
        .collect();
    let den: Vec<String> = (0..7)
        .filter(|&i| d[i] < 0)
        .map(|i| {
            if d[i] == -1 {
                NAMES[i].to_string()
            } else {
                format!("{}^{}", NAMES[i], -d[i])
            }
        })
        .collect();
    let head = if num.is_empty() {
        "1".to_string()
    } else {
        num.join("*")
    };
    if den.is_empty() {
        head
    } else {
        format!("{head}/{}", den.join("*"))
    }
}

/// 量纲值的二元运算规则（由 `Number` 的运算经 `binary_op` 分发到这里）。
///
/// - `Add`/`Sub`：**要求量纲相同**（上层预检已保证；这里只做保守兜底）
/// - `Mul`/`Div`：指数相加/相减；其中一边是**普通标量**时量纲不变（标量视作无量纲）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
}

/// 量纲值与（可能是标量的）另一值的运算
pub fn binary(
    a: &crate::number::Number,
    b: &crate::number::Number,
    op: Op,
) -> crate::number::Number {
    use crate::number::Number;
    let (qa, qb) = (a.as_quantity(), b.as_quantity());
    let mk = |v: Number, d: Dim| Number::Quantity(Box::new(Quantity::new(v, d)));
    match (qa, qb) {
        // 量 ± 量：量纲须相同（预检保证）
        (Some(x), Some(y)) => match op {
            Op::Add => mk(x.value.add(&y.value), x.dim),
            Op::Sub => mk(x.value.sub(&y.value), x.dim),
            Op::Mul => mk(x.value.mul(&y.value), combine(&x.dim, &y.dim, 1)),
            Op::Div => mk(x.value.div(&y.value), combine(&x.dim, &y.dim, -1)),
        },
        // 量 × 标量 / 量 ÷ 标量：量纲不变（标量视作无量纲）
        (Some(x), None) => mk(x.value.mul(b), x.dim),
        // 标量 × 量：量纲不变
        (None, Some(y)) => mk(a.mul(&y.value), y.dim),
        (None, None) => unreachable!("binary 只应在至少一侧为量纲值时调用"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dim_arithmetic() {
        let l = [1, 0, 0, 0, 0, 0, 0]; // m
        let t = [0, 0, 1, 0, 0, 0, 0]; // s
        assert_eq!(combine(&l, &t, 1), [1, 0, 1, 0, 0, 0, 0]); // m*s
        assert_eq!(combine(&l, &t, -1), [1, 0, -1, 0, 0, 0, 0]); // m/s
        assert_eq!(pow(&l, 2), [2, 0, 0, 0, 0, 0, 0]); // m^2
        assert!(same(&l, &l) && !same(&l, &t));
        assert!(is_dimensionless(&DIMLESS) && has_dimension(&l));
    }

    #[test]
    fn dim_rendering() {
        assert_eq!(render(&[1, 0, 0, 0, 0, 0, 0]), "m");
        assert_eq!(render(&[1, 0, -1, 0, 0, 0, 0]), "m/s");
        assert_eq!(render(&[1, 0, -2, 0, 0, 0, 0]), "m/s^2");
        assert_eq!(render(&[2, 1, -2, 0, 0, 0, 0]), "m^2*kg/s^2");
        assert_eq!(render(&DIMLESS), "1");
    }
}
