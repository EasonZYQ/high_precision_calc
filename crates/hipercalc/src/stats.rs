//! 统计函数：**解析期展开**（见 `docs/ROADMAP.md` 第 5 项）。
//!
//! # 为什么在解析期做
//!
//! - **不给 `Expr` 加"列表"变体**：`Expr::Number` 在 14 个文件里被穷尽匹配（calculus 全部子模块 + 各 solver），
//!   加变体会牵动一大片。`[e1, e2, …]` 在解析期就脱糖成 `Function("list", …)`，本模块按这个形状识别，
//!   **零 AST 波及**，同一套语法将来直接给矩阵用（`[[1,2],[3,4]]`）。
//! - 求和型函数（mean / var / stddev / corr）改写成**已有的 sum / 算术节点**，
//!   求值器完全不需要认识它们，`eval_function` 的参数个数校验也就碰不到它们。
//!
//! # 三个设计细节
//!
//! - **方差用 `E[X²] − E[X]²`**：这个式子**不需要变量替换**。
//!   "先算均值、再逐项求偏差"要把均值代回 `f` 的 AST，麻烦且易错。
//! - **`median` / `percentile` 在解析期就地求值**：排序要求"具体的值"，而解析期展开拿不到运行时的列表
//!   ⇒ 把数据点算成 `Number`、排序、取位，然后**替换成一个常量表达式**。
//!   代价：它们的数据点必须在解析期可求值（引用 `/let` 变量没问题 —— 那些变量就在求值器里）。
//! - **本模块会生成 `sum(...)` 节点**，而那个节点要靠 calculus 那一遍来展开 ⇒ 解析期的预扫必须
//!   把"输入里出现了统计函数名"也算进来（见 `parser::parse_and_eval` 里的 `had_calculus`）。

use crate::parser::{BinOp, Evaluator, Expr};
use hipercalc_core::bigfloat::BigFloat;
use hipercalc_core::number::Number;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive, Zero};

/// 本模块负责的函数名（其余函数原样交给求值器）
pub fn is_stat_name(name: &str) -> bool {
    matches!(
        name,
        "mean" | "var" | "var_s" | "stddev" | "stddev_s" | "median" | "percentile" | "corr"
    )
}

/// 输入里是否出现了统计函数名。
///
/// 给解析期的预扫用：`had_calculus` 原本只看"有没有 diff/taylor 之类"，而统计展开会**生成 sum 节点**，
/// 那个节点必须再走一遍 calculus 才能展开 ⇒ 预扫要把统计名也算上。
pub fn input_may_have_stats(input: &str) -> bool {
    [
        "mean",
        "var",
        "var_s",
        "stddev",
        "stddev_s",
        "median",
        "percentile",
        "corr",
    ]
    .iter()
    .any(|n| input.contains(n))
}

/// 递归改写整棵树里的统计函数调用
pub fn expand_stats(ev: &Evaluator, e: &Expr) -> Result<Expr, String> {
    Ok(match e {
        Expr::Function(name, args) => {
            let args: Vec<Expr> = args
                .iter()
                .map(|a| expand_stats(ev, a))
                .collect::<Result<_, _>>()?;
            if is_stat_name(name) {
                expand_call(ev, name, args)?
            } else {
                Expr::Function(name.clone(), args)
            }
        }
        Expr::Binary(a, op, b) => Expr::Binary(
            Box::new(expand_stats(ev, a)?),
            *op,
            Box::new(expand_stats(ev, b)?),
        ),
        Expr::Unary(op, a) => Expr::Unary(*op, Box::new(expand_stats(ev, a)?)),
        Expr::Pow(a, b) => Expr::Pow(
            Box::new(expand_stats(ev, a)?),
            Box::new(expand_stats(ev, b)?),
        ),
        Expr::Sd(a) => Expr::Sd(Box::new(expand_stats(ev, a)?)),
        Expr::Factor(a) => Expr::Factor(Box::new(expand_stats(ev, a)?)),
        Expr::Equation(a, b) => Expr::Equation(
            Box::new(expand_stats(ev, a)?),
            Box::new(expand_stats(ev, b)?),
        ),
        Expr::System(v) => Expr::System(
            v.iter()
                .map(|x| expand_stats(ev, x))
                .collect::<Result<_, _>>()?,
        ),
        other => other.clone(),
    })
}

