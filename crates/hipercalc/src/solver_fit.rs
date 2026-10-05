//! 多项式函数拟合（便捷函数求解）
//!
//! 输入若干坐标即可求多项式函数（2 点一次、3 点二次……）；坐标后可跟"带参数的解析式模板"，
//! 则解出未知参数并给出完整解析式；模板里**参数欠定**时输出参数之间的关系，解析式也用关系表达。
//! 名为 `P` 的坐标表示函数顶点（提供 `f(a)=b` 与 `f'(a)=0` 两个约束）。
//!
//! 三条设计约定：
//! - 全程走 `Number` 的精确运算链（有理数、分数、`pi`/`sqrt(2)` 都保持精确，能精确呈现就用 `=`）；
//! - 模板必须是"化简后关于 x 的多项式"（`sin/log` 之类不支持 ⇒ 明确报错），
//!   化简由"按次数合并同类项"自动完成：解析式 = `g(x) + Σ p_j·φ_j(x)`；
//! - 参数解表示成**自由参数的线性形式**（`LinForm`），因此欠定情形能给出关系式而不是报错。
//!
//! 渲染与 `solver_factor::format_poly` 的分工：后者只吃 `BigRational`（因式分解用），
//! 本模块的系数是 `Number`（可为无理/复数/自由参数线性形式），故自带渲染器，两者不要混用。

use std::collections::BTreeMap;

use num_traits::Signed;

use crate::parser::{DisplayMode, Evaluator, Expr};
use crate::solve_aux;
use crate::solver_poly;
use hipercalc_core::calc_mode;
use hipercalc_core::display;
use hipercalc_core::number::Number;

/// Fast 模式下"约束/参数"个数上限（Deep 放开）。次数 ≈ 上限 − 1；
/// 有理数范德蒙矩阵的分子位数增长很快，16 已经能在毫秒级完成，再大才可能数秒以上。
const FIT_MAX_SIZE_FAST: usize = 16;

/// 一个坐标点：`(x, y)` 或顶点 `P(x, y)`
#[derive(Debug, Clone)]
pub struct FitPoint {
    /// 是否为顶点（输入时写成大写 `P(...)`）
    pub is_vertex: bool,
    /// 横坐标表达式（求解阶段再求值，保留 `pi`/分数等精确形式）
    pub x: Expr,
    /// 纵坐标表达式
    pub y: Expr,
}

/// 解析式模板（左值只作装饰，解析层已丢弃 ⇒ 输出恒为 `y = …`）
#[derive(Debug, Clone)]
pub struct FitTemplate {
    /// 右值表达式（关于自变量 `var` 与若干未知参数）
    pub rhs: Expr,
    /// 自变量名（固定 'x'）
    pub var: char,
}

/// 一行拟合输入（坐标 + 可选解析式模板）
#[derive(Debug, Clone)]
pub struct FitInput {
    pub points: Vec<FitPoint>,
    pub template: Option<FitTemplate>,
}

/// 线性形式：`k + Σ coeff_i · free_i`（`free_i` 为自由参数名，按名字典序；零系数自动剔除）。
/// 参数解与多项式系数都用它表示 ⇒ 唯一解时退化为常数，欠定时保留自由参数。
#[derive(Debug, Clone)]
pub struct LinForm {
    k: Number,
    terms: BTreeMap<String, Number>,
}

impl LinForm {
    /// 常数
    pub fn constant(k: Number) -> Self {
        LinForm {
            k,
            terms: BTreeMap::new(),
        }
    }

    /// 单个自由参数（系数 1）
    pub fn free(name: &str) -> Self {
        let mut terms = BTreeMap::new();
        terms.insert(name.to_string(), Number::from_int(1));
        LinForm {
            k: Number::from_int(0),
            terms,
        }
    }

    /// 是否为常数（不含自由参数）
    pub fn is_constant(&self) -> bool {
        self.terms.values().all(|c| c.is_zero())
    }

    /// 取常数项（非常数形式返回 None）
    pub fn as_constant(&self) -> Option<&Number> {
        if self.is_constant() {
            Some(&self.k)
        } else {
            None
        }
    }

    /// 求和（零系数自动剔除）
    pub fn add(&self, other: &LinForm) -> LinForm {
        let mut k = self.k.add(&other.k);
        let mut terms = self.terms.clone();
        for (name, c) in &other.terms {
            let mut e = terms
                .get(name)
                .cloned()
                .unwrap_or_else(|| Number::from_int(0));
            e = e.add(c);
            terms.insert(name.clone(), e);
        }
        LinForm::prune(&mut k, &mut terms)
    }

    pub fn sub(&self, other: &LinForm) -> LinForm {
        self.add(&other.neg())
    }

    pub fn neg(&self) -> LinForm {
        let mut terms = BTreeMap::new();
        for (name, c) in &self.terms {
            terms.insert(name.clone(), c.neg());
        }
        LinForm {
            k: self.k.neg(),
            terms,
        }
    }

    /// 乘一个数值常数
    pub fn scale(&self, n: &Number) -> LinForm {
        let mut k = self.k.mul(n);
        let mut terms = BTreeMap::new();
        for (name, c) in &self.terms {
            terms.insert(name.clone(), c.mul(n));
        }
        LinForm::prune(&mut k, &mut terms)
    }

