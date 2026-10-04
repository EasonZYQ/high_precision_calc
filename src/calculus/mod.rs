//! 高等数学功能：求导 / 极限 / 积分 / Taylor 展开 / 求和求积。
//!
//! # 为什么走"AST 重写"而不是新函数
//!
//! 这些功能的**结果要能参与运算**（`diff(x^2,x)+1`、`diff(x^2,x)=2` 要能解方程），
//! 而 `Number` 装不下带自由变量的符号表达式、`eval_function` 也只能返回 `Number`。
//! 于是做法是：在 `parse_and_eval` 里 **解析完成后、分类/求值前**，把
//! `Expr::Function("diff", …)` 这类节点**就地展开成等价的普通 `Expr`**：
//!
//! - 有 `&Evaluator` 可用（数值积分/数值极限要求值）；
//! - **不必给 `Parser` 加生命周期**，也**不新增 `Expr` 变体**（那边有 12 处穷尽 match）；
//! - 展开结果自然融入既有求值 / 方程 / 显示链路。
//!
//! # 职责边界（重要）
//!
//! - `expand_calculus` **递归替换**普通节点，但**遇到 `Factor`/`Sd` 子树不进入**，
//!   而是在递归之前检查其内部是否含高等数学函数名 ⇒ 命中即报错；
//! - `Equation`/`System` 里的高数函数**要**被替换（`diff(x^2,x)=2` 才能解方程），这是刻意行为；
//! - `parser::eval_function` 里还留了一层同名拦截作为**兜底**（防将来有人新增解析路径绕过这里），
//!   正常流程不会触发。
//!
//! # 零行为变化
//!
//! 入口先做**字符串子串预扫描**（要求函数名后跟 `(`），未命中直接原样返回，
//! 因此既有输入的行为完全不变。

pub mod diff;
pub mod integrate;
pub mod limit;
pub mod series;
pub mod sumprod;
pub mod normalize;
pub mod render;

use crate::parser::{Evaluator, Expr};

pub use render::{expr_prefix, render_expr};

/* ---------------- 护栏常量 ---------------- */

const CALC_EXPR_MAX_TERMS_FAST: usize = 4096;
const CALC_EXPR_MAX_TERMS_DEEP: usize = 65536;
const CALC_DIFF_MAX_DEPTH_FAST: usize = 64;
const CALC_DIFF_MAX_DEPTH_DEEP: usize = 256;
const INT_MAX_EVALS_FAST: usize = 2000;
const INT_MAX_EVALS_DEEP: usize = 200_000;
const INT_MAX_DEPTH_FAST: usize = 16;
const INT_MAX_DEPTH_DEEP: usize = 40;
const LIMIT_NUM_ITERS_FAST: usize = 60;
const LIMIT_NUM_ITERS_DEEP: usize = 200;
const TAYLOR_MAX_DEGREE_FAST: usize = 50;
const TAYLOR_MAX_DEGREE_DEEP: usize = 1000;
const SUM_MAX_TERMS_FAST: usize = 2000;
const SUM_MAX_TERMS_DEEP: usize = 100_000;
const SUM_EXACT_MAX_TERMS_FAST: usize = 200;
const SUM_EXACT_MAX_TERMS_DEEP: usize = 2000;
const PROD_MAX_TERMS_FAST: usize = 1000;
const PROD_MAX_TERMS_DEEP: usize = 20_000;

/// 规范化后项数上限
pub fn max_terms() -> usize {
    if crate::calc_mode::is_deep() {
        CALC_EXPR_MAX_TERMS_DEEP
    } else {
        CALC_EXPR_MAX_TERMS_FAST
    }
}

/// 求导递归深度上限
pub fn max_diff_depth() -> usize {
    if crate::calc_mode::is_deep() {
        CALC_DIFF_MAX_DEPTH_DEEP
    } else {
        CALC_DIFF_MAX_DEPTH_FAST
    }
}

/// 数值积分的被积函数求值次数上限
pub fn max_int_evals() -> usize {
    if crate::calc_mode::is_deep() {
        INT_MAX_EVALS_DEEP
    } else {
        INT_MAX_EVALS_FAST
    }
}

/// 数值极限的最大迭代步数
pub fn max_limit_iters() -> usize {
    if crate::calc_mode::is_deep() {
        LIMIT_NUM_ITERS_DEEP
    } else {
        LIMIT_NUM_ITERS_FAST
    }
}

