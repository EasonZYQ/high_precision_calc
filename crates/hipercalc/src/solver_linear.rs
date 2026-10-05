use hipercalc_core::number::Number;
use crate::parser::DisplayMode;

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
pub fn gaussian_elimination(
    matrix: &mut Vec<Vec<Number>>,
    vars: &[char],
) -> Option<LinearSolution> {
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
    Some(LinearSolution { values: solution, unique, infinite })
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
