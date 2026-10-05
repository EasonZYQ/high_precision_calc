//! 符号求导（`diff(f, x)`）。
//!
//! 递归按规则重写 `Expr`，产出**未化简**的表达式；化简由 `normalize::simplify` 统一负责
//! （否则乘积法则会立刻炸出 `1*x + x*1` 这类垃圾）。
//!
//! 覆盖面：四则运算、商法则、常数幂、一般幂（对数求导）、以及一整套初等函数的链式法则。

use crate::parser::{BinOp, Expr, UnaryOp};

use super::{max_diff_depth, ERROR_DIFF_DEPTH, ERROR_DIFF_VAR, ERROR_NO_SYMBOLIC_EQ};

/// 求导入口：`f` 对 `var` 求导（结果已化简）
pub fn diff(ev: &crate::parser::Evaluator, f: &Expr, var: &str) -> Result<Expr, String> {
    if var.len() != 1 && var != "ans" {
        return Err(ERROR_DIFF_VAR.to_string());
    }
    let raw = diff_raw(ev, f, var, 0)?;
    super::normalize::simplify(ev, &raw)
}

/// 递归求导（未化简）
fn diff_raw(
    ev: &crate::parser::Evaluator,
    e: &Expr,
    var: &str,
    depth: usize,
) -> Result<Expr, String> {
    if depth > max_diff_depth() {
        return Err(ERROR_DIFF_DEPTH.to_string());
    }
    match e {
        Expr::Number(_) => Ok(num(0)),
        Expr::Variable(v) => Ok(num(if v == var { 1 } else { 0 })),
        Expr::Unary(UnaryOp::Pos, x) => diff_raw(ev, x, var, depth + 1),
        Expr::Unary(UnaryOp::Neg, x) => Ok(neg(diff_raw(ev, x, var, depth + 1)?)),
        Expr::Binary(l, op, r) => {
            let dl = diff_raw(ev, l, var, depth + 1)?;
            let dr = diff_raw(ev, r, var, depth + 1)?;
            match op {
                BinOp::Add => Ok(add(dl, dr)),
                BinOp::Sub => Ok(sub(dl, dr)),
                // 乘积法则
                BinOp::Mul => Ok(add(mul(dl, r.as_ref().clone()), mul(l.as_ref().clone(), dr))),
                // 商法则：(l'r - lr') / r^2
                BinOp::Div => Ok(div(
                    sub(mul(dl, r.as_ref().clone()), mul(l.as_ref().clone(), dr)),
                    pow(r.as_ref().clone(), num(2)),
                )),
            }
        }
        Expr::Pow(base, exp) => {
            // 指数不含 var：用幂法则 `g·f^(g-1)·f'`
            if !contains_var(ev, exp, var) {
                let df = diff_raw(ev, base, var, depth + 1)?;
                return Ok(mul(
                    mul(
                        exp.as_ref().clone(),
                        pow(
                            base.as_ref().clone(),
                            sub(exp.as_ref().clone(), num(1)),
                        ),
                    ),
                    df,
                ));
            }
            // 一般幂 `f^g`：对数求导 `f^g·(g'·ln f + g·f'/f)`
            let dg = diff_raw(ev, exp, var, depth + 1)?;
            let df = diff_raw(ev, base, var, depth + 1)?;
            let inner = add(
                mul(dg, call("ln", base.as_ref().clone())),
                div(
                    mul(exp.as_ref().clone(), df),
                    base.as_ref().clone(),
                ),
            );
            Ok(mul(
                pow(base.as_ref().clone(), exp.as_ref().clone()),
                inner,
            ))
        }
        Expr::Function(name, args) => diff_function(ev, name, args, var, depth),
        Expr::Sd(x) | Expr::Factor(x) => diff_raw(ev, x, var, depth + 1),
        Expr::Equation(..) | Expr::System(..) => Err(ERROR_NO_SYMBOLIC_EQ.to_string()),
    }
}

/// 函数调用求导：先算对"自变量 u"的导数，再乘链式 `u'`
fn diff_function(
    ev: &crate::parser::Evaluator,
    name: &str,
    args: &[Expr],
    var: &str,
    depth: usize,
) -> Result<Expr, String> {
    // log(底, 真数) 先重写成 ln(x)/ln(底)，复用 ln 规则
    if name == "log" {
        if args.len() != 2 {
            return Err(format!("函数 log 需要两个参数"));
        }
        let rewritten = div(
            call("ln", args[1].clone()),
            call("ln", args[0].clone()),
        );
        return diff_raw(ev, &rewritten, var, depth + 1);
    }
    if args.len() != 1 {
        return Err(format!("函数 {0} 不支持求导", name));
    }
    let u = &args[0];
    let du = diff_raw(ev, u, var, depth + 1)?;
    // 常数函数直接给 0（避免产出 `cos(x)*0`，也避免不支持求导的函数在常值处误报错）
    if is_zero_expr(&du) {
        return Ok(num(0));
    }
    let outer = outer_derivative(name, u, args)?;
    Ok(mul(outer, du))
}

/// 是否字面为零（未化简的导数里常数导数是 `Number(0)`）
fn is_zero_expr(e: &Expr) -> bool {
    matches!(e, Expr::Number(n) if n.is_zero())
}