/// 泰勒展开的最高次数上限
pub fn max_taylor_degree() -> usize {
    if crate::calc_mode::is_deep() {
        TAYLOR_MAX_DEGREE_DEEP
    } else {
        TAYLOR_MAX_DEGREE_FAST
    }
}

/// 逐项求和的项数上限（整数项）
pub fn max_sum_terms() -> usize {
    if crate::calc_mode::is_deep() {
        SUM_MAX_TERMS_DEEP
    } else {
        SUM_MAX_TERMS_FAST
    }
}

/// 逐项求和的项数上限（**含除法/负幂**的项：有理数分母会爆炸，预算收紧）
pub fn max_sum_exact_terms() -> usize {
    if crate::calc_mode::is_deep() {
        SUM_EXACT_MAX_TERMS_DEEP
    } else {
        SUM_EXACT_MAX_TERMS_FAST
    }
}

/// 逐项求积的项数上限（阶乘型输出位数增长极快，单独收紧）
pub fn max_prod_terms() -> usize {
    if crate::calc_mode::is_deep() {
        PROD_MAX_TERMS_DEEP
    } else {
        PROD_MAX_TERMS_FAST
    }
}

/// 阶乘闭式允许的最大 n（`10000!` 有三万多位数）
pub const FACTORIAL_MAX: i64 = 10_000;

/// 洛必达的最大轮数
pub const LIMIT_LHOPITAL_MAX: usize = 6;

/// `x → ±inf` 数值探测的指数上限（10^k）。取 30 远低于 `check_trig_range` 的 10^(precision-2)
/// 护栏（默认 10⁷⁸），保证探测不会撞上"三角函数参数过大"而把错误归因搞错。
pub const LIMIT_INF_K_MAX: i32 = 30;

/// 数值积分的二分递归深度上限
pub fn max_int_depth() -> usize {
    if crate::calc_mode::is_deep() {
        INT_MAX_DEPTH_DEEP
    } else {
        INT_MAX_DEPTH_FAST
    }
}

/* ---------------- 文案（必须与 i18n::TABLE 的简体列逐字一致） ---------------- */

pub const ERROR_TOO_MANY_TERMS: &str = "求导结果规模过大（/mode deep 可放宽）";
pub const ERROR_DIFF_DEPTH: &str = "求导展开嵌套过深（/mode deep 可放宽）";
pub const ERROR_DIFF_VAR: &str = "求导变量必须是单个变量（如 x）";
pub const ERROR_NO_SYMBOLIC_EQ: &str = "等式不能出现在符号运算中";
pub const ERROR_SUM_SHADOWED: &str = "求导变量被求和绑定变量遮蔽";
pub const ERROR_CALC_IN_WRAPPER: &str = "高等数学函数 {0} 不能出现在此处（fac/sd/triangle/primefac 内部）";
pub const ERROR_CALC_ARITY: &str = "函数 {0} 需要 {1} 个参数";
pub const ERROR_INF_POSITION: &str = "此处不能使用无穷（inf）";
// 积分
pub const ERROR_INT_BOUND: &str = "积分上下限必须是常数（可为 inf）";
pub const ERROR_INT_SINGULAR: &str = "被积函数在积分区间内出现奇点或非有限值";
pub const ERROR_INT_BUDGET: &str = "积分求值次数超出预算（/mode deep 可放宽）";
pub const ERROR_INT_INF_CONVERGE: &str = "无穷限积分需要能求出原函数并收敛（本次无法判定）";
pub const ERROR_NO_ANTIDERIVATIVE: &str = "无法求出初等原函数（可改用定积分做数值积分）";
pub const ERROR_TABLE_NOT_COVERED: &str = "初等原函数表未覆盖该形态（可用定积分做数值积分）";
// 极限
pub const ERROR_LIMIT_POINT: &str = "极限点必须是常数或 inf";
pub const ERROR_LIMIT_UNDECIDED: &str = "无法判定极限（结构分析失败且数值逼近未收敛）";
pub const ERROR_LIMIT_ONE_SIDED: &str = "左右极限不相等，极限不存在";
// 泰勒展开
pub const ERROR_TAYLOR_POINT: &str = "泰勒展开点必须是常数（不得含变量 {0}）";
pub const ERROR_TAYLOR_ORDER: &str = "泰勒展开阶数必须是非负整数（上限 {0}，/mode deep 可放宽）";
pub const ERROR_TAYLOR_TOO_LARGE: &str = "泰勒展开式过大（/mode deep 可放宽）";
pub const ERROR_TAYLOR_SINGULAR: &str = "泰勒展开点在函数或其导数的奇点上";
// 求和 / 求积
pub const ERROR_SUM_VAR: &str = "求和/求积的变量必须是单个变量（如 k）";
pub const ERROR_SUM_BOUND: &str = "求和/求积的上下限必须是常数整数";
pub const ERROR_SUM_NO_CLOSED_FORM: &str = "求和/求积范围过大且无闭式（/mode deep 可放宽）";
pub const ERROR_SUM_COMPLEX: &str = "求和/求积需求出实数（本次得到复数）";

