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
//!
//! # 关于 `clippy::needless_range_loop`
//!
//! 消元法（`det` / `inv` / `solve` / `rref`）的核心形态是
//! `a[r][c] = a[r][c].sub(&factor.mul(&a[col][c]))` —— **同时借用两行**。
//! 改成迭代器要 `split_at_mut` 把表拆开、再手动对齐下标，代价是把教科书上的
//! 高斯消元改成一段没人愿意读的指针体操。这里保留下标写法，在本文件关掉这条
//! lint（`trace` 这类单行遍历仍然用迭代器）。
#![allow(clippy::needless_range_loop)]

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
    for (i, row) in m.iter().enumerate() {
        acc = acc.add(&row[i]);
    }
    Ok(acc)
}

/// 行列式：**Bareiss 分数自由消元**（每次消元除以"上一个主元"而不是当前主元）。
///
/// 为什么值得单独实现：
/// - 朴素高斯消元每一步都要 `factor = a[r][col] / pivot`，一共 **O(n²) 次除法**；
/// - Bareiss 的除法是 `a[i][j] = (a[i][j]·a[k][k] − a[i][k]·a[k][j]) / prev`，
///   每个元素**每层只除一次**，而且整数矩阵下这些除法**必然整除**（这正是它的性质），
///   所以中间量不会像高斯消元那样膨胀成巨大的分数。
///
/// 主元为 0 时向下换行（记一次符号）；整列全零则行列式为 0。
pub fn det(m: &Mat) -> Result<Number, String> {
    if rows(m) != cols(m) {
        return Err("只有方阵才有行列式".to_string());
    }
    let n = rows(m);
    if n == 0 {
        return Ok(Number::from_int(1)); // 空矩阵的行列式约定为 1
    }
    let mut a: Mat = m.to_vec();
    let mut sign = 1i64;
    // `prev` 是上一层的主元（第 0 层约定为 1）
    let mut prev = Number::from_int(1);
    for k in 0..n.saturating_sub(1) {
        // 主元为 0 ⇒ 向下找一个非零的换上来（不能"整列全零就判 0"，
        // 那只在**当前列**成立；这里按 Bareiss 的做法：找不到就真为 0）
        if a[k][k].is_zero() {
            let Some(p) = ((k + 1)..n).find(|&r| !a[r][k].is_zero()) else {
                return Ok(Number::from_int(0));
            };
            a.swap(p, k);
            sign = -sign;
        }
        let pivot = a[k][k].clone();
        for i in (k + 1)..n {
            for j in (k + 1)..n {
                let num = a[i][j].mul(&pivot).sub(&a[i][k].mul(&a[k][j]));
                a[i][j] = num.div(&prev);
            }
            // 该列消成 0（后面用不到，但保持矩阵形态清晰）
            a[i][k] = Number::from_int(0);
        }
        prev = pivot;
    }
    let d = a[n - 1][n - 1].clone();
    Ok(if sign < 0 { d.neg() } else { d })
}

/// 逆矩阵（在增广矩阵上做消元；无逆给出明确错误）
pub fn inv(m: &Mat) -> Result<Number, String> {
    if rows(m) != cols(m) {
        return Err("只有方阵才能求逆".to_string());
    }
    let n = rows(m);
    // ⚠️ `identity(n)` 必须**只构造一次**：原来写在闭包里，每行都重建一张 n×n 单位阵
    // ⇒ n 次 O(n²) 的 `Number` 构造（小矩阵看不出来，n 一大就是纯浪费）。
    let id = identity(n);
    let mut a: Mat = m
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let mut row = r.clone();
            row.extend_from_slice(&id[i]);
            row
        })
        .collect();
    for col in 0..n {
        let Some(p) = (col..n).find(|&r| !a[r][col].is_zero()) else {
            return Err("矩阵不可逆（存在全零列）".to_string());
        };
        a.swap(p, col);
        let pivot = a[col][col].clone();
        // 从 `col` 起算：左半边 `c < col` 的列要么是 0、要么是已归一化的单位列，
        // 再除一遍纯属白做（这里原来写的是 `0..2n`，工作量整整多一倍）。
        for c in col..(2 * n) {
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
            for c in col..(2 * n) {
                a[r][c] = a[r][c].sub(&factor.mul(&a[col][c]));
            }
        }
    }
    let out: Mat = a.iter().map(|r| r[n..].to_vec()).collect();
    Ok(Number::Matrix(Box::new(out)))
}

/// 解线性方程组 `A x = b`（Gauss-Jordan）。`b` 可以是行向量或列向量，按顺序取值。
/// 无解 / 无穷多解都给明确错误（不做"给一个特解"这种含糊处理）。
pub fn solve(a: &Mat, b: &Mat) -> Result<Number, String> {
    let n = rows(a);
    if n != cols(a) {
        return Err("解方程组要求系数矩阵是方阵".to_string());
    }
    let rhs: Vec<Number> = b.iter().flatten().cloned().collect();
    if rhs.len() != n {
        return Err(format!("右端向量长度应为 {n}"));
    }
    // 增广矩阵
    let mut m: Mat = a
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let mut row = r.clone();
            row.push(rhs[i].clone());
            row
        })
        .collect();
    for col in 0..n {
        let Some(p) = (col..n).find(|&r| !m[r][col].is_zero()) else {
            return Err("方程组无唯一解（系数矩阵奇异）".to_string());
        };
        m.swap(p, col);
        let pivot = m[col][col].clone();
        for c in col..=n {
            m[col][c] = m[col][c].div(&pivot);
        }
        for r in 0..n {
            if r == col {
                continue;
            }
            let factor = m[r][col].clone();
            if factor.is_zero() {
                continue;
            }
            for c in col..=n {
                m[r][c] = m[r][c].sub(&factor.mul(&m[col][c]));
            }
        }
    }
    Ok(Number::Matrix(Box::new(
        m.iter().map(|r| vec![r[n].clone()]).collect(),
    )))
}