    /// 去掉零系数项（保持"零形式"的唯一表示，便于相等判定）
    fn prune(k: &mut Number, terms: &mut BTreeMap<String, Number>) -> LinForm {
        let mut out = BTreeMap::new();
        for (name, c) in terms.iter() {
            if !c.is_zero() {
                out.insert(name.clone(), c.clone());
            }
        }
        let kk = std::mem::replace(k, Number::from_int(0));
        LinForm { k: kk, terms: out }
    }

    /// 渲染为文本：`2 - a`、`a`、`a + b`、`-a`、`0`…
    pub fn render(&self, mode: DisplayMode) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !self.k.is_zero() {
            parts.push(display_part(&self.k, mode));
        }
        for (name, c) in &self.terms {
            if c.is_zero() {
                continue;
            }
            // 系数为 1 / -1 时省略数字
            let one = Number::from_int(1);
            let neg_one = Number::from_int(-1);
            if number_eq(c, &one) {
                parts.push(name.clone());
            } else if number_eq(c, &neg_one) {
                parts.push(format!("-{}", name));
            } else {
                parts.push(format!("{}*{}", display_part(c, mode), name));
            }
        }
        if parts.is_empty() {
            return "0".to_string();
        }
        let mut out = parts[0].clone();
        for p in &parts[1..] {
            if let Some(rest) = p.strip_prefix('-') {
                out.push_str(" - ");
                out.push_str(rest);
            } else {
                out.push_str(" + ");
                out.push_str(p);
            }
        }
        out
    }

    /// 作为"系数"渲染：多项、或串里含空格/除号（如 `1 / 2`、`2 - a`）时加括号，
    /// 否则拼出来的 `1 / 2*x` 会有歧义
    pub fn render_coeff(&self, mode: DisplayMode) -> String {
        let s = self.render(mode);
        let multi = self.terms.len() + usize::from(!self.k.is_zero()) > 1;
        if multi || s.contains(' ') || s.contains('/') {
            format!("({})", s)
        } else {
            s
        }
    }
}

/// 数值相等（`Number` 没有 `PartialEq`，用"差为零"判定；精确值按精确相等，近似值按零判定）
pub fn number_eq(a: &Number, b: &Number) -> bool {
    a.sub(b).is_zero()
}

/// 单个数值的显示（走既有显示层，自动处理精确/近似/复数）
fn display_part(n: &Number, mode: DisplayMode) -> String {
    match mode {
        DisplayMode::MathIO => display::format_mathio(n),
        DisplayMode::LineIO => display::format_lineio(n),
    }
}

/// 拟合结果
#[derive(Debug, Clone)]
pub struct FitSolution {
    /// 参数 → 线性形式（唯一解时是常数；欠定时含自由参数）；无模板时为空
    pub params: Vec<(String, LinForm)>,
    /// 自由参数名（按字典序；唯一解时为空）
    pub free: Vec<String>,
    /// 一般式的升幂系数（含自由参数时为线性形式）
    pub coeffs: Vec<LinForm>,
    /// 自变量名
    pub var: char,
}

impl FitSolution {
    /// 系数是否全为常数（决定能否输出数值顶点式）
    pub fn all_constant(&self) -> bool {
        self.coeffs.iter().all(|c| c.is_constant())
    }
}

/* ---------------- 求解 ---------------- */

/// 求解多项式拟合（坐标 → 函数）。错误信息为中文，直接面向用户。
pub fn solve_polynomial_fit(
    points: &[FitPoint],
    template: Option<&FitTemplate>,
    evaluator: &Evaluator,
) -> Result<FitSolution, String> {
    if points.len() < 2 {
        // 单个顶点是典型误用，给更具体的提示（顶点已提供 2 个约束，但缺普通坐标）
        if points.len() == 1 && points[0].is_vertex {
            return Err("顶点坐标还需要至少一个普通坐标".to_string());
        }
        return Err("多项式拟合至少需要 2 个坐标".to_string());
    }
    // 坐标求值（保持精确）
    let mut pts: Vec<(bool, Number, Number)> = Vec::with_capacity(points.len());
    for p in points {
        let x = evaluator.evaluate_with_vars(&p.x, &[])?;
        let y = evaluator.evaluate_with_vars(&p.y, &[])?;
        pts.push((p.is_vertex, x, y));
    }
    let n_vertex = pts.iter().filter(|(v, _, _)| *v).count();
    let n_plain = pts.len() - n_vertex;
    if n_vertex > 0 && n_plain == 0 {
        return Err("顶点坐标还需要至少一个普通坐标".to_string());
    }
    // 约束个数：普通点 1 个，顶点 2 个（f(a)=b 与 f'(a)=0）
    let constraints = n_plain + 2 * n_vertex;

    match template {
        None => solve_plain(&pts, constraints),
        Some(t) => solve_with_template(&pts, constraints, t, evaluator),
    }
}

