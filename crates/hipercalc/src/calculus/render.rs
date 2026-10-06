//! 把 `Expr` 渲染成字符串（供符号结果、泰勒展开等显示）。
//!
//! # 风格
//!
//! 与 `fac`/`factor` 的输出风格**必须一致**（那套由 `solver_factor` 生成，两处各自维护、
//! 改动时要同步）。实测对齐的样式：
//!
//! | 情形 | 输出 |
//! |---|---|
//! | 数字系数 × 原子 | `2*x`、`2*x^2`（无空格） |
//! | 系数或因子是"组" | `1 / 8 * (2*x - 1)`、`(x - 2) * (x + 2)`（两侧空格） |
//! | 和差 | `x^2 + 2*x + 4`、`x - 2` |
//! | 幂 | `x^2`、`(x + 1)^2` |
//! | 函数 | `sin(x)`、`sin(x)^2`（不用 `sin^2(x)`） |
//! | 绝对值 | `abs(x)`（不用 `|x|`） |
//!
//! 数字本身按显示模式走 `display::format_mathio` / `format_lineio`。

use crate::parser::{BinOp, DisplayMode, Expr, UnaryOp};

/// 渲染优先级（越大越"紧"）
const P_ADD: u8 = 1;
const P_MUL: u8 = 2;
const P_UNARY: u8 = 3;
const P_POW: u8 = 4;
const P_ATOM: u8 = 5;

/// 表达式 → 字符串
pub fn render_expr(e: &Expr, mode: DisplayMode) -> String {
    render_prec(e, mode, 0)
}

/// 渲染子式；自身优先级低于 `parent` 时加括号
fn render_prec(e: &Expr, mode: DisplayMode, parent: u8) -> String {
    let (text, own) = render_own(e, mode);
    if own < parent {
        format!("({})", text)
    } else {
        text
    }
}

/// 是否是"原子"（渲染出来是一个不可再分的 token，可与数字系数紧贴成 `2*x^3`）
fn is_atom(e: &Expr) -> bool {
    matches!(
        e,
        Expr::Number(_) | Expr::Variable(_) | Expr::Function(_, _) | Expr::Pow(_, _)
    )
}

/// 幂指数：只有"单纯 token"才不加括号（`x^1 / 2` 会被读成 `(x^1)/2`，必须避免）
fn render_exponent(e: &Expr, mode: DisplayMode) -> String {
    let ok = match e {
        Expr::Variable(_) | Expr::Function(_, _) => true,
        Expr::Number(n) => !n.is_negative() && !render_number(n, mode).contains(' '),
        Expr::Pow(_, _) => true,
        _ => false,
    };
    // 内层按最低优先级渲染（括号由本函数统一加，避免出现 `x^((1 + 1))`）
    let text = render_prec(e, mode, P_ADD);
    if ok { text } else { format!("({})", text) }
}

fn render_number(n: &hipercalc_core::number::Number, mode: DisplayMode) -> String {
    match mode {
        DisplayMode::MathIO => hipercalc_core::display::format_mathio(n),
        DisplayMode::LineIO => hipercalc_core::display::format_lineio(n),
    }
}

/// 渲染幂：指数 1/2、1/3 走根式函数，其余走 `^`
fn render_pow(base: &Expr, exp: &Expr, mode: DisplayMode) -> String {
    if let Expr::Number(n) = exp
        && let Some(r) = n.as_rational()
    {
        let half = num_rational_half(&r);
        if half == Some(2) {
            return format!("sqrt({})", render_expr(base, mode));
        }
        if half == Some(3) {
            return format!("cbrt({})", render_expr(base, mode));
        }
    }
    // 幂底本身是幂时必须加括号：`x^2^3` 有歧义（应为 `(x^2)^3`）
    let base_prec = if matches!(base, Expr::Pow(_, _)) {
        P_POW + 1
    } else {
        P_POW
    };
    format!(
        "{}^{}",
        render_prec(base, mode, base_prec),
        render_exponent(exp, mode)
    )
}