/// 特征多项式的系数（**升幂**：`[c0, c1, …]` 表示 `c0 + c1·λ + …`）。
/// 只做 2×2 与 3×3（闭式），更大的留待将来。
/// 求根放在上层（core 不依赖应用层的多项式求根）。
pub fn char_poly_coeffs(m: &Mat) -> Result<Vec<Number>, String> {
    let n = rows(m);
    if n != cols(m) {
        return Err("只有方阵才有特征多项式".to_string());
    }
    let tr = trace(m)?;
    let d = det(m)?;
    match n {
        // λ² - tr·λ + det
        2 => Ok(vec![d, tr.neg(), Number::from_int(1)]),
        // λ³ - tr·λ² + m2·λ - det，其中 m2 = 所有主子式之和
        3 => {
            let mut m2 = Number::from_int(0);
            for i in 0..3 {
                for j in (i + 1)..3 {
                    // 划去第 i、j 行与列后剩下的一阶主子式，即 2×2 子式
                    let k = 3 - i - j; // 剩下的那个下标
                    let minor = m[i][i].mul(&m[j][j]).sub(&m[i][j].mul(&m[j][i]));
                    let _ = k;
                    m2 = m2.add(&minor);
                }
            }
            Ok(vec![d.neg(), m2, tr.neg(), Number::from_int(1)])
        }
        _ => Err(format!("特征值目前只支持 2×2 与 3×3（当前 {n}×{n}）")),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::format_mathio;

    /// 造矩阵：`mat(&[&[1,2], &[3,4]])`
    fn mat(rows: &[&[i64]]) -> Mat {
        rows.iter()
            .map(|r| r.iter().map(|v| Number::from_int(*v)).collect())
            .collect()
    }

    /// **余子式（Laplace）展开** —— O(n!)，只拿来对拍 n ≤ 5。
    ///
    /// 单独写一份"显然正确但很慢"的实现，是这一对测试的全部价值：
    /// Bareiss 的除法、换行符号一旦写错，只有和它逐项相等才算过。
    fn cofactor(m: &Mat) -> Number {
        let n = m.len();
        if n == 1 {
            return m[0][0].clone();
        }
        let mut acc = Number::from_int(0);
        for j in 0..n {
            if m[0][j].is_zero() {
                continue;
            }
            // 去掉第 0 行、第 j 列
            let minor: Mat = m[1..]
                .iter()
                .map(|row| {
                    row.iter()
                        .enumerate()
                        .filter(|(k, _)| *k != j)
                        .map(|(_, v)| v.clone())
                        .collect()
                })
                .collect();
            let term = m[0][j].mul(&cofactor(&minor));
            acc = if j % 2 == 0 {
                acc.add(&term)
            } else {
                acc.sub(&term)
            };
        }
        acc
    }

    /// 确定性伪随机（不引第三方 RNG）
    fn lcg(s: &mut u64) -> u64 {
        *s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        *s >> 11
    }

    /// **Bareiss 与余子式展开必须逐项相等**（整数矩阵 ⇒ 精确有理数，无误差）
    #[test]
    fn bareiss_matches_cofactor_expansion() {
        let mut seed = 20261031u64;
        for n in 2..=5usize {
            for _ in 0..40 {
                let mut rows: Vec<Vec<i64>> = Vec::with_capacity(n);
                for _ in 0..n {
                    let mut r = Vec::with_capacity(n);
                    for _ in 0..n {
                        r.push((lcg(&mut seed) % 9) as i64 - 4); // -4..=4
                    }
                    rows.push(r);
                }
                let owned: Vec<&[i64]> = rows.iter().map(|r| r.as_slice()).collect();
                let m = mat(&owned);
                let want = cofactor(&m);
                let got = det(&m).unwrap();
                assert_eq!(
                    format_mathio(&got),
                    format_mathio(&want),
                    "n={n} 矩阵 {rows:?} 的行列式不一致"
                );
            }
        }
    }

    /// 主元为 0 时必须**换行**而不是直接判 0；整列全零才是真的 0。
    #[test]
    fn bareiss_swaps_zero_pivot() {
        // 首列首行为 0，但第二行同列非零 ⇒ det = 0*... - 1*... = 1
        let m = mat(&[&[0, 1], &[1, 0]]);
        assert_eq!(format_mathio(&det(&m).unwrap()), "-1");
        // 首列全零 ⇒ det = 0
        let z = mat(&[&[0, 2], &[0, 3]]);
        assert_eq!(format_mathio(&det(&z).unwrap()), "0");
        // 两行相同 ⇒ det = 0（Bareiss 在这里会出现"主元为 0 且下方也为 0"）
        let s = mat(&[&[1, 2], &[1, 2]]);
        assert_eq!(format_mathio(&det(&s).unwrap()), "0");
    }

    /// 三角矩阵：行列式 = 对角线乘积（6×6 也覆盖到 n>5 的路径）
    #[test]
    fn bareiss_triangular_is_product_of_diagonal() {
        let m = mat(&[
            &[2, 3, 1, 4, 5, 6],
            &[0, 3, 7, 1, 2, 2],
            &[0, 0, 4, 5, 1, 1],
            &[0, 0, 0, 5, 3, 2],
            &[0, 0, 0, 0, 6, 7],
            &[0, 0, 0, 0, 0, 7],
        ]);
        assert_eq!(format_mathio(&det(&m).unwrap()), "5040"); // 2·3·4·5·6·7
    }
}
