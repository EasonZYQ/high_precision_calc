use crate::parser::Expr;

/// 方程信息
pub struct EquationInfo {
    /// 变量列表（按字母顺序）
    pub variables: Vec<char>,
}

impl EquationInfo {
    /// 从表达式树中提取所有变量
    pub fn extract(expr: &Expr) -> Self {
        let mut vars = Vec::new();
        extract_vars(expr, &mut vars);
        vars.sort();
        vars.dedup();
        EquationInfo { variables: vars }
    }
}

fn extract_vars(expr: &Expr, vars: &mut Vec<char>) {
    match expr {
        Expr::Variable(name) => {
            if name != "ans" && name.len() == 1 {
                vars.push(name.chars().next().unwrap());
            }
        }
        Expr::Binary(left, _, right) => {
            extract_vars(left, vars);
            extract_vars(right, vars);
        }
        Expr::Unary(_, e) => extract_vars(e, vars),
        Expr::Pow(base, exp) => {
            extract_vars(base, vars);
            extract_vars(exp, vars);
        }
        Expr::Function(_, args) => {
            for arg in args {
                extract_vars(arg, vars);
            }
        }
        Expr::Sd(inner) => extract_vars(inner, vars),
        Expr::Factor(inner) => extract_vars(inner, vars),
        Expr::Equation(left, right) => {
            extract_vars(left, vars);
            extract_vars(right, vars);
        }
        Expr::Number(_) => {} // 无变量
        Expr::System(_) => {}
    }
}

/// 判断方程是否为多项式（仅含常数、变量、+、-、*、^整数 的组合；除以常数也允许）
pub fn is_polynomial(expr: &Expr) -> bool {
    match expr {
        Expr::Number(_) => true,
        Expr::Variable(_) => true,
        Expr::Binary(left, op, right) => {
            use crate::parser::BinOp;
            match op {
                BinOp::Add | BinOp::Sub | BinOp::Mul => is_polynomial(left) && is_polynomial(right),
                // 除以非零常数仍为多项式（如 x/2）
                BinOp::Div => is_polynomial(left) && is_constant_expr(right),
            }
        }
        Expr::Unary(_, e) => is_polynomial(e),
        Expr::Pow(base, exp) => {
            if !is_polynomial(base) {
                return false;
            }
            // 指数必须是常数整数
            is_constant_integer(exp)
        }
        Expr::Function(_, _) => false,
        Expr::Sd(_) => false,
        Expr::Factor(inner) => is_polynomial(inner),
        Expr::Equation(left, right) => is_polynomial(left) && is_polynomial(right),
        Expr::System(eqs) => eqs.iter().all(is_polynomial),
    }
}

/// 是否为不依赖变量的常数表达式
fn is_constant_expr(expr: &Expr) -> bool {
    match expr {
        Expr::Number(_) => true,
        Expr::Unary(op, e) => {
            matches!(
                op,
                crate::parser::UnaryOp::Pos | crate::parser::UnaryOp::Neg
            ) && is_constant_expr(e)
        }
        _ => false,
    }
}

/// 检查表达式是否为常整数（可求值且不含变量）
fn is_constant_integer(expr: &Expr) -> bool {
    match expr {
        Expr::Number(n) => n.as_rational().map(|r| r.is_integer()).unwrap_or(false),
        Expr::Unary(_, e) => is_constant_integer(e),
        _ => false,
    }
}

/// 判断是否为线性方程（多项式且变量次数 ≤ 1）
pub fn is_linear(expr: &Expr) -> bool {
    if !is_polynomial(expr) {
        return false;
    }
    check_linear(expr)
}