/// 无模板：范德蒙矩阵 + 顶点解析导数行，解唯一
fn solve_plain(pts: &[(bool, Number, Number)], constraints: usize) -> Result<FitSolution, String> {
    if constraints < 2 {
        return Err("多项式拟合至少需要 2 个坐标".to_string());
    }
    let degree = constraints - 1;
    if !calc_mode::is_deep() && constraints > FIT_MAX_SIZE_FAST {
        return Err(format!(
            "坐标/参数过多（{0} 个，Fast 模式上限 {1}）；如确认需要继续，请先执行 /mode deep",
            constraints, FIT_MAX_SIZE_FAST
        ));
    }
    let cols = degree + 1;
    let mut matrix: Vec<Vec<Number>> = Vec::with_capacity(constraints);
    for (is_vertex, x, y) in pts {
        // f(x) = y： [1, x, x², …, x^degree | y]
        let mut row = vec![Number::from_int(1)];
        let mut pow = Number::from_int(1);
        for _ in 1..=degree {
            pow = pow.mul(x);
            row.push(pow.clone());
        }
        row.push(y.clone());
        matrix.push(row);
        if *is_vertex {
            // f'(x) = 0： [0, 1, 2x, 3x², …, degree·x^(degree-1) | 0]
            let mut row: Vec<Number> = Vec::with_capacity(cols + 1);
            let mut pow = Number::from_int(1);
            for k in 0..=degree {
                if k == 0 {
                    row.push(Number::from_int(0));
                } else {
                    row.push(pow.mul(&Number::from_int(k as i64)));
                    pow = pow.mul(x);
                }
            }
            row.push(Number::from_int(0));
            matrix.push(row);
        }
    }
    let dummy: Vec<char> = (0..cols)
        .map(|i| char::from(b'a' + (i % 26) as u8))
        .collect();
    let sol = crate::solver_linear::gaussian_elimination(&mut matrix, &dummy)
        .ok_or_else(|| "坐标之间存在矛盾，无法同时满足".to_string())?;
    if !sol.unique {
        return Err("坐标重复或退化，无法唯一确定多项式".to_string());
    }
    if sol.values.len() != cols {
        return Err("坐标重复或退化，无法唯一确定多项式".to_string());
    }
    let coeffs: Vec<LinForm> = sol
        .values
        .iter()
        .map(|(_, v)| LinForm::constant(v.clone()))
        .collect();
    Ok(FitSolution {
        params: Vec::new(),
        free: Vec::new(),
        coeffs,
        var: 'x',
    })
}

/// 有模板：单位向量代入法把模板线性化，再用自写 RREF 解出"参数 = 自由参数的线性形式"
fn solve_with_template(
    pts: &[(bool, Number, Number)],
    constraints: usize,
    t: &FitTemplate,
    evaluator: &Evaluator,
) -> Result<FitSolution, String> {
    let var = t.var;
    if !expr_has_var(&t.rhs, var) {
        return Err("模板缺少自变量 x".to_string());
    }
    let unknowns = collect_unknowns(&t.rhs, var, evaluator);
    if unknowns.is_empty() {
        return Err("模板中没有未知参数".to_string());
    }
    let m = unknowns.len();
    if !calc_mode::is_deep() && constraints.max(m) > FIT_MAX_SIZE_FAST {
        return Err(format!(
            "坐标/参数过多（{0} 个，Fast 模式上限 {1}）；如确认需要继续，请先执行 /mode deep",
            constraints.max(m),
            FIT_MAX_SIZE_FAST
        ));
    }

    // 基函数 φ_j(x) 与"参数全零"部分 g(x)：都用单位向量代入模板后提取升幂系数（数值、精确）
    let mut basis: Vec<Vec<Number>> = Vec::with_capacity(m);
    for j in 0..m {
        let ev = evaluator_with(evaluator, &unknowns, Some((j, Number::from_int(1))));
        basis.push(extract_poly(&ev, &t.rhs, var)?);
    }
    let ev0 = evaluator_with(evaluator, &unknowns, None);
    let g = extract_poly(&ev0, &t.rhs, var)?;
    // φ_j = （只把 p_j 取 1 的取值）− g。**不能直接用"只有 p_j=1"的取值当系数函数**：
    // 其它参数在模板里仍取 0，若模板含常数偏移（如 `A*x + b` 里的 A*x 项），差值才是真正的系数。
    for b in basis.iter_mut() {
        *b = poly_sub(b, &g);
    }

    // 线性性检验：模板必须"对参数线性"。取一组固定样本值，比较"直接代入模板"
    // 与"线性组合 g + Σ p_j·φ_j"在若干个 x 上的取值；不一致说明模板对参数非线性（如 a*b*x、a^2*x）。
    check_param_linearity(&t.rhs, var, &unknowns, &basis, &g, evaluator)?;

    // 约束矩阵（元素为 Number）：普通点 Σ p_j φ_j(a) = y - g(a)；顶点追加 Σ p_j φ_j'(a) = -g'(a)
    let cols = m;
    let mut matrix: Vec<Vec<Number>> = Vec::with_capacity(constraints);
    for (is_vertex, x, y) in pts {
        let gx = poly_eval(&g, x);
        let mut row: Vec<Number> = basis.iter().map(|b| poly_eval(b, x)).collect();
        row.push(y.sub(&gx));
        matrix.push(row);
        if *is_vertex {
            let gd = poly_eval_deriv(&g, x);
            let mut row: Vec<Number> = basis.iter().map(|b| poly_eval_deriv(b, x)).collect();
            row.push(gd.neg());
            matrix.push(row);
        }
    }

    // RREF + 把主元参数解成自由参数的线性形式
    let pivots = rref(&mut matrix, cols)?; // 矛盾（全零行 + 非零右端）在此报错
    let mut forms: Vec<LinForm> = unknowns.iter().map(|n| LinForm::free(n)).collect();
    for (row, col) in &pivots {
        let mut form = LinForm::constant(matrix[*row][cols].clone());
        for (j, name) in unknowns.iter().enumerate() {
            if pivots.iter().any(|(_, c)| c == &j) {
                continue;
            }
            let coeff = matrix[*row][j].clone();
            if !coeff.is_zero() {
                form = form.sub(&LinForm::free(name).scale(&coeff));
            }
        }
        forms[*col] = form;
    }
    let free: Vec<String> = unknowns
        .iter()
        .enumerate()
        .filter(|(j, _)| !pivots.iter().any(|(_, c)| c == j))
        .map(|(_, n)| n.clone())
        .collect();

    // 解析式 = g(x) + Σ p_j·φ_j(x)，按次数相加 ⇒ 自动展开合并同类项
    let mut coeffs: Vec<LinForm> = g.iter().map(|c| LinForm::constant(c.clone())).collect();
    for (j, form) in forms.iter().enumerate() {
        for (d, c) in basis[j].iter().enumerate() {
            let add = form.scale(c);
            coeffs[d] = coeffs[d].add(&add);
        }
    }
    // 去掉最高次的高位零系数（保持降幂渲染整洁）
    while coeffs.len() > 1
        && coeffs
            .last()
            .map(|c| c.render(DisplayMode::MathIO) == "0")
            .unwrap_or(false)
    {
        coeffs.pop();
    }

    // 复验：把参数解代回原模板（数值抽样）看是否满足每个坐标
    verify_solution(&t.rhs, var, &unknowns, &forms, evaluator, pts)?;

    let params: Vec<(String, LinForm)> = unknowns.iter().cloned().zip(forms.into_iter()).collect();
    Ok(FitSolution {
        params,
        free,
        coeffs,
        var,
    })
}