/// 指数是否为 1/n（n = 2 或 3），用于根式改写
fn num_rational_half(r: &num_rational::BigRational) -> Option<u32> {
    use num_traits::One;
    let numer = r.numer();
    let denom = r.denom();
    if !numer.is_one() {
        return None;
    }
    [2u32, 3u32]
        .into_iter()
        .find(|&n| denom == &num_bigint::BigInt::from(n))
}

fn render_own(e: &Expr, mode: DisplayMode) -> (String, u8) {
    match e {
        Expr::Number(n) => (render_number(n, mode), P_ATOM),
        Expr::Variable(v) => (v.clone(), P_ATOM),
        Expr::Function(name, args) => {
            let inner = args
                .iter()
                .map(|a| render_expr(a, mode))
                .collect::<Vec<_>>()
                .join(", ");
            (format!("{}({})", name, inner), P_ATOM)
        }
        Expr::Pow(b, ex) => (render_pow(b, ex, mode), P_POW),
        Expr::Unary(UnaryOp::Neg, x) => (format!("-{}", render_prec(x, mode, P_POW)), P_UNARY),
        Expr::Unary(UnaryOp::Pos, x) => render_own(x, mode),
        Expr::Binary(l, op, r) => match op {
            BinOp::Add => (
                format!(
                    "{} + {}",
                    render_prec(l, mode, P_ADD),
                    render_prec(r, mode, P_ADD)
                ),
                P_ADD,
            ),
            // 右操作数要求更高优先级：`x - (y + z)` 的括号不能被去掉
            BinOp::Sub => (
                format!(
                    "{} - {}",
                    render_prec(l, mode, P_ADD),
                    render_prec(r, mode, P_MUL)
                ),
                P_ADD,
            ),
            BinOp::Mul => {
                let lt = render_prec(l, mode, P_MUL);
                let rt = render_prec(r, mode, P_MUL);
                // 数字系数紧跟原子时用无空格 `*`（`2*x`、`2*x^2`）；
                // 系数本身含空格（`1 / 8`）或右侧是"组"时用 ` * `（`1 / 8 * (2*x - 1)`）
                let tight = !lt.contains(' ') && is_atom(r);
                let sep = if tight { "*" } else { " * " };
                (format!("{}{}{}", lt, sep, rt), P_MUL)
            }
            BinOp::Div => (
                format!(
                    "{} / {}",
                    render_prec(l, mode, P_MUL),
                    render_prec(r, mode, P_UNARY)
                ),
                P_MUL,
            ),
        },
        // 包装节点在渲染时透明（正常流程里它们不该出现在符号结果中）
        Expr::Sd(x) | Expr::Factor(x) => render_own(x, mode),
        Expr::Equation(l, r) => (
            format!("{} = {}", render_expr(l, mode), render_expr(r, mode)),
            0,
        ),
        Expr::System(es) => (
            es.iter()
                .map(|x| render_expr(x, mode))
                .collect::<Vec<_>>()
                .join(", "),
            0,
        ),
    }
}

/// 符号结果的前缀：只有**所有数值系数在该显示模式下都能精确表示**才是 `=`，否则 `≈`。
///
/// 直接复用 `solve_aux::result_prefix` 的既有判据，保证与数值结果同源：
/// MathIO 下精确分数算精确（`1 / 3`），LineIO 下它会显示成 20 位小数，只能算近似。
pub fn expr_prefix(e: &Expr, mode: DisplayMode) -> &'static str {
    if coeffs_all_exact(e, mode) {
        "="
    } else {
        "≈"
    }
}