/// `d/du f(u)`，u 为自变量
fn outer_derivative(name: &str, u: &Expr, _args: &[Expr]) -> Result<Expr, String> {
    let uu = u.clone();
    Ok(match name {
        "sin" => call("cos", uu),
        "cos" => neg(call("sin", uu)),
        "tan" => pow(call("sec", uu), num(2)),
        "cot" => neg(pow(call("csc", uu), num(2))),
        "sec" => mul(call("sec", uu.clone()), call("tan", uu)),
        "csc" => neg(mul(call("csc", uu.clone()), call("cot", uu))),
        "arcsin" | "asin" => div(num(1), call("sqrt", sub(num(1), pow(uu, num(2))))),
        "arccos" | "acos" => neg(div(num(1), call("sqrt", sub(num(1), pow(uu, num(2)))))),
        "arctan" | "atan" => div(num(1), add(num(1), pow(uu, num(2)))),
        "arccot" | "acot" => neg(div(num(1), add(num(1), pow(uu, num(2))))),
        // d/du arcsec(u) = 1/(|u|·sqrt(u²-1))
        "arcsec" | "asec" => div(
            num(1),
            mul(
                call("abs", uu.clone()),
                call("sqrt", sub(pow(uu, num(2)), num(1))),
            ),
        ),
        "arccsc" | "acsc" => neg(div(
            num(1),
            mul(
                call("abs", uu.clone()),
                call("sqrt", sub(pow(uu, num(2)), num(1))),
            ),
        )),
        "sinh" => call("cosh", uu),
        "cosh" => call("sinh", uu),
        "tanh" => pow(call("sech", uu), num(2)),
        "coth" => neg(pow(call("csch", uu), num(2))),
        "sech" => neg(mul(call("sech", uu.clone()), call("tanh", uu))),
        "csch" => neg(mul(call("csch", uu.clone()), call("coth", uu))),
        "ln" | "log10" | "log2" => {
            // log10/log2 只差一个常数因子，这里按 ln 主体处理（log10 的真导数会多 1/ln10，
            // 但本机把 log10/log2 视为固定底，下面单独处理更精确）
            match name {
                "log10" => div(num(1), mul(uu.clone(), call("ln", num_expr(10)))),
                "log2" => div(num(1), mul(uu.clone(), call("ln", num_expr(2)))),
                _ => div(num(1), uu),
            }
        }
        "exp" => call("exp", uu),
        "sqrt" | "sqr" => div(num(1), mul(num(2), call("sqrt", uu))),
        "cbrt" => div(num(1), mul(num(3), pow(call("cbrt", uu), num(2)))),
        "abs" => call("sign", uu),
        _ => return Err(format!("函数 {0} 不支持求导", name)),
    })
}

/// 表达式是否含指定自由变量（用于区分"常数指数"与"一般幂"）
fn contains_var(ev: &crate::parser::Evaluator, e: &Expr, var: &str) -> bool {
    // 有自由变量就一定含 var（校验期已确认只可能有一个自由变量）
    ev.evaluate_with_vars(e, &[]).is_err() && super::has_free_var_named(e, var)
}

/* ---------------- 构造 Expr 的小工具 ---------------- */

fn num(v: i64) -> Expr {
    Expr::Number(hipercalc_core::number::Number::from_int(v))
}

fn num_expr(v: i64) -> Expr {
    num(v)
}

fn add(a: Expr, b: Expr) -> Expr {
    Expr::Binary(Box::new(a), BinOp::Add, Box::new(b))
}

fn sub(a: Expr, b: Expr) -> Expr {
    Expr::Binary(Box::new(a), BinOp::Sub, Box::new(b))
}

fn mul(a: Expr, b: Expr) -> Expr {
    Expr::Binary(Box::new(a), BinOp::Mul, Box::new(b))
}

fn div(a: Expr, b: Expr) -> Expr {
    Expr::Binary(Box::new(a), BinOp::Div, Box::new(b))
}

fn pow(a: Expr, b: Expr) -> Expr {
    Expr::Pow(Box::new(a), Box::new(b))
}

fn neg(a: Expr) -> Expr {
    Expr::Unary(UnaryOp::Neg, Box::new(a))
}

fn call(name: &str, a: Expr) -> Expr {
    Expr::Function(name.to_string(), vec![a])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{DisplayMode, Evaluator, Parser};

    fn d(input: &str, var: &str) -> String {
        let ev = Evaluator::new();
        let mut p = Parser::new(input);
        let e = p.parse_expression().unwrap();
        let r = diff(&ev, &e, var).unwrap();
        super::super::render::render_expr(&r, DisplayMode::LineIO)
    }

    fn derr(input: &str, var: &str) -> String {
        let ev = Evaluator::new();
        let mut p = Parser::new(input);
        let e = p.parse_expression().unwrap();
        diff(&ev, &e, var).unwrap_err()
    }

    #[test]
    fn basic_rules() {
        assert_eq!(d("x^2", "x"), "2*x");
        assert_eq!(d("x^3", "x"), "3*x^2");
        assert_eq!(d("5", "x"), "0");
        assert_eq!(d("y", "x"), "0");
        assert_eq!(d("x", "x"), "1");
    }

    #[test]
    fn elementary_functions() {
        assert_eq!(d("sin(x)", "x"), "cos(x)");
        assert_eq!(d("cos(x)", "x"), "-sin(x)");
        assert_eq!(d("exp(x)", "x"), "exp(x)");
        assert_eq!(d("ln(x)", "x"), "1 / x");
        assert_eq!(d("sqrt(x)", "x"), "0.5 / sqrt(x)");
        assert_eq!(d("x^x", "x"), "x^x * (ln(x) + 1)");
    }

    #[test]
    fn product_quotient_and_chain() {
        // 排序按总次数降序，所以 x^2*cos(x) 在前
        assert_eq!(d("x^2*sin(x)", "x"), "x^2*cos(x) + 2*x*sin(x)");
        assert_eq!(d("1/x", "x"), "-1 / x^2");
        assert_eq!(d("sin(2*x)", "x"), "2*cos(2*x)");
    }

    #[test]
    fn unsupported_and_errors() {
        assert!(derr("floor(x)", "x").contains("不支持求导"));
        assert!(derr("x^2", "xy").contains("单个变量"));
    }
}