/* ---------------- 注册表 ---------------- */

/// 高等数学函数名（与 `parser::FUNCTIONS` 同步；顺序无关）。
/// 每新增一项，必须同时补 `CALCULUS_ARITIES` 与对应实现。
pub const CALCULUS_ARITIES: &[(&str, &[usize])] = &[("diff", &[2]), ("int", &[2, 4]), ("lim", &[3]), ("taylor", &[4]), ("sum", &[4]), ("prod", &[4])];

/// 允许 `inf` 出现的位置：`(函数名, 允许 inf 的参数下标)`。
/// 其余位置出现 `inf` 要在这里就报错 —— 否则会落到求值路径报"未定义变量: inf"，误导用户。
pub const CALCULUS_INF_ALLOWED: &[(&str, &[usize])] = &[("int", &[2, 3]), ("lim", &[2])];

/// 从注册表导出函数名列表（仅测试用于一致性断言）
#[cfg(test)]
pub fn calculus_names() -> Vec<&'static str> {
    CALCULUS_ARITIES.iter().map(|(n, _)| *n).collect()
}

pub fn is_calculus_name(name: &str) -> bool {
    CALCULUS_ARITIES.iter().any(|(n, _)| *n == name)
}

/* ---------------- 入口 ---------------- */

/// 输入串里是否**可能**含高等数学函数（廉价预扫描）。
///
/// 要求函数名两侧都是标识符边界、且其后紧跟 `(`：避免 `mydiff(` 这类误判。
/// 宁可多跑一次展开也不能漏。
pub fn input_may_have_calculus(input: &str) -> bool {
    let b = input.as_bytes();
    CALCULUS_ARITIES.iter().any(|(name, _)| {
        let mut from = 0usize;
        while let Some(pos) = input[from..].find(name) {
            let at = from + pos;
            let after = at + name.len();
            let before_ok = at == 0 || !is_ident_byte(b[at - 1]);
            let after_ok = b.get(after) == Some(&b'(');
            if before_ok && after_ok {
                return true;
            }
            from = after;
            if from >= input.len() {
                break;
            }
        }
        false
    })
}

fn is_ident_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// 把高等数学函数节点就地展开成普通 `Expr`；未命中时原样返回（幂等）。
pub fn expand_calculus(ev: &Evaluator, e: Expr) -> Result<Expr, String> {
    // 顶层**就是**一次高数调用时不做整体化简：这样 `taylor(...)` 能保留自己拼好的
    // **升幂**顺序（`x - x^3/6 + x^5/120`，教科书惯例）。一旦和别的运算组合
    // （`2*diff(...)`、`taylor(...)+x`），就交给通用化简——它按次数降序排，
    // 与因式分解等其它输出保持一致。
    let top_is_calc = matches!(&e, Expr::Function(name, _) if is_calculus_name(name));
    let out = expand_node(ev, e)?;
    if top_is_calc {
        return Ok(out);
    }
    // 展开后整体再规范化一次：`2*diff(x^3,x)` 展开成 `2*(3*x^2)`，要合并成 `6*x^2`
    simplify_after_expand(ev, out)
}

/// 展开后的收尾化简；`Factor`/`Sd` 不进（它们有各自的求值链路），等式两侧分别化简
fn simplify_after_expand(ev: &Evaluator, e: Expr) -> Result<Expr, String> {
    match e {
        Expr::Factor(_) | Expr::Sd(_) => Ok(e),
        Expr::Equation(l, r) => Ok(Expr::Equation(
            Box::new(normalize::simplify(ev, &l)?),
            Box::new(normalize::simplify(ev, &r)?),
        )),
        Expr::System(es) => {
            let mut out = Vec::with_capacity(es.len());
            for x in es {
                out.push(simplify_after_expand(ev, x)?);
            }
            Ok(Expr::System(out))
        }
        other => normalize::simplify(ev, &other),
    }
}