/// 是否包含自变量
fn expr_has_var(expr: &Expr, var: char) -> bool {
    match expr {
        Expr::Variable(name) => name == &var.to_string(),
        Expr::Number(_) => false,
        Expr::Binary(a, _, b) => expr_has_var(a, var) || expr_has_var(b, var),
        Expr::Unary(_, e) => expr_has_var(e, var),
        Expr::Pow(a, b) => expr_has_var(a, var) || expr_has_var(b, var),
        Expr::Function(_, args) => args.iter().any(|a| expr_has_var(a, var)),
        Expr::Sd(e) | Expr::Factor(e) => expr_has_var(e, var),
        Expr::Equation(a, b) => expr_has_var(a, var) || expr_has_var(b, var),
        Expr::System(es) => es.iter().any(|e| expr_has_var(e, var)),
    }
}

/// 收集未知参数：模板里出现的 `Variable`，排除自变量、`ans`、常数与 `/let` 已存变量。
/// （`pi/e/i/tau/phi` 在解析期就变成 `Number`，不会出现在这里。）
fn collect_unknowns(expr: &Expr, var: char, evaluator: &Evaluator) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    collect_unknowns_rec(expr, var, evaluator, &mut out);
    out
}

fn collect_unknowns_rec(expr: &Expr, var: char, evaluator: &Evaluator, out: &mut Vec<String>) {
    match expr {
        Expr::Variable(name) => {
            if name.len() == 1 && name.starts_with(var) {
                return;
            }
            if name == "ans" || evaluator.vars.contains_key(name) {
                return;
            }
            if !out.contains(name) {
                out.push(name.clone());
            }
        }
        Expr::Number(_) => {}
        Expr::Binary(a, _, b) => {
            collect_unknowns_rec(a, var, evaluator, out);
            collect_unknowns_rec(b, var, evaluator, out);
        }
        Expr::Unary(_, e) => collect_unknowns_rec(e, var, evaluator, out),
        Expr::Pow(a, b) => {
            collect_unknowns_rec(a, var, evaluator, out);
            collect_unknowns_rec(b, var, evaluator, out);
        }
        Expr::Function(_, args) => {
            for a in args {
                collect_unknowns_rec(a, var, evaluator, out);
            }
        }
        Expr::Sd(e) | Expr::Factor(e) => collect_unknowns_rec(e, var, evaluator, out),
        Expr::Equation(a, b) => {
            collect_unknowns_rec(a, var, evaluator, out);
            collect_unknowns_rec(b, var, evaluator, out);
        }
        Expr::System(es) => {
            for e in es {
                collect_unknowns_rec(e, var, evaluator, out);
            }
        }
    }
}

