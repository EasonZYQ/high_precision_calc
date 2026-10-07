//! 矩阵（`Number::Matrix`）的运算与线性代数。
//!
//! # 设计约定（与 `number.rs` 里矩阵变体的说明一致）
//!
//! - 元素仍是 `Number` ⇒ **精确矩阵、复数矩阵自动成立**（整数矩阵的行列式给精确值）；
//! - 只有**合法组合**在这里实现：矩阵 ± 矩阵（同形逐元素）、矩阵 × 矩阵、数 × 矩阵、矩阵 × 数；
//!   非法组合（数 + 矩阵、行列不匹配）**由上层（求值器）预检报错** ——
//!   因为 `Number::add/mul` 的签名是无 `Result` 的，没法在这里报错；
//! - 消元取**第一个非零主元**而不是最大主元：精确算术下不需要数值稳定性，
//!   只需要避免"零主元做除数"；这也避开了 `Number` 没有 `Ord` 的问题。
//!   （代价：近似矩阵的数值稳定性一般，已在注释里说明。）

use crate::number::Number;

type Mat = Vec<Vec<Number>>;

/// 行数 / 列数
pub fn rows(m: &Mat) -> usize {
    m.len()
}
pub fn cols(m: &Mat) -> usize {
    m.first().map(|r| r.len()).unwrap_or(0)
}

/// 同形矩阵的逐元素运算
fn zip_map(a: &Mat, b: &Mat, f: impl Fn(&Number, &Number) -> Number) -> Mat {
    a.iter()
        .zip(b.iter())
        .map(|(ra, rb)| ra.iter().zip(rb.iter()).map(|(x, y)| f(x, y)).collect())
        .collect()
}

fn scale(m: &Mat, k: &Number) -> Mat {
    m.iter()
        .map(|r| r.iter().map(|x| x.mul(k)).collect())
        .collect()
}

/// 矩阵加法（同形）
pub fn add(a: &Mat, b: &Mat) -> Mat {
    zip_map(a, b, |x, y| x.add(y))
}

/// 矩阵减法（同形）
pub fn sub(a: &Mat, b: &Mat) -> Mat {
    zip_map(a, b, |x, y| x.sub(y))
}

/// 矩阵乘法（a 的列数 = b 的行数）
pub fn matmul(a: &Mat, b: &Mat) -> Mat {
    let (n, p, q) = (rows(a), cols(b), cols(a));
    (0..n)
        .map(|i| {
            (0..p)
                .map(|j| {
                    let mut acc = Number::from_int(0);
                    for k in 0..q {
                        acc = acc.add(&a[i][k].mul(&b[k][j]));
                    }
                    acc
                })
                .collect()
        })
        .collect()
}

/// `Number::add/sub/mul` 的矩阵分发：只处理合法组合。
/// 非法组合返回左侧原值（上层预检应当先报错），并用 debug_assert 暴露漏检。
pub fn binary_add(a: &Number, b: &Number) -> Number {
    match (a.as_matrix(), b.as_matrix()) {
        (Some(x), Some(y)) if rows(x) == rows(y) && cols(x) == cols(y) => {
            Number::Matrix(Box::new(add(x, y)))
        }
        _ => unrecoverable("矩阵加法要求两边同形，或由上层预检报错"),
    }
}

pub fn binary_sub(a: &Number, b: &Number) -> Number {
    match (a.as_matrix(), b.as_matrix()) {
        (Some(x), Some(y)) if rows(x) == rows(y) && cols(x) == cols(y) => {
            Number::Matrix(Box::new(sub(x, y)))
        }
        _ => unrecoverable("矩阵减法要求两边同形，或由上层预检报错"),
    }
}

/// 乘法三态：矩阵×矩阵（矩阵乘）、数×矩阵、矩阵×数（数乘）
pub fn binary_mul(a: &Number, b: &Number) -> Number {
    match (a.as_matrix(), b.as_matrix()) {
        (Some(x), Some(y)) if cols(x) == rows(y) => Number::Matrix(Box::new(matmul(x, y))),
        (Some(x), None) => Number::Matrix(Box::new(scale(x, b))),
        (None, Some(y)) => Number::Matrix(Box::new(scale(y, a))),
        _ => unrecoverable("矩阵乘法要求 a 的列数等于 b 的行数，或由上层预检报错"),
    }
}

fn unrecoverable(msg: &str) -> Number {
    debug_assert!(false, "{msg}");
    Number::from_int(0)
}