fn expand_node(ev: &Evaluator, e: Expr) -> Result<Expr, String> {
    match e {
        Expr::Function(name, args) if is_calculus_name(&name) => {
            let mut expanded = Vec::with_capacity(args.len());
            for a in args {
                expanded.push(expand_node(ev, a)?);
            }
            dispatch(ev, &name, expanded)
        }
        Expr::Function(name, args) => {
            let mut expanded = Vec::with_capacity(args.len());
            for a in args {
                expanded.push(expand_node(ev, a)?);
            }
            Ok(Expr::Function(name, expanded))
        }
        Expr::Binary(l, op, r) => Ok(Expr::Binary(
            Box::new(expand_node(ev, *l)?),
            op,
            Box::new(expand_node(ev, *r)?),
        )),
        Expr::Unary(op, x) => Ok(Expr::Unary(op, Box::new(expand_node(ev, *x)?))),
        Expr::Pow(b, x) => Ok(Expr::Pow(
            Box::new(expand_node(ev, *b)?),
            Box::new(expand_node(ev, *x)?),
        )),
        Expr::Equation(l, r) => Ok(Expr::Equation(
            Box::new(expand_node(ev, *l)?),
            Box::new(expand_node(ev, *r)?),
        )),
        Expr::System(es) => {
            let mut out = Vec::with_capacity(es.len());
            for x in es {
                out.push(expand_node(ev, x)?);
            }
            Ok(Expr::System(out))
        }
        // 包装节点：不进入，但内部出现高数函数要明确报错
        Expr::Factor(inner) => {
            reject_calculus_inside(&inner)?;
            Ok(Expr::Factor(inner))
        }
        Expr::Sd(inner) => {
            reject_calculus_inside(&inner)?;
            Ok(Expr::Sd(inner))
        }
        other => Ok(other),
    }
}

/// `fac(...)` / `sd(...)` 内部不允许出现高等数学函数
fn reject_calculus_inside(e: &Expr) -> Result<(), String> {
    if let Some(name) = first_calculus_name(e) {
        return Err(ERROR_CALC_IN_WRAPPER.replace("{0}", name));
    }
    Ok(())
}

fn first_calculus_name(e: &Expr) -> Option<&str> {
    match e {
        Expr::Function(name, args) => {
            if is_calculus_name(name) {
                return Some(name.as_str());
            }
            args.iter().find_map(first_calculus_name)
        }
        Expr::Binary(l, _, r) => first_calculus_name(l).or_else(|| first_calculus_name(r)),
        Expr::Unary(_, x) | Expr::Sd(x) | Expr::Factor(x) => first_calculus_name(x),
        Expr::Pow(a, b) => first_calculus_name(a).or_else(|| first_calculus_name(b)),
        Expr::Equation(l, r) => first_calculus_name(l).or_else(|| first_calculus_name(r)),
        Expr::System(es) => es.iter().find_map(first_calculus_name),
        _ => None,
    }
}

/// 按函数名分派到具体实现
fn dispatch(ev: &Evaluator, name: &str, args: Vec<Expr>) -> Result<Expr, String> {
    check_arity(name, args.len())?;
    check_inf_positions(name, &args)?;
    match name {
        "diff" => {
            let var = as_variable_name(&args[1])?;
            if contains_var_name(&args[0], &var) && binds_same_var(&args[0], &var) {
                return Err(ERROR_SUM_SHADOWED.to_string());
            }
            diff::diff(ev, &args[0], &var)
        }
        "int" => {
            let var = as_variable_name(&args[1])?;
            if args.len() == 2 {
                // 不定积分：返回原函数（符号结果；**不写 +C**，文档已说明省略积分常数）
                integrate::antiderivative(ev, &args[0], &var)
            } else {
                // 定积分：返回数值（精确优先，数值兜底）
                let r = integrate::definite(ev, &args[0], &var, &args[2], &args[3])?;
                Ok(Expr::Number(r))
            }
        }
        "lim" => {
            let var = as_variable_name(&args[1])?;
            limit::limit(ev, &args[0], &var, &args[2])
        }
        "taylor" => {
            let var = as_variable_name(&args[1])?;
            let n = as_order(ev, &args[3])?;
            series::taylor(ev, &args[0], &var, &args[2], n)
        }
        "sum" => {
            let var = as_variable_name_or(&args[1], ERROR_SUM_VAR)?;
            sumprod::sum(ev, &args[0], &var, &args[2], &args[3])
        }
        "prod" => {
            let var = as_variable_name_or(&args[1], ERROR_SUM_VAR)?;
            sumprod::prod(ev, &args[0], &var, &args[2], &args[3])
        }
        _ => Err(format!("未知函数: {}", name)),
    }
}