/// 克隆一个 Evaluator，并把未知参数按给定取值注入（`set` 为 None 时全部取 0）
fn evaluator_with(
    base: &Evaluator,
    unknowns: &[String],
    set: Option<(usize, Number)>,
) -> Evaluator {
    let mut ev = Evaluator::new();
    ev.display_mode = base.display_mode;
    ev.angle_mode = base.angle_mode;
    ev.ans = base.ans.clone();
    ev.vars = base.vars.clone();
    for (i, name) in unknowns.iter().enumerate() {
        let v = match &set {
            Some((j, val)) if *j == i => val.clone(),
            _ => Number::from_int(0),
        };
        ev.vars.insert(name.clone(), v);
    }
    ev
}

/// 提取模板的升幂系数（失败 ⇒ 模板不是关于 x 的多项式）
fn extract_poly(ev: &Evaluator, rhs: &Expr, var: char) -> Result<Vec<Number>, String> {
    solver_poly::extract_polynomial(ev, rhs, var)
        .ok_or_else(|| "模板不是多项式，无法提取升幂系数".to_string())
}

/// 多项式减法（长度自动对齐到较长者，短者高位补 0）
fn poly_sub(a: &[Number], b: &[Number]) -> Vec<Number> {
    let n = a.len().max(b.len());
    let zero = Number::from_int(0);
    (0..n)
        .map(|i| {
            let ai = a.get(i).cloned().unwrap_or_else(|| zero.clone());
            let bi = b.get(i).cloned().unwrap_or_else(|| zero.clone());
            ai.sub(&bi)
        })
        .collect()
}

/// 多项式求值（Horner）
fn poly_eval(coeffs: &[Number], x: &Number) -> Number {
    let mut acc = Number::from_int(0);
    for c in coeffs.iter().rev() {
        acc = acc.mul(x).add(c);
    }
    acc
}

/// 多项式求导后在 x 处取值（解析：Σ k·c_k·x^(k−1)）
fn poly_eval_deriv(coeffs: &[Number], x: &Number) -> Number {
    let mut acc = Number::from_int(0);
    for (k, c) in coeffs.iter().enumerate().skip(1) {
        let mut pow = Number::from_int(1);
        for _ in 1..k {
            pow = pow.mul(x);
        }
        acc = acc.add(&c.mul(&Number::from_int(k as i64)).mul(&pow));
    }
    acc
}

/// 模板对参数是否"线性"的检验：取固定样本参数值，比较"直接代入模板"与"线性组合"在若干 x 上的取值。
/// 这是必要的防御——单位向量代入法只对"参数线性"的模板成立（如 `a*b*x + c`、`a^2*x + b` 不成立）。
fn check_param_linearity(
    rhs: &Expr,
    var: char,
    unknowns: &[String],
    basis: &[Vec<Number>],
    g: &[Number],
    evaluator: &Evaluator,
) -> Result<(), String> {
    // 样本参数值：确定性取值（2、3、5… 交替），避免引入随机性
    let samples = [2i64, 3, 5, 7, 11];
    let xs = [Number::from_int(2), Number::from_int(-3)];
    let mut values: Vec<Number> = Vec::with_capacity(unknowns.len());
    let mut ev = evaluator_with(evaluator, unknowns, None);
    for (i, name) in unknowns.iter().enumerate() {
        let v = Number::from_int(samples[i % samples.len()]);
        ev.vars.insert(name.clone(), v.clone());
        values.push(v);
    }
    for x in &xs {
        let direct = ev.evaluate_with_var(rhs, &var.to_string(), x)?;
        let mut combo = poly_eval(g, x);
        for (j, v) in values.iter().enumerate() {
            combo = combo.add(&poly_eval(&basis[j], x).mul(v));
        }
        if direct.sub(&combo).abs().to_approx().value.abs() > number_tol() {
            return Err("模板与坐标不一致（可能对参数不是线性的）".to_string());
        }
    }
    Ok(())
}

/// 复验：把参数解代回模板（数值抽样：自由参数取几组确定值），检查每个坐标都满足
fn verify_solution(
    rhs: &Expr,
    var: char,
    unknowns: &[String],
    forms: &[LinForm],
    evaluator: &Evaluator,
    pts: &[(bool, Number, Number)],
) -> Result<(), String> {
    let sample_sets: [&[i64]; 3] = [&[0, 0, 0, 0, 0], &[1, 2, 3, 4, 5], &[-2, 3, -5, 7, -11]];
    for samples in sample_sets {
        let mut ev = evaluator_with(evaluator, unknowns, None);
        for (j, name) in unknowns.iter().enumerate() {
            let mut v = Number::from_int(0);
            for (i, c) in forms[j].terms.iter() {
                let idx = unknowns.iter().position(|n| n == i).unwrap_or(0);
                let s = samples[idx % samples.len()];
                v = v.add(&c.mul(&Number::from_int(s)));
            }
            v = v.add(&forms[j].k);
            ev.vars.insert(name.clone(), v);
        }
        for (is_vertex, x, y) in pts {
            let f = ev.evaluate_with_var(rhs, &var.to_string(), x)?;
            if f.sub(y).abs().to_approx().value.abs() > number_tol() {
                return Err("模板与坐标不一致（可能对参数不是线性的）".to_string());
            }
            if *is_vertex {
                // 顶点：导数必须为 0（数值差分，容差放宽）
                let h = Number::Approx(hipercalc_core::bigfloat::BigFloat::div(
                    &hipercalc_core::bigfloat::BigFloat::from_u64(1),
                    &num_bigint::BigInt::from(10).pow(20).into(),
                    hipercalc_core::bigfloat::precision(),
                ));
                let fp = ev.evaluate_with_var(rhs, &var.to_string(), &x.add(&h))?;
                let fm = ev.evaluate_with_var(rhs, &var.to_string(), &x.sub(&h))?;
                let d = fp.sub(&fm).div(&h.mul(&Number::from_int(2)));
                if d.abs().to_approx().value.abs() > number_tol() {
                    return Err("模板与坐标不一致（可能对参数不是线性的）".to_string());
                }
            }
        }
    }
    Ok(())
}