/// 矩阵的整数次幂（`n = 0` 给单位阵；`n < 0` 走逆矩阵）——由 `Number::int_pow` 调用
pub fn int_pow(m: &Number, exp: u32) -> Result<Number, String> {
    let mat = m.as_matrix().ok_or_else(|| "矩阵幂需要矩阵".to_string())?;
    if rows(mat) != cols(mat) {
        return Err("只有方阵才能做幂运算".to_string());
    }
    // 快速幂
    let mut acc = identity(rows(mat));
    let mut base = mat.clone();
    let mut e = exp;
    while e > 0 {
        if e & 1 == 1 {
            acc = matmul(&acc, &base);
        }
        e >>= 1;
        if e > 0 {
            base = matmul(&base, &base);
        }
    }
    Ok(Number::Matrix(Box::new(acc)))
}

/// 单位阵
pub fn identity(n: usize) -> Mat {
    (0..n)
        .map(|i| {
            (0..n)
                .map(|j| {
                    if i == j {
                        Number::from_int(1)
                    } else {
                        Number::from_int(0)
                    }
                })
                .collect()
        })
        .collect()
}

/// 转置
pub fn transpose(m: &Mat) -> Mat {
    let (n, p) = (rows(m), cols(m));
    (0..p)
        .map(|j| (0..n).map(|i| m[i][j].clone()).collect())
        .collect()
}

/// 迹（方阵对角线和）
pub fn trace(m: &Mat) -> Result<Number, String> {
    if rows(m) != cols(m) {
        return Err("只有方阵才有迹".to_string());
    }
    let mut acc = Number::from_int(0);
    for i in 0..rows(m) {
        acc = acc.add(&m[i][i]);
    }
    Ok(acc)
}

/// 行列式（高斯消元 + 行交换符号；第一个非零主元）
pub fn det(m: &Mat) -> Result<Number, String> {
    if rows(m) != cols(m) {
        return Err("只有方阵才有行列式".to_string());
    }
    let n = rows(m);
    let mut a: Mat = m.to_vec();
    let mut sign = 1i64;
    let mut acc = Number::from_int(1);
    for col in 0..n {
        // 找第一个非零主元
        let Some(p) = (col..n).find(|&r| !a[r][col].is_zero()) else {
            return Ok(Number::from_int(0)); // 该列全零 ⇒ 行列式为 0
        };
        if p != col {
            a.swap(p, col);
            sign = -sign;
        }
        let pivot = a[col][col].clone();
        acc = acc.mul(&pivot);
        for r in (col + 1)..n {
            let factor = a[r][col].div(&pivot);
            for c in col..n {
                a[r][c] = a[r][c].sub(&factor.mul(&a[col][c]));
            }
        }
    }
    Ok(if sign < 0 { acc.neg() } else { acc })
}

/// 逆矩阵（在增广矩阵上做消元；无逆给出明确错误）
pub fn inv(m: &Mat) -> Result<Number, String> {
    if rows(m) != cols(m) {
        return Err("只有方阵才能求逆".to_string());
    }
    let n = rows(m);
    let mut a: Mat = m
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let mut row = r.clone();
            row.extend_from_slice(&identity(n)[i]);
            row
        })
        .collect();
    for col in 0..n {
        let Some(p) = (col..n).find(|&r| !a[r][col].is_zero()) else {
            return Err("矩阵不可逆（存在全零列）".to_string());
        };
        a.swap(p, col);
        let pivot = a[col][col].clone();
        for c in 0..(2 * n) {
            a[col][c] = a[col][c].div(&pivot);
        }
        for r in 0..n {
            if r == col {
                continue;
            }
            let factor = a[r][col].clone();
            if factor.is_zero() {
                continue;
            }
            for c in 0..(2 * n) {
                a[r][c] = a[r][c].sub(&factor.mul(&a[col][c]));
            }
        }
    }
    let out: Mat = a.iter().map(|r| r[n..].to_vec()).collect();
    Ok(Number::Matrix(Box::new(out)))
}

/// 秩（行阶梯化，数非零行）
pub fn rank(m: &Mat) -> usize {
    let mut a = m.to_vec();
    let (n, p) = (rows(&a), cols(&a));
    let mut r = 0usize;
    for col in 0..p {
        let Some(pv) = (r..n).find(|&i| !a[i][col].is_zero()) else {
            continue;
        };
        a.swap(pv, r);
        let pivot = a[r][col].clone();
        for i in (r + 1)..n {
            let factor = a[i][col].div(&pivot);
            for c in col..p {
                a[i][c] = a[i][c].sub(&factor.mul(&a[r][c]));
            }
        }
        r += 1;
        if r == n {
            break;
        }
    }
    r
}

/// 渲染成 `[[1, 2], [3, 4]]`（元素用调用方给的格式化函数，避免重复缩放/加单位）
pub fn render(m: &Mat, fmt: &dyn Fn(&Number) -> String) -> String {
    let body = m
        .iter()
        .map(|r| {
            let cells: Vec<String> = r.iter().map(fmt).collect();
            format!("[{}]", cells.join(", "))
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{body}]")
}