fn check_arity(name: &str, got: usize) -> Result<(), String> {
    let Some((_, allowed)) = CALCULUS_ARITIES.iter().find(|(n, _)| *n == name) else {
        return Err(format!("未知函数: {}", name));
    };
    if allowed.contains(&got) {
        return Ok(());
    }
    let want = match allowed.len() {
        1 => allowed[0].to_string(),
        _ => allowed
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(" 或 "),
    };
    Err(ERROR_CALC_ARITY
        .replace("{0}", name)
        .replace("{1}", &want))
}

/// `inf` 只允许出现在白名单位置（其余位置给出明确报错，而不是"未定义变量: inf"）
fn check_inf_positions(name: &str, args: &[Expr]) -> Result<(), String> {
    let allowed: &[usize] = CALCULUS_INF_ALLOWED
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, a)| *a)
        .unwrap_or(&[]);
    for (i, a) in args.iter().enumerate() {
        if contains_inf(a) && !allowed.contains(&i) {
            return Err(ERROR_INF_POSITION.to_string());
        }
    }
    Ok(())
}

fn contains_inf(e: &Expr) -> bool {
    match e {
        Expr::Variable(v) => v == "inf",
        Expr::Binary(l, _, r) => contains_inf(l) || contains_inf(r),
        Expr::Unary(_, x) | Expr::Sd(x) | Expr::Factor(x) => contains_inf(x),
        Expr::Pow(a, b) => contains_inf(a) || contains_inf(b),
        Expr::Function(_, args) => args.iter().any(contains_inf),
        _ => false,
    }
}

/// 泰勒阶数：必须是非负整数常数
fn as_order(ev: &Evaluator, e: &Expr) -> Result<usize, String> {
    let limit = max_taylor_degree();
    let v = ev
        .evaluate_with_vars(e, &[])
        .map_err(|_| ERROR_TAYLOR_ORDER.replace("{0}", &limit.to_string()))?;
    let r = v
        .as_rational()
        .ok_or_else(|| ERROR_TAYLOR_ORDER.replace("{0}", &limit.to_string()))?;
    if !r.is_integer() || r.numer().sign() == num_bigint::Sign::Minus {
        return Err(ERROR_TAYLOR_ORDER.replace("{0}", &limit.to_string()));
    }
    let n: usize = r
        .numer()
        .try_into()
        .map_err(|_| ERROR_TAYLOR_ORDER.replace("{0}", &limit.to_string()))?;
    if n > limit {
        return Err(ERROR_TAYLOR_ORDER.replace("{0}", &limit.to_string()));
    }
    Ok(n)
}

/// 第二个参数必须是单个变量名
fn as_variable_name(e: &Expr) -> Result<String, String> {
    as_variable_name_or(e, ERROR_DIFF_VAR)
}

fn as_variable_name_or(e: &Expr, msg: &str) -> Result<String, String> {
    match e {
        // 解析器把单个字母（含大写）收成 Variable；`ans` 也是 Variable
        Expr::Variable(v) => Ok(v.clone()),
        _ => Err(msg.to_string()),
    }
}

/// 表达式里是否出现名为 `var` 的自由变量
pub fn has_free_var_named(e: &Expr, var: &str) -> bool {
    match e {
        Expr::Variable(v) => v == var,
        Expr::Binary(l, _, r) => has_free_var_named(l, var) || has_free_var_named(r, var),
        Expr::Unary(_, x) | Expr::Sd(x) | Expr::Factor(x) => has_free_var_named(x, var),
        Expr::Pow(a, b) => has_free_var_named(a, var) || has_free_var_named(b, var),
        Expr::Function(_, args) => args.iter().any(|a| has_free_var_named(a, var)),
        Expr::Equation(l, r) => has_free_var_named(l, var) || has_free_var_named(r, var),
        Expr::System(es) => es.iter().any(|x| has_free_var_named(x, var)),
        Expr::Number(_) => false,
    }
}

fn contains_var_name(e: &Expr, var: &str) -> bool {
    has_free_var_named(e, var)
}

/// 是否把 `var` 用作求和/求积的绑定变量（v1 还没有 sum/prod，先留接口）
fn binds_same_var(_e: &Expr, _var: &str) -> bool {
    false
}