/// 数据源：列表形态 / 表达式+范围形态
enum Data {
    /// 一组具体表达式（`[x1, x2, …]` 或 `x1, x2, …`）
    Items(Vec<Expr>),
    /// `f, k, a, b`
    Range(Expr, Expr, Expr, Expr),
}

/// 判别规则（**精确，不靠猜**）：4 个参数且第 2 个是变量 ⇒ 范围形态；否则按数据列表。
fn parse_data(name: &str, args: &[Expr]) -> Result<Data, String> {
    if args.len() == 4 {
        if let Expr::Variable(k) = &args[1] {
            return Ok(Data::Range(
                args[0].clone(),
                Expr::Variable(k.clone()),
                args[2].clone(),
                args[3].clone(),
            ));
        }
    }
    let items = match args {
        [Expr::Function(n, xs)] if n == "list" => xs.clone(),
        _ => args.to_vec(),
    };
    if items.len() < 2 {
        return Err(format!(
            "{name} 至少需要两个数据点：列表形态 {name}([x1, x2, …])，或范围形态 {name}(f, k, a, b)"
        ));
    }
    Ok(Data::Items(items))
}

fn num(v: i64) -> Expr {
    Expr::Number(Number::from_int(v))
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
fn sq(a: Expr) -> Expr {
    Expr::Pow(Box::new(a), Box::new(num(2)))
}
fn sqrt(a: Expr) -> Expr {
    Expr::Function("sqrt".to_string(), vec![a])
}

/// 项数：列表形态是字面量，范围形态是 `b − a + 1`
fn count_of(data: &Data) -> Expr {
    match data {
        Data::Items(xs) => num(xs.len() as i64),
        Data::Range(_, _, a, b) => add(sub(b.clone(), a.clone()), num(1)),
    }
}

/// 对每个数据点套一个「项变换」后求和：列表形态展开成显式加法树，范围形态用 `sum`。
fn sum_of(data: &Data, term: impl Fn(&Expr) -> Expr) -> Expr {
    match data {
        Data::Items(xs) => {
            let mut it = xs.iter().map(|x| term(x));
            let first = it.next().expect("parse_data 已保证 ≥2 项");
            it.fold(first, add)
        }
        Data::Range(f, k, a, b) => Expr::Function(
            "sum".to_string(),
            vec![term(f), k.clone(), a.clone(), b.clone()],
        ),
    }
}

fn expand_call(ev: &Evaluator, name: &str, args: Vec<Expr>) -> Result<Expr, String> {
    match name {
        "median" | "percentile" => expand_median_like(ev, name, args),
        "corr" => expand_corr(args),
        _ => {
            // mean / var / var_s / stddev / stddev_s
            let data = parse_data(name, &args)?;
            let n = count_of(&data);
            let mean = div(sum_of(&data, |x| x.clone()), n.clone());
            let ex2 = div(sum_of(&data, |x| sq(x.clone())), n.clone()); // E[X²]
            let var = sub(ex2, sq(mean.clone())); // E[X²] − E[X]²
            let var_s = mul(var.clone(), div(n.clone(), sub(n.clone(), num(1))));
            Ok(match name {
                "mean" => mean,
                "var" => var,
                "stddev" => sqrt(var),
                "var_s" => var_s.clone(),
                "stddev_s" => sqrt(var_s),
                other => return Err(format!("未知统计函数: {other}")),
            })
        }
    }
}

/// `median(data)` / `percentile(p, data)`：解析期求值 + 排序 + 取位，替换成常量表达式。
fn expand_median_like(ev: &Evaluator, name: &str, args: Vec<Expr>) -> Result<Expr, String> {
    let (pct, data) = if name == "percentile" {
        // 列表形态是 2 个参数（p + 列表），范围形态是 5 个（p + f, k, a, b）
        if args.len() < 2 {
            return Err(
                "percentile 用法：percentile(p, [x1, x2, …]) 或 percentile(p, f, k, a, b)"
                    .to_string(),
            );
        }
        let p = eval_const(ev, &args[0])?;
        let p = p
            .as_rational()
            .ok_or_else(|| "percentile 的百分位必须是具体数值".to_string())?;
        if p < BigRational::zero() || p > BigRational::from_integer(BigInt::from(100)) {
            return Err("百分位必须在 0 到 100 之间".to_string());
        }
        (Some(p), parse_data(name, &args[1..])?)
    } else {
        (None, parse_data(name, &args)?)
    };

    let mut vals = concrete_values(ev, &data)?;
    vals.sort_by(cmp_numbers);
    let n = vals.len();

    match pct {
        None => {
            // 中位数：奇数取中间，偶数取中间两个的平均（用 Expr 表达，交给求值器算，保持精确）
            if n % 2 == 1 {
                Ok(Expr::Number(vals[n / 2].clone()))
            } else {
                Ok(div(
                    add(
                        Expr::Number(vals[n / 2 - 1].clone()),
                        Expr::Number(vals[n / 2].clone()),
                    ),
                    num(2),
                ))
            }
        }
        Some(p) => {
            // 线性插值：rank = p/100·(n−1)，在 floor 与 ceil 之间按小数部分插值
            let hundred = BigRational::from_integer(BigInt::from(100));
            let rank = &p * BigRational::from_integer(BigInt::from(n as i64 - 1)) / hundred;
            let lo = rank
                .to_integer()
                .to_i64()
                .ok_or_else(|| "percentile 的位置超出范围".to_string())?
                as usize;
            let frac = &rank - BigRational::from_integer(BigInt::from(lo as i64));
            if frac.is_zero() || lo + 1 >= n {
                return Ok(Expr::Number(vals[lo].clone()));
            }
            let w_lo = Expr::Number(Number::from_rational(BigRational::one() - &frac));
            let w_hi = Expr::Number(Number::from_rational(frac));
            Ok(add(
                mul(w_lo, Expr::Number(vals[lo].clone())),
                mul(w_hi, Expr::Number(vals[lo + 1].clone())),
            ))
        }
    }
}

/// `corr(x, y, k, a, b)` 或 `corr([xs], [ys])`：r = (E[XY] − E[X]E[Y]) / (σx·σy)
fn expand_corr(args: Vec<Expr>) -> Result<Expr, String> {
    let (dx, dy) = if args.len() == 5 {
        if let Expr::Variable(k) = &args[2] {
            let kx = Expr::Variable(k.clone());
            let (a, b) = (args[3].clone(), args[4].clone());
            (
                Data::Range(args[0].clone(), kx.clone(), a.clone(), b.clone()),
                Data::Range(args[1].clone(), kx, a, b),
            )
        } else {
            return Err("corr 用法：corr(x, y, k, a, b) 或 corr([x1, …], [y1, …])".to_string());
        }
    } else if args.len() == 2 {
        (
            parse_data("corr", &args[0..1])?,
            parse_data("corr", &args[1..2])?,
        )
    } else {
        return Err("corr 用法：corr(x, y, k, a, b) 或 corr([x1, …], [y1, …])".to_string());
    };
    // 两个数据源的长度必须一致（列表形态能立刻查；范围形态由用户自己保证）
    if let (Data::Items(xs), Data::Items(ys)) = (&dx, &dy) {
        if xs.len() != ys.len() {
            return Err(format!(
                "corr 的两组数据长度必须相同（当前 {} 与 {}）",
                xs.len(),
                ys.len()
            ));
        }
    }
    let nx = count_of(&dx);
    let ny = count_of(&dy);
    let mean_x = div(sum_of(&dx, |x| x.clone()), nx.clone());
    let mean_y = div(sum_of(&dy, |y| y.clone()), ny.clone());
    let exy = match (&dx, &dy) {
        (Data::Items(xs), Data::Items(ys)) => {
            let mut it = xs
                .iter()
                .zip(ys.iter())
                .map(|(x, y)| mul(x.clone(), y.clone()));
            let first = it
                .next()
                .ok_or_else(|| "corr 需要至少两组数据".to_string())?;
            div(it.fold(first, add), nx.clone())
        }
        (Data::Range(f, k, a, b), Data::Range(g, _, _, _)) => div(
            Expr::Function(
                "sum".to_string(),
                vec![mul(f.clone(), g.clone()), k.clone(), a.clone(), b.clone()],
            ),
            nx.clone(),
        ),
        _ => return Err("corr 的两组数据形态必须一致".to_string()),
    };
    let var_x = sub(
        div(sum_of(&dx, |x| sq(x.clone())), nx.clone()),
        sq(mean_x.clone()),
    );
    let var_y = sub(
        div(sum_of(&dy, |y| sq(y.clone())), ny.clone()),
        sq(mean_y.clone()),
    );
    Ok(div(
        sub(exy, mul(mean_x, mean_y)),
        mul(sqrt(var_x), sqrt(var_y)),
    ))
}

/// 解析期求一个常量表达式的值。
///
/// `Evaluator::evaluate` 需要 `&mut`，而这里手上只有 `&`；`Evaluator` 也没实现 `Clone`。
/// 所以走 `&self` 的 `evaluate_with_var`，代入一个**不可能出现的变量名** —— 语义上等价于直接求值，
/// 不必为了这点便利去改核心类型的派生。
fn eval_const(ev: &Evaluator, e: &Expr) -> Result<Number, String> {
    ev.evaluate_with_var(e, "\u{1}", &Number::from_int(0))
}

/// 把数据点全部算成具体数值（`median` / `percentile` 排序要用）
fn concrete_values(ev: &Evaluator, data: &Data) -> Result<Vec<Number>, String> {
    match data {
        Data::Items(xs) => xs.iter().map(|x| eval_const(ev, x)).collect(),
        Data::Range(f, k, a, b) => {
            let kname = match k {
                Expr::Variable(n) => n.clone(),
                _ => return Err("范围形态的第二个参数必须是变量".to_string()),
            };
            let lo = eval_const(ev, a)?
                .as_rational()
                .and_then(|r| r.to_integer().to_i64())
                .ok_or_else(|| "范围下界必须是具体整数".to_string())?;
            let hi = eval_const(ev, b)?
                .as_rational()
                .and_then(|r| r.to_integer().to_i64())
                .ok_or_else(|| "范围上界必须是具体整数".to_string())?;
            if hi < lo {
                return Err("范围上界不能小于下界".to_string());
            }
            if hi - lo > 100_000 {
                return Err("数据点过多（上限 100000）".to_string());
            }
            (lo..=hi)
                .map(|v| ev.evaluate_with_var(f, &kname, &Number::from_int(v)))
                .collect()
        }
    }
}

/// 比较两个数：`Number` 没有 `Ord` ⇒ 借定点尾数的符号判断（与项目里其它比较一致）
fn cmp_numbers(a: &Number, b: &Number) -> std::cmp::Ordering {
    let d = BigFloat::sub(&a.to_approx(), &b.to_approx(), 40);
    if d.value.is_negative() {
        std::cmp::Ordering::Less
    } else if d.value.is_zero() {
        std::cmp::Ordering::Equal
    } else {
        std::cmp::Ordering::Greater
    }
}