fn check_linear(expr: &Expr) -> bool {
    match expr {
        Expr::Number(_) => true,
        Expr::Variable(_) => true,
        Expr::Binary(left, op, right) => {
            if !check_linear(left) || !check_linear(right) {
                return false;
            }
            match op {
                crate::parser::BinOp::Mul => {
                    // 乘积中最多一个含变量
                    let l_has_var = has_variable(left);
                    let r_has_var = has_variable(right);
                    if l_has_var && r_has_var {
                        false // x*y 不是线性
                    } else {
                        true
                    }
                }
                _ => true, // +, -, / 已排除
            }
        }
        Expr::Unary(_, e) => check_linear(e),
        Expr::Pow(base, exp) => {
            // x^1 是线性
            if let Expr::Variable(_) = base.as_ref() {
                is_constant_integer(exp)
                    && match exp.as_ref() {
                        Expr::Number(n) => n
                            .as_rational()
                            .map(|r| {
                                *r.numer() == num_bigint::BigInt::from(1u32)
                                    && *r.denom() == num_bigint::BigInt::from(1u32)
                            })
                            .unwrap_or(false),
                        _ => false,
                    }
            } else {
                false
            }
        }
        Expr::Function(_, _) => false,
        Expr::Sd(_) => false,
        Expr::Factor(inner) => check_linear(inner),
        Expr::Equation(l, r) => check_linear(l) && check_linear(r),
        Expr::System(eqs) => eqs.iter().all(check_linear),
    }
}

fn has_variable(expr: &Expr) -> bool {
    match expr {
        Expr::Variable(name) => name != "ans",
        Expr::Binary(left, _, right) => has_variable(left) || has_variable(right),
        Expr::Unary(_, e) => has_variable(e),
        Expr::Pow(base, exp) => has_variable(base) || has_variable(exp),
        Expr::Function(_, args) => args.iter().any(has_variable),
        Expr::Sd(inner) => has_variable(inner),
        Expr::Factor(inner) => has_variable(inner),
        Expr::Equation(l, r) => has_variable(l) || has_variable(r),
        Expr::System(eqs) => eqs.iter().any(has_variable),
        Expr::Number(_) => false,
    }
}

/// 前向三角函数：只有它们才把实参当作**角度**（反三角的实参是比值，不是角度）
const FORWARD_TRIG: [&str; 6] = ["sin", "cos", "tan", "cot", "sec", "csc"];

/// 表达式里是否出现"以该变量为角度的前向三角函数"。
///
/// 用途：解方程时 **自变量是否代表角度** 只由这一点决定 ——
/// 只有这种方程，根收集的初值才需要按当前角度单位给出
/// （`sin(x)=0.5` 在度模式下的根是 `30, 150, …`；而 `1/x=2` 的根永远是 `0.5`）。
/// 反三角（`arcsin` 等）返回的才是角度，其实参是比值 ⇒ 不算。
pub fn has_trig_of_var(expr: &Expr, var: char) -> bool {
    match expr {
        Expr::Function(name, args) => {
            if FORWARD_TRIG.contains(&name.as_str()) && args.iter().any(|a| references_var(a, var))
            {
                return true;
            }
            args.iter().any(|a| has_trig_of_var(a, var))
        }
        Expr::Binary(left, _, right) => has_trig_of_var(left, var) || has_trig_of_var(right, var),
        Expr::Unary(_, e) => has_trig_of_var(e, var),
        Expr::Pow(base, exp) => has_trig_of_var(base, var) || has_trig_of_var(exp, var),
        Expr::Sd(inner) => has_trig_of_var(inner, var),
        Expr::Factor(inner) => has_trig_of_var(inner, var),
        Expr::Equation(l, r) => has_trig_of_var(l, var) || has_trig_of_var(r, var),
        Expr::System(eqs) => eqs.iter().any(|e| has_trig_of_var(e, var)),
        Expr::Number(_) | Expr::Variable(_) => false,
    }
}

/// 子树是否引用了指定变量（`ans` 与存储变量不算）
fn references_var(expr: &Expr, var: char) -> bool {
    match expr {
        Expr::Variable(name) => name.len() == 1 && name.starts_with(var),
        Expr::Binary(left, _, right) => references_var(left, var) || references_var(right, var),
        Expr::Unary(_, e) => references_var(e, var),
        Expr::Pow(base, exp) => references_var(base, var) || references_var(exp, var),
        Expr::Function(_, args) => args.iter().any(|a| references_var(a, var)),
        Expr::Sd(inner) => references_var(inner, var),
        Expr::Factor(inner) => references_var(inner, var),
        Expr::Equation(l, r) => references_var(l, var) || references_var(r, var),
        Expr::System(eqs) => eqs.iter().any(|e| references_var(e, var)),
        Expr::Number(_) => false,
    }
}