/// 表达式是否含**自由变量**（`ans` 与 `/let` 存储的大写变量不算）
pub fn has_free_variables(ev: &Evaluator, e: &Expr) -> bool {
    match e {
        Expr::Variable(v) => {
            // 借求值器判定：`ans` 与存储变量能求出来，自由变量会报"未定义变量"
            ev.evaluate_with_vars(&Expr::Variable(v.clone()), &[])
                .is_err()
        }
        Expr::Number(_) => false,
        Expr::Binary(l, _, r) => has_free_variables(ev, l) || has_free_variables(ev, r),
        Expr::Unary(_, x) | Expr::Sd(x) | Expr::Factor(x) => has_free_variables(ev, x),
        Expr::Pow(a, b) => has_free_variables(ev, a) || has_free_variables(ev, b),
        Expr::Function(_, args) => args.iter().any(|a| has_free_variables(ev, a)),
        Expr::Equation(l, r) => has_free_variables(ev, l) || has_free_variables(ev, r),
        Expr::System(es) => es.iter().any(|x| has_free_variables(ev, x)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{DisplayMode, Parser};

    fn parse(s: &str) -> Expr {
        let mut p = Parser::new(s);
        p.parse_system().unwrap()
    }

    fn expanded(s: &str) -> Result<String, String> {
        let ev = Evaluator::new();
        let e = parse(s);
        let out = expand_calculus(&ev, e)?;
        Ok(render_expr(&out, DisplayMode::LineIO))
    }

    #[test]
    fn calculus_names_are_registered_in_functions() {
        // 注册表与白名单必须同步：只改一处会导致高亮/Tab 补全失效或解析被拒
        for n in calculus_names() {
            assert!(
                crate::parser::FUNCTIONS.contains(&n),
                "{n} 未登记进 parser::FUNCTIONS"
            );
        }
    }

    #[test]
    fn prescan_requires_call_syntax() {
        assert!(input_may_have_calculus("diff(x^2,x)"));
        assert!(input_may_have_calculus("2*diff(x^2,x)+1"));
        assert!(!input_may_have_calculus("1+2*3"));
        assert!(!input_may_have_calculus("x+1")); // `diff` 作为子串不存在
        assert!(!input_may_have_calculus("mydiff(3)"));
    }

    #[test]
    fn expands_inside_expressions() {
        assert_eq!(expanded("diff(x^2,x)").unwrap(), "2*x");
        assert_eq!(expanded("diff(x^2,x)+1").unwrap(), "2*x + 1");
        assert_eq!(expanded("2*diff(x^3,x)").unwrap(), "6*x^2");
        assert_eq!(expanded("diff(diff(x^3,x),x)").unwrap(), "6*x");
    }

    #[test]
    fn wrapper_rejects_calculus() {
        let ev = Evaluator::new();
        // `Expr::Factor`/`Expr::Sd` 包装由 parse_factor/parse_sd 产生（parse_system 不会给），
        // 这里手工构造，直接测重写通路的拦截逻辑
        let err = expand_calculus(&ev, Expr::Factor(Box::new(parse("diff(x^3,x)")))).unwrap_err();
        assert!(err.contains("不能出现在此处"), "{err}");
        let err = expand_calculus(&ev, Expr::Sd(Box::new(parse("diff(x^2,x)")))).unwrap_err();
        assert!(err.contains("不能出现在此处"), "{err}");
    }

    #[test]
    fn idempotent() {
        let ev = Evaluator::new();
        let once = expand_calculus(&ev, parse("diff(x^2,x)+1")).unwrap();
        let twice = expand_calculus(&ev, once.clone()).unwrap();
        assert_eq!(
            render_expr(&once, DisplayMode::LineIO),
            render_expr(&twice, DisplayMode::LineIO)
        );
    }

    #[test]
    fn arity_and_var_validation() {
        assert!(expanded("diff(x^2)").unwrap_err().contains("需要"));
        assert!(expanded("diff(x^2,x+1)").unwrap_err().contains("单个变量"));
    }

    #[test]
    fn free_variable_detection() {
        let mut ev = Evaluator::new();
        assert!(has_free_variables(&ev, &parse("2*x")));
        assert!(!has_free_variables(&ev, &parse("1+2*3")));
        ev.vars
            .insert("A".to_string(), crate::number::Number::from_int(3));
        assert!(!has_free_variables(&ev, &parse("A+1")));
    }
}