/// 数值容差（以内部尾数刻度表示）：允许误差约 10^(−3·precision/4)（默认 80 位 → 1e-60），
/// 远小于显示精度 1e-20，因此既能容忍近似坐标带来的微小误差，又能拦住真正的"不一致"。
fn number_tol() -> num_bigint::BigInt {
    num_bigint::BigInt::from(10).pow((hipercalc_core::bigfloat::precision() * 3 / 4) as u32)
}

/// 对增广矩阵做完全行化简（RREF），返回"主元列 → 所在行"的列表；
/// 出现"全零系数行 + 非零右端"⇒ 矛盾（Err）。列数 `cols`，最后一列为右端。
fn rref(matrix: &mut Vec<Vec<Number>>, cols: usize) -> Result<Vec<(usize, usize)>, String> {
    let rows = matrix.len();
    let mut pivots: Vec<(usize, usize)> = Vec::new();
    let mut row = 0usize;
    for col in 0..cols {
        if row >= rows {
            break;
        }
        // 选主元（精确运算，任意非零即可；优先取一个非零行）
        let mut pick = None;
        for r in row..rows {
            if !matrix[r][col].is_zero() {
                pick = Some(r);
                break;
            }
        }
        let Some(pr) = pick else { continue };
        matrix.swap(row, pr);
        // 归一化
        let pivot = matrix[row][col].clone();
        for j in col..=cols {
            matrix[row][j] = matrix[row][j].div(&pivot);
        }
        // 消去其它行
        for r in 0..rows {
            if r == row {
                continue;
            }
            let factor = matrix[r][col].clone();
            if factor.is_zero() {
                continue;
            }
            for j in col..=cols {
                let sub = factor.mul(&matrix[row][j]);
                matrix[r][j] = matrix[r][j].sub(&sub);
            }
        }
        pivots.push((row, col));
        row += 1;
    }
    // 矛盾检查
    for r in 0..rows {
        let all_zero = (0..cols).all(|c| matrix[r][c].is_zero());
        if all_zero && !matrix[r][cols].is_zero() {
            return Err("坐标之间存在矛盾，无法同时满足".to_string());
        }
    }
    Ok(pivots)
}

/* ---------------- 渲染 ---------------- */

/// 一般式：升幂系数 → 降幂文本，如 `x^2 + 2*x + 4`、`(a + b)*x + c`
pub fn format_polynomial(coeffs: &[LinForm], var: char, mode: DisplayMode) -> String {
    let mut terms: Vec<String> = Vec::new();
    for (d, c) in coeffs.iter().enumerate().rev() {
        if c.render(mode) == "0" {
            continue;
        }
        let one = Number::from_int(1);
        let neg_one = Number::from_int(-1);
        let is_one = c.is_constant() && number_eq(c.as_constant().unwrap(), &one);
        let is_neg_one = c.is_constant() && number_eq(c.as_constant().unwrap(), &neg_one);
        let body = match (d, is_one, is_neg_one) {
            (0, _, _) => c.render(mode),
            (1, true, _) => var.to_string(),
            (1, _, true) => format!("-{}", var),
            (1, _, _) => format!("{}*{}", c.render_coeff(mode), var),
            (_, true, _) => format!("{}^{}", var, d),
            (_, _, true) => format!("-{}^{}", var, d),
            (_, _, _) => format!("{}*{}^{}", c.render_coeff(mode), var, d),
        };
        terms.push(body);
    }
    if terms.is_empty() {
        return "0".to_string();
    }
    let mut out = terms[0].clone();
    for t in &terms[1..] {
        if let Some(rest) = t.strip_prefix('-') {
            out.push_str(" - ");
            out.push_str(rest);
        } else {
            out.push_str(" + ");
            out.push_str(t);
        }
    }
    out
}

