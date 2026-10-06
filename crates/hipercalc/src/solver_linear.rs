use crate::parser::DisplayMode;
use hipercalc_core::number::Number;

/// 线性方程组求解结果
#[derive(Debug, Clone)]
pub struct LinearSolution {
    /// (变量名, 解)
    pub values: Vec<(char, Number)>,
    /// 是否唯一解
    pub unique: bool,
    /// 是否无穷多解（方程欠定）
    pub infinite: bool,
}

/// 高斯消元法求解线性方程组
/// 输入: 增广矩阵 rows × (vars+1)，变量列表
/// 输出: 解或 None（无解/无穷解）
pub fn gaussian_elimination(matrix: &mut [Vec<Number>], vars: &[char]) -> Option<LinearSolution> {
    let n = matrix.len();
    let m = vars.len();

    if n == 0 || m == 0 {
        return None;
    }

    // 前向消元
    let mut row = 0;
    for col in 0..m {
        if row >= n {
            break;
        }
        // 找主元
        let mut max_row = row;
        for r in row..n {
            if matrix[max_row][col].is_zero() && !matrix[r][col].is_zero() {
                max_row = r;
            }
        }

        if matrix[max_row][col].is_zero() {
            continue;
        }

        matrix.swap(row, max_row);

        // 归一化主元行
        let pivot = matrix[row][col].clone();
        for j in col..=m {
            matrix[row][j] = matrix[row][j].div(&pivot);
        }

        // 消去其他行
        for r in 0..n {
            if r == row {
                continue;
            }
            let factor = matrix[r][col].clone();
            if factor.is_zero() {
                continue;
            }
            for j in col..=m {
                let sub = factor.mul(&matrix[row][j]);
                matrix[r][j] = matrix[r][j].sub(&sub);
            }
        }

        row += 1;
    }

    // 检查无解
    for r in 0..n {
        let mut all_zero = true;
        for c in 0..m {
            if !matrix[r][c].is_zero() {
                all_zero = false;
                break;
            }
        }
        if all_zero && !matrix[r][m].is_zero() {
            return None;
        }
    }

    // 计算系数矩阵的秩：若秩小于变量数，则方程欠定，有无穷多解
    let mut rank = 0;
    for r in 0..n {
        let mut all_zero = true;
        for c in 0..m {
            if !matrix[r][c].is_zero() {
                all_zero = false;
                break;
            }
        }
        if !all_zero {
            rank += 1;
        }
    }
    let infinite = rank < m;

    // 提取解
    let mut solution = Vec::new();
    for c in 0..m {
        let mut found = None;
        for r in 0..n {
            if !matrix[r][c].is_zero() {
                if found.is_some() {
                    found = None;
                    break;
                }
                found = Some(r);
            }
        }
        if let Some(r) = found {
            solution.push((vars[c], matrix[r][m].clone()));
        }
    }

    let unique = solution.len() == m && n >= m && !infinite;
    Some(LinearSolution {
        values: solution,
        unique,
        infinite,
    })
}

/// 格式化线性方程组解。
/// 按显示模式输出，并给每项加上 `=`/`≈` 精确度标记（与单方程 `print_solutions` 一致）：
/// MathIO 下精确值给 `=`；LineIO 下只有能完整显示的有理数才给 `=`，其余为 `≈`。
pub fn format_linear_solution(sol: &LinearSolution, mode: DisplayMode) -> String {
    if sol.values.is_empty() {
        return "无解".to_string();
    }
    let parts: Vec<String> = sol
        .values
        .iter()
        .map(|(v, n)| {
            let s = match mode {
                DisplayMode::MathIO => hipercalc_core::display::format_mathio(n),
                DisplayMode::LineIO => hipercalc_core::display::format_lineio(n),
            };
            format!("{} {} {}", v, crate::solve_aux::result_prefix(n, mode), s)
        })
        .collect();
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use hipercalc_core::number::Number;

    /// 用整数构造增广矩阵（每行是"移项后 = 0"的系数，最后一项为常数项）
    fn mat(rows: &[&[i64]]) -> Vec<Vec<Number>> {
        rows.iter()
            .map(|r| r.iter().map(|&v| Number::from_int(v)).collect())
            .collect()
    }

    #[test]
    fn unique_2x2_solution_is_exact() {
        // x + y = 5, 2x - y = 1：两式相加 3x = 6 ⇒ x = 2，回代 y = 3；代入第二式 2*2-3 = 1 ✓
        let mut m = mat(&[&[1, 1, 5], &[2, -1, 1]]);
        let sol = gaussian_elimination(&mut m, &['x', 'y']).expect("应有唯一解");
        assert!(sol.unique && !sol.infinite);
        assert_eq!(
            format_linear_solution(&sol, crate::parser::DisplayMode::MathIO),
            "x = 2, y = 3"
        );
    }

    #[test]
    fn inconsistent_system_has_no_solution() {
        // x + y = 1 与 x + y = 2 互相矛盾 ⇒ 无解
        let mut m = mat(&[&[1, 1, 1], &[1, 1, 2]]);
        assert!(gaussian_elimination(&mut m, &['x', 'y']).is_none());
    }

    #[test]
    fn dependent_rows_are_infinite() {
        // 2x + 2y = 2 是 x + y = 1 的 2 倍 ⇒ 秩 1 < 未知数 2，无穷多解
        let mut m = mat(&[&[1, 1, 1], &[2, 2, 2]]);
        let sol = gaussian_elimination(&mut m, &['x', 'y']).expect("有解（无穷多）");
        assert!(sol.infinite, "应标记无穷多解");
    }

    #[test]
    fn fewer_equations_than_unknowns_is_infinite() {
        // 只有 1 个方程、2 个未知数 ⇒ 必然欠定
        let mut m = mat(&[&[1, 1, 1]]);
        let sol = gaussian_elimination(&mut m, &['x', 'y']).expect("有解（欠定）");
        assert!(sol.infinite);
    }

    #[test]
    fn fraction_solution_follows_display_mode() {
        // 2x = 1 ⇒ x = 1/2：MathIO 给精确分式，LineIO 给小数
        let mut m = mat(&[&[2, 1]]);
        let sol = gaussian_elimination(&mut m, &['x']).unwrap();
        assert_eq!(
            format_linear_solution(&sol, crate::parser::DisplayMode::MathIO),
            "x = 1 / 2"
        );
        assert_eq!(
            format_linear_solution(&sol, crate::parser::DisplayMode::LineIO),
            "x = 0.5"
        );
    }
}