fn coeffs_all_exact(e: &Expr, mode: DisplayMode) -> bool {
    match e {
        Expr::Number(n) => crate::solve_aux::result_prefix(n, mode) == "=",
        Expr::Variable(_) => true,
        Expr::Function(_, args) => args.iter().all(|a| coeffs_all_exact(a, mode)),
        Expr::Pow(a, b) => coeffs_all_exact(a, mode) && coeffs_all_exact(b, mode),
        Expr::Unary(_, x) | Expr::Sd(x) | Expr::Factor(x) => coeffs_all_exact(x, mode),
        Expr::Binary(l, _, r) => coeffs_all_exact(l, mode) && coeffs_all_exact(r, mode),
        Expr::Equation(l, r) => coeffs_all_exact(l, mode) && coeffs_all_exact(r, mode),
        Expr::System(es) => es.iter().all(|x| coeffs_all_exact(x, mode)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Expr;

    fn var(name: &str) -> Expr {
        Expr::Variable(name.to_string())
    }

    fn num(n: i64) -> Expr {
        Expr::Number(hipercalc_core::number::Number::from_int(n))
    }

    fn mul(a: Expr, b: Expr) -> Expr {
        Expr::Binary(Box::new(a), BinOp::Mul, Box::new(b))
    }

    fn add(a: Expr, b: Expr) -> Expr {
        Expr::Binary(Box::new(a), BinOp::Add, Box::new(b))
    }

    fn sub(a: Expr, b: Expr) -> Expr {
        Expr::Binary(Box::new(a), BinOp::Sub, Box::new(b))
    }

    fn pow(a: Expr, b: Expr) -> Expr {
        Expr::Pow(Box::new(a), Box::new(b))
    }

    #[test]
    fn atom_and_coefficient_spacing() {
        let m = DisplayMode::LineIO;
        // 数字系数 × 原子：无空格
        assert_eq!(render_expr(&mul(num(2), var("x")), m), "2*x");
        assert_eq!(render_expr(&mul(num(2), pow(var("x"), num(3))), m), "2*x^3");
        // 两侧都是"组"：带空格
        assert_eq!(
            render_expr(&mul(add(var("x"), num(1)), sub(var("x"), num(1))), m),
            "(x + 1) * (x - 1)"
        );
    }

    #[test]
    fn precedence_edges() {
        let m = DisplayMode::LineIO;
        // 减法右侧的和必须带括号
        assert_eq!(
            render_expr(&sub(var("x"), add(var("y"), num(1))), m),
            "x - (y + 1)"
        );
        // 幂的指数含运算符时必须带括号
        assert_eq!(
            render_expr(&pow(var("x"), add(num(1), num(1))), m),
            "x^(1 + 1)"
        );
        // 幂的底是乘积时带括号
        assert_eq!(
            render_expr(&pow(mul(num(2), var("x")), num(3)), m),
            "(2*x)^3"
        );
        // 负号优先级高于幂：-x^2 = -(x^2)
        assert_eq!(
            render_expr(
                &Expr::Unary(UnaryOp::Neg, Box::new(pow(var("x"), num(2)))),
                m
            ),
            "-x^2"
        );
        // 除法的右侧要求更高优先级
        assert_eq!(
            render_expr(
                &Expr::Binary(
                    Box::new(var("x")),
                    BinOp::Div,
                    Box::new(mul(num(2), var("y")))
                ),
                m
            ),
            "x / (2*y)"
        );
    }

    #[test]
    fn root_rewrites() {
        let m = DisplayMode::LineIO;
        let half = Expr::Number(hipercalc_core::number::Number::from_rational(
            num_rational::BigRational::new(
                num_bigint::BigInt::from(1),
                num_bigint::BigInt::from(2),
            ),
        ));
        assert_eq!(render_expr(&pow(var("x"), half.clone()), m), "sqrt(x)");
        let third = Expr::Number(hipercalc_core::number::Number::from_rational(
            num_rational::BigRational::new(
                num_bigint::BigInt::from(1),
                num_bigint::BigInt::from(3),
            ),
        ));
        assert_eq!(render_expr(&pow(var("x"), third), m), "cbrt(x)");
    }
}