/// 顶点式 `y = a*(x - m)^2 + k`（仅"系数全为常数、实际次数恰为 2、首项非 0"时给出）
pub fn format_vertex_form(coeffs: &[LinForm], var: char, mode: DisplayMode) -> Option<String> {
    if coeffs.len() != 3 {
        return None;
    }
    let a = coeffs[2].as_constant()?.clone();
    let b = coeffs[1].as_constant()?.clone();
    let c = coeffs[0].as_constant()?.clone();
    if a.is_zero() {
        return None;
    }
    // m = -b/(2a)，k = c - b²/(4a)
    let m = b.neg().div(&a.mul(&Number::from_int(2)));
    let k = c.sub(&b.mul(&b).div(&a.mul(&Number::from_int(4))));
    let one = Number::from_int(1);
    let neg_one = Number::from_int(-1);
    // m == 0 时顶点式退化为 a*x² + k，与一般式逐字相同 ⇒ 不重复输出
    if m.is_zero() {
        return None;
    }
    let a_part = if number_eq(&a, &one) {
        String::new()
    } else if number_eq(&a, &neg_one) {
        "-".to_string()
    } else {
        let a_str = display_part(&a, mode);
        if a_str.contains(' ') || a_str.contains('/') {
            format!("({})*", a_str)
        } else {
            format!("{}*", a_str)
        }
    };
    let body = if m.is_negative() {
        format!("{}({} + {})^2", a_part, var, display_part(&m.neg(), mode))
    } else {
        format!("{}({} - {})^2", a_part, var, display_part(&m, mode))
    };
    let tail = if k.is_zero() {
        String::new()
    } else if k.is_negative() {
        format!(" - {}", display_part(&k.neg(), mode))
    } else {
        format!(" + {}", display_part(&k, mode))
    };
    Some(format!("{}{}", body, tail))
}

/// 参数行：唯一解时 `a = 1, b = 1, c = 1`；欠定时每行一个关系式（由调用方逐行输出）
pub fn format_fit_params(sol: &FitSolution, mode: DisplayMode) -> Vec<String> {
    if sol.params.is_empty() {
        return Vec::new();
    }
    if sol.free.is_empty() {
        // 唯一解：单行拼起来
        let items: Vec<String> = sol
            .params
            .iter()
            .map(|(name, form)| {
                let v = form.k.clone();
                format!(
                    "{} {} {}",
                    name,
                    solve_aux::result_prefix(&v, mode),
                    display_part(&v, mode)
                )
            })
            .collect();
        return vec![items.join(", ")];
    }
    // 欠定：主元参数逐行给关系；自由参数单独提示
    let mut lines: Vec<String> = Vec::new();
    for (name, form) in &sol.params {
        if sol.free.iter().any(|f| f == name) {
            continue;
        }
        lines.push(format!("{} = {}", name, form.render(mode)));
    }
    lines.push(format!("自由参数: {}", sol.free.join(", ")));
    lines
}

/// 解析式行：恒为 `y = <一般式>`（含自由参数时同样成立）
pub fn format_fit_solution(sol: &FitSolution, mode: DisplayMode) -> String {
    let body = format_polynomial(&sol.coeffs, sol.var, mode);
    let prefix = if sol.coeffs.iter().all(|c| prefix_of_form(c, mode) == "=") {
        "="
    } else {
        "≈"
    };
    format!("y {} {}", prefix, body)
}

/// 单个线性形式的精确性前缀
fn prefix_of_form(form: &LinForm, mode: DisplayMode) -> &'static str {
    if let Some(k) = form.as_constant() {
        solve_aux::result_prefix(k, mode)
    } else {
        "="
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{EvalResult, parse_and_eval};

    /// 跑一行拟合输入，按 `handle_fit` 的规则拼装输出行
    fn run_with(
        input: &str,
        mode: DisplayMode,
        vars: &[(&str, i64)],
    ) -> Result<Vec<String>, String> {
        let mut ev = Evaluator::new();
        ev.display_mode = mode;
        for (name, v) in vars {
            ev.vars.insert((*name).to_string(), Number::from_int(*v));
        }
        match parse_and_eval(input, &mut ev)? {
            EvalResult::Fit(fit) => {
                let sol = solve_polynomial_fit(&fit.points, fit.template.as_ref(), &ev)?;
                let mut lines = format_fit_params(&sol, mode);
                lines.push(format_fit_solution(&sol, mode));
                if sol.all_constant() {
                    if let Some(v) = format_vertex_form(&sol.coeffs, sol.var, mode) {
                        lines.push(format!("y = {}", v));
                    }
                }
                Ok(lines)
            }
            _ => Err("输入未被识别为拟合".to_string()),
        }
    }

    fn mathio(input: &str) -> Vec<String> {
        run_with(input, DisplayMode::MathIO, &[]).unwrap()
    }

    fn lineio(input: &str) -> Vec<String> {
        run_with(input, DisplayMode::LineIO, &[]).unwrap()
    }

    fn err(input: &str) -> String {
        run_with(input, DisplayMode::MathIO, &[]).unwrap_err()
    }

    #[test]
    fn plain_fit_linear_quadratic_cubic() {
        assert_eq!(mathio("(1,2) (3,4)"), vec!["y = x + 1"]);
        assert_eq!(
            mathio("(0,1) (1,3) (2,7)"),
            vec!["y = x^2 + x + 1", "y = (x + 1 / 2)^2 + 3 / 4"]
        );
        // 三次：无顶点式行
        assert_eq!(mathio("(0,0) (1,1) (2,8) (3,27)"), vec!["y = x^3"]);
        // 分数/负号坐标
        assert_eq!(mathio("(0,0) (1,1/2) (2,2)"), vec!["y = (1 / 2)*x^2"]);
        assert_eq!(mathio("(0,0) (3,1)"), vec!["y = (1 / 3)*x"]);
        assert_eq!(lineio("(0,0) (3,1)"), vec!["y ≈ 0.33333333333333333333*x"]);
    }

    #[test]
    fn vertex_point_counts_as_two_constraints() {
        // P(1,3) 提供 f(1)=3 与 f'(1)=0 ⇒ 再加 1 个点即可定二次
        assert_eq!(
            mathio("P(1,3) (0,0)"),
            vec!["y = -3*x^2 + 6*x", "y = -3*(x - 1)^2 + 3"]
        );
        // P + 2 个点 ⇒ 4 个约束 ⇒ 三次（无顶点式）
        assert_eq!(
            mathio("P(1,3) (0,1) (2,5)"),
            vec!["y = 2*x^3 - 6*x^2 + 6*x + 1"]
        );
        // 3 点共线 ⇒ 实际一次，不输出顶点式
        assert_eq!(mathio("(0,0) (1,1) (2,2)"), vec!["y = x"]);
    }

    #[test]
    fn template_solves_unknown_parameters() {
        assert_eq!(
            mathio("(0,1) (1,3) (2,7) y = a*x^2+b*x+c"),
            vec![
                "a = 1, b = 1, c = 1",
                "y = x^2 + x + 1",
                "y = (x + 1 / 2)^2 + 3 / 4"
            ]
        );
        // 左值只是装饰（`f(x) =` 被忽略，输出恒为 y = …）
        assert_eq!(
            mathio("(0,0) (1,2) f(x) = a*x + b"),
            vec!["a = 2, b = 0", "y = 2*x"]
        );
        // 其他参数名（大写、多字母）同样可解
        assert_eq!(
            mathio("(0,0) (1,2) y = k*x + m"),
            vec!["k = 2, m = 0", "y = 2*x"]
        );
    }

    #[test]
    fn known_parameters_from_let_are_kept() {
        assert_eq!(
            run_with("(0,1) (1,3) y = A*x + b", DisplayMode::MathIO, &[("A", 2)]).unwrap(),
            vec!["b = 1", "y = 2*x + 1"]
        );
        // A 固定为 2 时，三点必然矛盾（数据实际对应 a=1）
        assert!(
            run_with(
                "(0,1) (1,3) (2,7) y = A*x^2+b*x+c",
                DisplayMode::MathIO,
                &[("A", 2)]
            )
            .unwrap_err()
            .contains("矛盾")
        );
    }

    #[test]
    fn underdetermined_prints_relations_and_symbolic_expression() {
        // 2 点 3 参：主元参数用自由参数表达，解析式也跟着化简
        let lines = mathio("(0,1) (1,3) y = a*x^2+b*x+c");
        assert_eq!(lines.len(), 4, "{lines:?}");
        assert!(lines[3].starts_with("y = "), "{lines:?}");
        assert!(lines[3].contains("*x^2"), "{lines:?}");
        assert!(
            lines.iter().any(|l| l.starts_with("自由参数: ")),
            "{lines:?}"
        );
        // 模板里同类项合并（a*x^2 + a*x + b*x + c ⇒ x 的系数是 a + b）
        let lines = mathio("(0,1) (1,4) y = a*x^2 + a*x + b*x + c");
        assert!(lines[3].contains("x^2"), "{lines:?}");
        assert!(lines[3].contains("*x "), "{lines:?}");
    }

    #[test]
    fn template_needs_polynomial_and_variable() {
        assert!(err("(0,0) (1,1) y = a*sin(x) + b").contains("不是多项式"));
        assert!(err("(0,0) (1,1) y = a + b").contains("自变量"));
        assert!(err("(0,0) (1,1) y = x^2").contains("没有未知参数"));
    }

    #[test]
    fn error_cases() {
        assert!(err("(1,2)").contains("至少需要 2 个坐标"));
        assert!(err("P(1,3)").contains("顶点坐标还需要至少一个普通坐标"));
        assert!(err("(1,2) (1,2)").contains("重复或退化"));
        assert!(err("(1,2) (1,3)").contains("矛盾"));
        assert!(err("p(1,2) (3,4)").contains("大写 P"));
        // Fast 模式规模护栏（17 个坐标 ⇒ 17 次）
        let many = (0..17)
            .map(|i| format!("({i},{})", i * i))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(err(&many).contains("Fast 模式上限"));
    }

    #[test]
    fn non_fit_inputs_are_left_alone() {
        // 这些都不应被识别为拟合（回退原有解析路径）
        for input in ["(1+2)*3", "f(1,2)", "P(2)", "P+1", "2P", "(1,2)+3"] {
            assert!(
                run_with(input, DisplayMode::MathIO, &[]).is_err(),
                "{input} 不应被识别为拟合"
            );
        }
    }
}
