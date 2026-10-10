//! 三角形求解（输入边 / 角 / 高，输出全部量）
//!
//! 输入形如 `triangle(a=3 b=4 c=5)`、`triangle(A=30 b=5 C=60)`、`triangle(hA=4 a=3 b=4)`。
//! 记号：**小写字母是边，大写字母是角**（`a` 与 `A` 是同一个顶点）；`h` + 大写字母是该边上的高
//! （`hA` 为 `a` 边上的高）。字母不限于 `a,b,c`：`triangle(x=3 y=4 z=5)` 与 `triangle(a=3 b=4 c=5)`
//! 等价，顶点按输入里**首次出现的顺序**排定，不足三个时按拉丁字母顺序补齐（`x=1 y=2` → 第三顶点 `z`）。
//! 角度单位跟随当前 `/mode deg|rad`。
//!
//! 三条设计约定：
//! - **内部角度一律弧度**：输入按 `AngleMode` 换算（`deg` 时乘 `π/180`，保持精确），
//!   输出前再用 `parser::radians_to_degrees` 精确转回（`Pi(coeff) → Rational(coeff*180)`，
//!   故 `π/2` 在度模式下精确显示为 `90`）；反三角必须**先**走 `trig::try_exact_*` 再用它转换，
//!   否则 `arccos(0) = π/2` 会退化成 `≈ 90`。
//! - **解析优先、数值兜底**：SSS/SAS/ASA/AAS/SSA 与"高可归约"的情形走公式推导
//!   （全程 `Number` 精确运算，能精确呈现就用 `=`）；其余组合用 `(A,B,s=2R)` 参数化的
//!   多起点 Gauss-Newton 求解，多解（如 SSA 的两解）全部列出。
//! - **已知量是"真值"**：解出后必回代校验每个已知量（超定矛盾要报错），校验通过后
//!   把已知量原样写回结果（`A=30` 就显示 `30`，而不是 `29.9999…`）。

use num_bigint::BigInt;
use num_integer::Integer;
use num_rational::BigRational;
use num_traits::{One, Zero};

use crate::parser::{self, DisplayMode, Evaluator, Expr};
use crate::solve_aux;
use hipercalc_core::bigfloat::{self, BigFloat};
use hipercalc_core::display;
use hipercalc_core::number::Number;
use hipercalc_core::trig::{self, AngleMode};

/* ---------------- 记号 ---------------- */

/// 三角形的一个"零件"：下标 0/1/2 分别是第 0/1/2 个顶点 —— 边是对边、角是顶角、高是对边上的高
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriPart {
    /// 边（第 i 个顶点所对的边）
    Side(u8),
    /// 角（第 i 个顶点处的角）
    Angle(u8),
    /// 高（第 i 个顶点所对的边上的高）
    Height(u8),
}

/// 记号的角色：小写字母 = 边、大写字母 = 角、`h`/`H` + 字母 = 高
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriRole {
    Side,
    Angle,
    Height,
}

/// 拉丁小写字母表（补齐顶点字母时按它顺延）
const LATIN: [char; 26] = [
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's',
    't', 'u', 'v', 'w', 'x', 'y', 'z',
];

/// 字母的"小写规范形"；无大小写区分（汉字、数字…）或大小写映射不是单字符（如 `ß`）时返回 `None`
fn lower_form(c: char) -> Option<char> {
    let mut lo = c.to_lowercase();
    let l = lo.next()?;
    if lo.next().is_some() {
        return None;
    }
    let mut up = c.to_uppercase();
    let u = up.next()?;
    if up.next().is_some() || u == l {
        return None;
    }
    Some(l)
}

/// 是否写成大写形式（`A` 是、`a` 不是）
fn is_upper_form(c: char) -> bool {
    matches!(c.to_uppercase().next(), Some(u) if u == c)
}

/// 三角形顶点的字母标注。
///
/// `verts[i]` 是第 i 个顶点的字母，边 / 角 / 高都由它派生：
/// 顶点 `verts[i]`、对角 `uppercase(verts[i])`、高 `h + uppercase(verts[i])`。
/// 经典记号 `a,b,c / A,B,C / hA,hB,hC` 只是 `verts = [a, b, c]` 的特例。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Alphabet {
    verts: [char; 3],
}

impl Alphabet {
    /// 由输入里出现过的顶点字母（`lower_form` 规范形，按出现顺序）构造字母表。
    /// 三个以上不同字母直接报错；不足三个时从最后一个字母往后顺延补齐（跳过已用过的）。
    pub fn from_letters(letters: &[char]) -> Result<Self, String> {
        let mut verts: Vec<char> = Vec::new();
        for &c in letters {
            if !verts.contains(&c) {
                verts.push(c);
            }
        }
        if verts.len() > 3 {
            return Err(format!(
                "三角形只有三个顶点，但出现了 {0} 个不同的字母: {1}",
                verts.len(),
                verts.iter().collect::<String>()
            ));
        }
        while verts.len() < 3 {
            let start = verts
                .last()
                .and_then(|last| LATIN.iter().position(|c| c == last))
                .map_or(0, |i| (i + 1) % LATIN.len());
            let picked = (0..LATIN.len())
                .map(|k| LATIN[(start + k) % LATIN.len()])
                .find(|c| !verts.contains(c));
            let Some(c) = picked else {
                return Err("三角形只有三个顶点，但出现了多个不同的字母".to_string());
            };
            verts.push(c);
        }
        Ok(Alphabet {
            verts: [verts[0], verts[1], verts[2]],
        })
    }

    /// 顶点字母（小写规范形）→ 下标
    pub fn index_of(&self, vert: char) -> Option<usize> {
        self.verts.iter().position(|c| *c == vert)
    }

    /// 第 i 条边的名字（= 第 i 个顶点的字母）
    pub fn side_label(&self, i: usize) -> String {
        self.verts[i].to_string()
    }

    /// 第 i 个角的名字（= 顶点字母的大写）
    pub fn angle_label(&self, i: usize) -> String {
        self.verts[i].to_uppercase().collect()
    }

    /// 第 i 条高的名字（`h` + 顶点大写字母）
    pub fn height_label(&self, i: usize) -> String {
        format!("h{0}", self.angle_label(i))
    }
}

impl TriPart {
    /// 下标（0/1/2）
    pub fn index(self) -> usize {
        match self {
            TriPart::Side(i) | TriPart::Angle(i) | TriPart::Height(i) => i as usize,
        }
    }

    /// 显示名（错误提示用）
    pub fn name(self, alpha: &Alphabet) -> String {
        let i = self.index();
        match self {
            TriPart::Side(_) => alpha.side_label(i),
            TriPart::Angle(_) => alpha.angle_label(i),
            TriPart::Height(_) => alpha.height_label(i),
        }
    }
}

/// 记号名 → (角色, 顶点字母的小写规范形)。
///
/// - 单字母：小写 = 边、大写 = 角（`a` 与 `A` 是同一顶点的边与角）；
/// - `h`/`H` + 单字母 = 该顶点所对边上的高（`hA` 是 `a` 边上的高）。
///
/// 字母不限 ASCII：`α/Α`、`β/Β` 这类有大小写区分的字母同样可用；汉字等无大小写区分的记号报错。
pub fn parse_part_name(name: &str) -> Result<(TriRole, char), String> {
    let chars: Vec<char> = name.chars().collect();
    let (fixed, letter) = match chars.as_slice() {
        [c] => (None, *c),
        ['h' | 'H', c] => (Some(TriRole::Height), *c),
        _ => return Err(format!("未知的三角形记号: {0}", name)),
    };
    let Some(vert) = lower_form(letter) else {
        return Err(format!("未知的三角形记号: {0}", name));
    };
    let role = match fixed {
        Some(r) => r,
        None if is_upper_form(letter) => TriRole::Angle,
        None => TriRole::Side,
    };
    Ok((role, vert))
}

/// 一行三角形输入（解析期不求值，保留 `pi`/分数等精确形式）
#[derive(Debug, Clone)]
pub struct TriangleInput {
    pub parts: Vec<(TriPart, Expr)>,
    /// 顶点字母（由用户写下的记号决定）
    pub alphabet: Alphabet,
}

/* ---------------- 结果 ---------------- */

/// 求解结果（角度内部为弧度）
#[derive(Debug, Clone)]
pub struct TriangleSolution {
    /// 输出标签用的顶点字母
    pub alphabet: Alphabet,
    pub a: Number,
    pub b: Number,
    pub c: Number,
    pub angle_a: Number,
    pub angle_b: Number,
    pub angle_c: Number,
    pub ha: Number,
    pub hb: Number,
    pub hc: Number,
    pub area: Number,
    pub perimeter: Number,
    pub circumradius: Number,
    pub inradius: Number,
}

/// 三角形的"基本三元组"：三边 + 三角（弧度）+ 面积；其余量都由它派生
#[derive(Debug, Clone)]
struct TriVals {
    sides: [Number; 3],
    angles: [Number; 3],
    area: Number,
}

/* ---------------- 已知量 ---------------- */

/// 归约过程中的部分已知信息（角度为弧度）
#[derive(Debug, Clone)]
struct Known {
    side: [Option<Number>; 3],
    angle: [Option<Number>; 3],
    height: [Option<Number>; 3],
    area: Option<Number>,
}

impl Known {
    fn new() -> Self {
        Known {
            side: [None, None, None],
            angle: [None, None, None],
            height: [None, None, None],
            area: None,
        }
    }

    fn side_count(&self) -> usize {
        self.side.iter().filter(|s| s.is_some()).count()
    }

    fn angle_count(&self) -> usize {
        self.angle.iter().filter(|s| s.is_some()).count()
    }
}

/* ---------------- 数值工具 ---------------- */

fn num0() -> Number {
    Number::from_int(0)
}

fn num1() -> Number {
    Number::from_int(1)
}

fn num2() -> Number {
    Number::from_int(2)
}

/// `π`（精确符号）
fn num_pi() -> Number {
    Number::from_pi_times(BigRational::new(BigInt::from(1), BigInt::from(1)))
}

fn is_lt(a: &Number, b: &Number) -> bool {
    a.sub(b).is_negative()
}

fn is_gt(a: &Number, b: &Number) -> bool {
    b.sub(a).is_negative()
}

/// 数值相等（`Number` 没有 `PartialEq`，用"差为零"判定）
fn number_eq(a: &Number, b: &Number) -> bool {
    a.sub(b).is_zero()
}

/// 数值近似相等：相对误差需小于 `10^-(display_digits+2)`（默认 1e-22）。
/// 既容忍解析解里的近似传播，又能拦住真正的"不一致"。
fn approx_eq(a: &Number, b: &Number) -> bool {
    let d = a.sub(b);
    if d.is_zero() {
        return true;
    }
    let dl = d.abs().to_approx().magnitude_log10();
    let al = a.abs().to_approx().magnitude_log10();
    let bl = b.abs().to_approx().magnitude_log10();
    let scale = al.max(bl).max(0.0);
    dl < scale - (bigfloat::display_digits() as f64 + 2.0)
}

/// 把值夹到 `[-1, 1]`：供 `arccos` 使用，避免近似运算后略微越界
fn clamp_unit(x: &Number) -> Number {
    if is_gt(x, &num1()) {
        num1()
    } else if is_lt(x, &num1().neg()) {
        num1().neg()
    } else {
        x.clone()
    }
}

/* ---------------- 三角函数（弧度） ---------------- */

fn sin_rad(angle: &Number) -> Number {
    trig::sin_number(angle, AngleMode::Radian)
}

fn cos_rad(angle: &Number) -> Number {
    trig::cos_number(angle, AngleMode::Radian)
}

/// 反正弦（弧度）：精确优先，回退高精度数值
fn arcsin_rad(x: &Number) -> Number {
    if let Some(e) = trig::try_exact_arcsin(x) {
        return e;
    }
    Number::Approx(x.to_approx().asin(bigfloat::precision()))
}

/// 反余弦（弧度）：精确优先，回退高精度数值
fn arccos_rad(x: &Number) -> Number {
    if let Some(e) = trig::try_exact_arccos(x) {
        return e;
    }
    Number::Approx(x.to_approx().acos(bigfloat::precision()))
}

/// 角度值（按 `mode` 解释）→ 弧度
fn angle_to_radians(v: &Number, mode: AngleMode) -> Number {
    match mode {
        AngleMode::Radian => v.clone(),
        AngleMode::Degree => {
            let pi_180 = BigRational::new(BigInt::from(1), BigInt::from(180));
            v.mul(&Number::from_pi_times(pi_180))
        }
    }
}

/// 弧度 → 显示单位（度模式下把精确 `Pi(coeff)` 精确转成 `Rational(coeff*180)`）
fn from_radians(rad: &Number, mode: AngleMode) -> Number {
    match mode {
        AngleMode::Radian => rad.clone(),
        AngleMode::Degree => parser::radians_to_degrees(rad.clone()),
    }
}

/* ---------------- 入口 ---------------- */

/// 求解三角形。错误信息为中文原文（在输出边界由 `i18n` 翻译）。
pub fn solve_triangle(
    input: &TriangleInput,
    ev: &Evaluator,
) -> Result<Vec<TriangleSolution>, String> {
    let mode = ev.angle_mode;
    let alpha = input.alphabet;

    // 1) 求值 + 规范化
    let mut known = Known::new();
    let mut givens: Vec<(TriPart, Number)> = Vec::new();
    for (part, expr) in &input.parts {
        let value = ev.evaluate_with_vars(expr, &[])?;
        if value.is_complex() {
            return Err("三角形求解不支持复数".to_string());
        }
        let norm = match part {
            TriPart::Angle(_) => angle_to_radians(&value, mode),
            _ => value.clone(),
        };
        // 值域
        match part {
            TriPart::Side(_) => {
                if norm.is_zero() || norm.is_negative() {
                    return Err(format!("边长必须为正数: {0}", part.name(&alpha)));
                }
            }
            TriPart::Height(_) => {
                if norm.is_zero() || norm.is_negative() {
                    return Err(format!("高必须为正数: {0}", part.name(&alpha)));
                }
            }
            TriPart::Angle(_) => {
                if !(is_gt(&norm, &num0()) && is_gt(&num_pi(), &norm)) {
                    return Err(format!(
                        "角度必须在 (0, 180°) 范围内: {0}",
                        part.name(&alpha)
                    ));
                }
            }
        }
        // 重复赋值：值相同则忽略，值不同则报错
        let slot = match part {
            TriPart::Side(i) => &mut known.side[*i as usize],
            TriPart::Angle(i) => &mut known.angle[*i as usize],
            TriPart::Height(i) => &mut known.height[*i as usize],
        };
        match slot {
            Some(prev) => {
                if !number_eq(prev, &norm) {
                    return Err(format!("三角形某个量被重复赋值: {0}", part.name(&alpha)));
                }
                continue;
            }
            None => *slot = Some(norm.clone()),
        }
        givens.push((*part, norm));
    }
    if givens.len() < 2 {
        return Err("三角形求解至少需要 2 个已知量".to_string());
    }

    // 2) 归约：两角 → 第三角；高 → 面积 / 邻边 / 邻边比
    complete_angles(&mut known);
    reduce_heights(&mut known);
    complete_angles(&mut known);

    // 一个长度都没有 ⇒ 形状或许已定，但大小不定
    if known.side_count() == 0 {
        return Err(underdetermined_message(&known, &alpha));
    }

    // 3) 解析解优先，数值兜底
    let vals_list = match solve_analytic(&known) {
        Some(r) => r?,
        None => solve_numeric(&known, &givens, &alpha)?,
    };

    // 4) 回代校验所有已知量，再把已知值原样写回
    let mut out = Vec::new();
    for mut vals in vals_list {
        verify_givens(&givens, &vals)?;
        write_back_givens(&mut vals, &givens);
        out.push(to_solution(&vals, &givens, &alpha)?);
    }
    Ok(out)
}

/* ---------------- 归约 ---------------- */

/// 两角已知 ⇒ 补齐第三角
fn complete_angles(k: &mut Known) {
    for i in 0..3 {
        if k.angle[i].is_some() {
            continue;
        }
        let j = (i + 1) % 3;
        let m = (i + 2) % 3;
        if let (Some(x), Some(y)) = (k.angle[j].clone(), k.angle[m].clone()) {
            k.angle[i] = Some(num_pi().sub(&x).sub(&y));
        }
    }
}

/// 把"高"能唯一确定的信息转成面积或边长：
/// - `S = hA·a/2`（高 × 其底边）
/// - `hA = b·sinC = c·sinB`（高 × 邻角 → 邻边）
/// - `a·hA = b·hB = 2S`（两条高 + 一条边 → 另一条边）
fn reduce_heights(k: &mut Known) {
    // 高 + 底边 → 面积
    if k.area.is_none() {
        for i in 0..3 {
            if let (Some(h), Some(s)) = (k.height[i].clone(), k.side[i].clone()) {
                k.area = Some(h.mul(&s).div(&num2()));
                break;
            }
        }
    }
    // 高 + 邻角 → 邻边：h_i = side[(i+1)]·sin(angle[(i+2)]) = side[(i+2)]·sin(angle[(i+1)])
    for i in 0..3 {
        let Some(h) = k.height[i].clone() else {
            continue;
        };
        let j = (i + 1) % 3;
        let m = (i + 2) % 3;
        if k.side[j].is_none()
            && let Some(ang) = k.angle[m].clone()
        {
            let s = sin_rad(&ang);
            if !s.is_zero() {
                k.side[j] = Some(h.div(&s));
            }
        }
        if k.side[m].is_none()
            && let Some(ang) = k.angle[j].clone()
        {
            let s = sin_rad(&ang);
            if !s.is_zero() {
                k.side[m] = Some(h.div(&s));
            }
        }
    }
    // 两条高 + 一条边 → 另一条边
    for i in 0..3 {
        for j in 0..3 {
            if i == j {
                continue;
            }
            let (Some(hi), Some(hj)) = (k.height[i].clone(), k.height[j].clone()) else {
                continue;
            };
            if k.side[j].is_none()
                && let Some(si) = k.side[i].clone()
            {
                k.side[j] = Some(si.mul(&hi).div(&hj));
            }
            if k.side[i].is_none()
                && let Some(sj) = k.side[j].clone()
            {
                k.side[i] = Some(sj.mul(&hj).div(&hi));
            }
        }
    }
}

/// "信息不足"的提示；形状已能确定时补一句，便于用户知道还差什么
fn underdetermined_message(k: &Known, alpha: &Alphabet) -> String {
    let all_angles = k.angle.iter().all(|a| a.is_some());
    let all_heights = k.height.iter().all(|h| h.is_some());
    if all_heights {
        let h: Vec<Number> = k.height.iter().map(|x| x.clone().unwrap()).collect();
        // a : b : c = 1/hA : 1/hB : 1/hC = hB·hC : hA·hC : hA·hB
        let vals = [h[1].mul(&h[2]), h[0].mul(&h[2]), h[0].mul(&h[1])];
        if let Some(ratio) = ratio_string(&vals) {
            // 比例标签用用户写下的字母：`x : y : z = 6 : 4 : 3`
            let shape = format!(
                "{0} : {1} : {2} = {3}",
                alpha.side_label(0),
                alpha.side_label(1),
                alpha.side_label(2),
                ratio
            );
            return format!(
                "已知信息不足，无法确定三角形（形状已确定 {0}，仅缺一个长度）",
                shape
            );
        }
        return "已知信息不足，无法确定三角形（形状已确定，仅缺一个长度）".to_string();
    }
    if all_angles {
        return "已知信息不足，无法确定三角形（形状已确定，仅缺一个长度）".to_string();
    }
    "已知信息不足，无法确定三角形".to_string()
}

/// 三个数的最简整数比（如 `6 : 4 : 3`）；有非有理项时返回 `None`
fn ratio_string(vals: &[Number; 3]) -> Option<String> {
    let mut rats = Vec::with_capacity(3);
    for v in vals {
        rats.push(v.as_rational()?);
    }
    // 通分到公共分母
    let mut lcm = BigInt::one();
    for r in &rats {
        lcm = lcm.lcm(r.denom());
    }
    let ints: Vec<BigInt> = rats
        .iter()
        .map(|r| r.numer() * (&lcm / r.denom()))
        .collect();
    let mut g = ints[0].clone();
    for i in &ints {
        g = g.gcd(i);
    }
    if g.is_zero() {
        return None;
    }
    let parts: Vec<String> = ints.iter().map(|i| (i / &g).to_string()).collect();
    Some(parts.join(" : "))
}

/* ---------------- 解析求解 ---------------- */

/// 能否用经典情形公式求解；返回 `None` 表示需要数值兜底
fn solve_analytic(k: &Known) -> Option<Result<Vec<TriVals>, String>> {
    let ns = k.side_count();
    let na = k.angle_count();
    let side_idx: Vec<usize> = (0..3).filter(|&i| k.side[i].is_some()).collect();
    let ang_idx: Vec<usize> = (0..3).filter(|&i| k.angle[i].is_some()).collect();

    if ns == 3 {
        return Some(solve_sss(
            k.side[0].clone().unwrap(),
            k.side[1].clone().unwrap(),
            k.side[2].clone().unwrap(),
        ));
    }
    if ns == 2 && na >= 1 {
        let i = side_idx[0];
        let j = side_idx[1];
        let third = 3 - i - j;
        // 已知角落在"第三条边所对的顶点"上 ⇒ 它是那两条边的夹角 ⇒ SAS
        if k.angle[third].is_some() {
            return Some(solve_sas(
                k.side[i].clone().unwrap(),
                k.side[j].clone().unwrap(),
                i,
                j,
                k.angle[third].clone().unwrap(),
            ));
        }
        // 否则已知角是其中一条已知边的对角 ⇒ SSA
        for &t in &[i, j] {
            if k.angle[t].is_some() {
                let other = if t == i { j } else { i };
                return Some(solve_ssa(
                    k.side[t].clone().unwrap(),
                    k.side[other].clone().unwrap(),
                    t,
                    other,
                    k.angle[t].clone().unwrap(),
                ));
            }
        }
        return None;
    }
    if ns == 1 && na >= 2 {
        return Some(solve_aas(k));
    }
    if ns == 2
        && let Some(s) = k.area.clone()
    {
        let i = side_idx[0];
        let j = side_idx[1];
        return Some(solve_two_sides_area(
            k.side[i].clone().unwrap(),
            k.side[j].clone().unwrap(),
            i,
            j,
            s,
        ));
    }
    if ns == 1
        && na == 1
        && let Some(s) = k.area.clone()
    {
        let i = ang_idx[0];
        if side_idx[0] == i {
            return Some(solve_side_angle_area(
                k.side[i].clone().unwrap(),
                i,
                k.angle[i].clone().unwrap(),
                s,
            ));
        }
    }
    None
}

/// 由三边求角：`cos(对角) = (x² + y² − opp²) / (2xy)`
fn angle_from_cos(opp: &Number, x: &Number, y: &Number) -> Result<Number, String> {
    let den = num2().mul(x).mul(y);
    if den.is_zero() {
        return Err("三角形求解未收敛".to_string());
    }
    let cos = x.mul(x).add(&y.mul(y)).sub(&opp.mul(opp)).div(&den);
    Ok(arccos_rad(&clamp_unit(&cos)))
}

/// 三边 → 三角形（SSS / 其它情形的收尾）
fn from_sides(sides: [Number; 3], area: Number) -> Result<TriVals, String> {
    let [a, b, c] = [&sides[0], &sides[1], &sides[2]];
    if !is_gt(&a.add(b), c) || !is_gt(&a.add(c), b) || !is_gt(&b.add(c), a) {
        return Err("三角形不满足三角不等式".to_string());
    }
    // 三角各自用余弦定理（不走 π−A−B 累减，才能保住 90°/60° 之类的精确值）
    let ang_a = angle_from_cos(a, b, c)?;
    let ang_b = angle_from_cos(b, a, c)?;
    let ang_c = angle_from_cos(c, a, b)?;
    Ok(TriVals {
        sides,
        angles: [ang_a, ang_b, ang_c],
        area,
    })
}

/// SSS：海伦公式求面积（`sqrt` 对完全平方给精确值），再逐角用余弦定理
fn solve_sss(a: Number, b: Number, c: Number) -> Result<Vec<TriVals>, String> {
    // 必须先验三角不等式：否则海伦公式的乘积为负，`sqrt` 会直接 panic
    if !is_gt(&a.add(&b), &c) || !is_gt(&a.add(&c), &b) || !is_gt(&b.add(&c), &a) {
        return Err("三角形不满足三角不等式".to_string());
    }
    let s = a.add(&b).add(&c).div(&num2());
    let prod = s.mul(&s.sub(&a)).mul(&s.sub(&b)).mul(&s.sub(&c));
    let area = prod.sqrt();
    Ok(vec![from_sides([a, b, c], area)?])
}

/// SAS：已知两条边 `i,j` 与其夹角（夹角的顶点是第三条边 `third` 所对的顶点）
fn solve_sas(
    si: Number,
    sj: Number,
    i: usize,
    j: usize,
    angle_third: Number,
) -> Result<Vec<TriVals>, String> {
    let cos_t = cos_rad(&angle_third);
    // third 边：x² = si² + sj² − 2·si·sj·cos(夹角)
    let sq = si
        .mul(&si)
        .add(&sj.mul(&sj))
        .sub(&num2().mul(&si).mul(&sj).mul(&cos_t));
    if is_lt(&sq, &num0()) {
        return Err("已知量之间存在矛盾，无法构成三角形".to_string());
    }
    let third_side = sq.sqrt();
    let area = si.mul(&sj).mul(&sin_rad(&angle_third)).div(&num2());
    let mut sides: [Number; 3] = [num0(), num0(), num0()];
    sides[i] = si;
    sides[j] = sj;
    sides[3 - i - j] = third_side;
    Ok(vec![from_sides(sides, area)?])
}

/// SSA：已知边 `t` 与其对角 `angle_t`，以及另一条已知边 `other`
fn solve_ssa(
    side_t: Number,
    side_other: Number,
    t: usize,
    other: usize,
    angle_t: Number,
) -> Result<Vec<TriVals>, String> {
    let sin_t = sin_rad(&angle_t);
    if sin_t.is_zero() {
        return Err("三角形不满足三角不等式".to_string());
    }
    // 2R = a / sinA
    let two_r = side_t.div(&sin_t);
    let sin_other = side_other.div(&two_r);
    if is_gt(&sin_other, &num1()) {
        return Err("该三角形无解".to_string());
    }
    let base = arcsin_rad(&clamp_unit(&sin_other));
    let candidates: Vec<Number> = if number_eq(&sin_other, &num1()) {
        vec![base]
    } else {
        vec![base.clone(), num_pi().sub(&base)]
    };
    let m = 3 - t - other;
    let mut out = Vec::new();
    for ang_other in candidates {
        let ang_m = num_pi().sub(&angle_t).sub(&ang_other);
        if !is_gt(&ang_m, &num0()) {
            continue;
        }
        let side_m = two_r.mul(&sin_rad(&ang_m));
        let mut sides: [Number; 3] = [num0(), num0(), num0()];
        sides[t] = side_t.clone();
        sides[other] = side_other.clone();
        sides[m] = side_m;
        let area = sides[0]
            .mul(&sides[1])
            .mul(&sides[2])
            .div(&two_r.mul(&num2()));
        match from_sides(sides, area) {
            Ok(v) => out.push(v),
            Err(e) => {
                if out.is_empty() {
                    return Err(e);
                }
            }
        }
    }
    if out.is_empty() {
        return Err("该三角形无解".to_string());
    }
    Ok(out)
}

/// AAS / ASA：两角已知（第三角可补），用正弦定理定比求其余两边
fn solve_aas(k: &Known) -> Result<Vec<TriVals>, String> {
    let mut known = k.clone();
    complete_angles(&mut known);
    let t = (0..3).find(|&i| known.side[i].is_some()).unwrap();
    let ang_t = known.angle[t].clone().unwrap();
    let sin_t = sin_rad(&ang_t);
    if sin_t.is_zero() {
        return Err("三角形不满足三角不等式".to_string());
    }
    let two_r = known.side[t].clone().unwrap().div(&sin_t);
    let mut sides: [Number; 3] = [num0(), num0(), num0()];
    for i in 0..3 {
        sides[i] = if i == t {
            known.side[t].clone().unwrap()
        } else {
            // a = 2R·sinA
            two_r.mul(&sin_rad(&known.angle[i].clone().unwrap()))
        };
    }
    let area = sides[0]
        .mul(&sides[1])
        .mul(&sides[2])
        .div(&two_r.mul(&num2()));
    Ok(vec![from_sides(sides, area)?])
}

/// 两边 + 面积：`sin(夹角) = 2S/(si·sj)`，一般有两解（锐角/钝角）
fn solve_two_sides_area(
    si: Number,
    sj: Number,
    i: usize,
    j: usize,
    area: Number,
) -> Result<Vec<TriVals>, String> {
    let den = si.mul(&sj);
    if den.is_zero() {
        return Err("三角形不满足三角不等式".to_string());
    }
    let sin_t = area.mul(&num2()).div(&den);
    if is_gt(&sin_t, &num1()) {
        return Err("已知量之间存在矛盾，无法构成三角形".to_string());
    }
    let base = arcsin_rad(&clamp_unit(&sin_t));
    let candidates: Vec<Number> = if number_eq(&sin_t, &num1()) {
        vec![base]
    } else {
        vec![base.clone(), num_pi().sub(&base)]
    };
    let mut out = Vec::new();
    for ang in candidates {
        if let Ok(mut v) = solve_sas(si.clone(), sj.clone(), i, j, ang) {
            v[0].area = area.clone();
            out.push(v.remove(0));
        }
    }
    if out.is_empty() {
        return Err("该三角形无解".to_string());
    }
    Ok(out)
}

/// 一边 + 其对角 + 面积：由 `2R = a/sinA`、`S = abc/(2·2R)`、余弦定理联立，
/// 得到 `b·c` 与 `b²+c²`，再由 `(b±c)²` 求根 ⇒ 一般两解（b、c 互换）
fn solve_side_angle_area(
    side_i: Number,
    i: usize,
    angle_i: Number,
    area: Number,
) -> Result<Vec<TriVals>, String> {
    let sin_i = sin_rad(&angle_i);
    if sin_i.is_zero() {
        return Err("三角形不满足三角不等式".to_string());
    }
    let two_r = side_i.div(&sin_i);
    // P = b·c = 2·2R·S / a
    let p = two_r.mul(&area).mul(&num2()).div(&side_i);
    // Q = b² + c² = a² + 2·b·c·cosA
    let q = side_i
        .mul(&side_i)
        .add(&p.mul(&num2()).mul(&cos_rad(&angle_i)));
    let plus_sq = q.add(&p.mul(&num2()));
    let minus_sq = q.sub(&p.mul(&num2()));
    if is_lt(&minus_sq, &num0()) || is_lt(&plus_sq, &num0()) {
        return Err("已知量之间存在矛盾，无法构成三角形".to_string());
    }
    let sum = plus_sq.sqrt();
    let diff = minus_sq.sqrt();
    let two = num2();
    let pairs = [
        (sum.add(&diff).div(&two), sum.sub(&diff).div(&two)),
        (sum.sub(&diff).div(&two), sum.add(&diff).div(&two)),
    ];
    let j = (i + 1) % 3;
    let m = (i + 2) % 3;
    let mut out = Vec::new();
    for (sj, sm) in pairs {
        let mut sides: [Number; 3] = [num0(), num0(), num0()];
        sides[i] = side_i.clone();
        sides[j] = sj;
        sides[m] = sm;
        let area_calc = sides[0].mul(&sides[1]).mul(&sides[2]).div(&two_r.mul(&two));
        if let Ok(v) = from_sides(sides, area_calc) {
            out.push(v);
        }
    }
    if out.is_empty() {
        return Err("已知量之间存在矛盾，无法构成三角形".to_string());
    }
    Ok(out)
}

/* ---------------- 数值兜底 ---------------- */

/// 未知量 `(A, B, s)`，其中 `s = 2R`；`C = π − A − B`，`a = s·sinA` 等
type Xyz = [BigFloat; 3];

const NUM_MAX_ITER: usize = 60;

fn bf_zero() -> BigFloat {
    BigFloat::from_u64(0)
}

fn bf_abs(x: &BigFloat) -> BigFloat {
    if x < &bf_zero() { x.neg() } else { x.clone() }
}

/// `1/k` 的 BigFloat
fn bf_frac(n: i64, d: i64) -> BigFloat {
    BigFloat::from_big_rational(&BigRational::new(BigInt::from(n), BigInt::from(d)))
}

fn bf_sin(x: &BigFloat) -> BigFloat {
    x.sin(bigfloat::precision())
}

/// 残差函数：把 `(A,B,s)` 代入后，逐条给出"计算值 − 已知值"（已按已知值的量级归一化）
fn residuals(givens: &[(TriPart, BigFloat)], x: &Xyz) -> Vec<BigFloat> {
    let prec = bigfloat::precision();
    let pi = BigFloat::pi(prec);
    let ang_a = x[0].clone();
    let ang_b = x[1].clone();
    let ang_c = BigFloat::sub(&pi, &BigFloat::add(&ang_a, &ang_b, prec), prec);
    let sa = bf_sin(&ang_a);
    let sb = bf_sin(&ang_b);
    let sc = bf_sin(&ang_c);
    let s = x[2].clone();
    let side = |sin: &BigFloat| BigFloat::mul(&s, sin, prec);
    let norm =
        |calc: BigFloat, v: &BigFloat| BigFloat::div(&BigFloat::sub(&calc, v, prec), v, prec);
    let norm_pi =
        |calc: BigFloat, v: &BigFloat| BigFloat::div(&BigFloat::sub(&calc, v, prec), &pi, prec);
    let mut out = Vec::with_capacity(givens.len());
    for (part, v) in givens {
        let r = match part {
            TriPart::Side(0) => norm(side(&sa), v),
            TriPart::Side(1) => norm(side(&sb), v),
            TriPart::Side(_) => norm(side(&sc), v),
            TriPart::Angle(0) => norm_pi(ang_a.clone(), v),
            TriPart::Angle(1) => norm_pi(ang_b.clone(), v),
            TriPart::Angle(_) => norm_pi(ang_c.clone(), v),
            TriPart::Height(0) => norm(BigFloat::mul(&side(&sb), &sc, prec), v),
            TriPart::Height(1) => norm(BigFloat::mul(&side(&sa), &sc, prec), v),
            TriPart::Height(_) => norm(BigFloat::mul(&side(&sa), &sb, prec), v),
        };
        out.push(r);
    }
    out
}

/// 3×3 线性方程组（列主元高斯消元）；奇异返回 `None`
fn solve3(mut a: [[BigFloat; 3]; 3], mut b: [BigFloat; 3]) -> Option<[BigFloat; 3]> {
    let prec = bigfloat::precision();
    for col in 0..3 {
        let mut best = bf_abs(&a[col][col]);
        let mut piv = col;
        for r in (col + 1)..3 {
            let v = bf_abs(&a[r][col]);
            if best < v {
                best = v;
                piv = r;
            }
        }
        if best.is_zero() {
            return None;
        }
        if piv != col {
            a.swap(piv, col);
            b.swap(piv, col);
        }
        let d = a[col][col].clone();
        for j in col..3 {
            a[col][j] = BigFloat::div(&a[col][j], &d, prec);
        }
        b[col] = BigFloat::div(&b[col], &d, prec);
        for r in 0..3 {
            if r == col {
                continue;
            }
            let f = a[r][col].clone();
            if f.is_zero() {
                continue;
            }
            for j in col..3 {
                let t = BigFloat::mul(&f, &a[col][j], prec);
                a[r][j] = BigFloat::sub(&a[r][j], &t, prec);
            }
            let t = BigFloat::mul(&f, &b[col], prec);
            b[r] = BigFloat::sub(&b[r], &t, prec);
        }
    }
    Some(b)
}

/// 数值兜底：多起点 Gauss-Newton（法方程 `JᵀJ Δ = −Jᵀ r`）
fn solve_numeric(
    k: &Known,
    givens: &[(TriPart, Number)],
    alpha: &Alphabet,
) -> Result<Vec<TriVals>, String> {
    let prec = bigfloat::precision();
    let pi = BigFloat::pi(prec);
    let bfs: Vec<(TriPart, BigFloat)> = givens.iter().map(|(p, v)| (*p, v.to_approx())).collect();
    if bfs.len() < 3 {
        return Err(underdetermined_message(k, alpha));
    }
    // 步长：角度用绝对步长，尺度用相对步长
    let h_step =
        BigFloat::from_big_rational(&BigRational::new(BigInt::from(1), BigInt::from(10).pow(40)));
    let tol =
        BigFloat::from_big_rational(&BigRational::new(BigInt::from(1), BigInt::from(10).pow(35)));

    // 种子：A、B 取 π/8 的整数倍（剔除 A+B ≥ π）
    let mut seeds: Vec<(BigFloat, BigFloat)> = Vec::new();
    for ka in 1..=7i64 {
        for kb in 1..=7i64 {
            if ka + kb >= 8 {
                continue;
            }
            let a = BigFloat::mul(&pi, &bf_frac(ka, 8), prec);
            let b = BigFloat::mul(&pi, &bf_frac(kb, 8), prec);
            seeds.push((a, b));
        }
    }

    let mut solutions: Vec<Xyz> = Vec::new();
    let mut rank_deficient = true;
    for (sa, sb) in seeds {
        let Some(s) = seed_scale(&bfs, &sa, &sb, prec) else {
            continue;
        };
        let mut x: Xyz = [sa, sb, s];
        let mut converged = false;
        for _ in 0..NUM_MAX_ITER {
            let r = residuals(&bfs, &x);
            let max_r = r.iter().fold(bf_zero(), |acc, v| {
                let a = bf_abs(v);
                if acc < a { a } else { acc }
            });
            if max_r < tol {
                converged = true;
                break;
            }
            // 前向差分雅可比
            let mut jac: [[BigFloat; 3]; 3] = [
                [bf_zero(), bf_zero(), bf_zero()],
                [bf_zero(), bf_zero(), bf_zero()],
                [bf_zero(), bf_zero(), bf_zero()],
            ];
            for col in 0..3 {
                let h = if col == 2 {
                    BigFloat::mul(&x[2].clone(), &h_step, prec)
                } else {
                    h_step.clone()
                };
                if h.is_zero() {
                    continue;
                }
                let mut xp = x.clone();
                xp[col] = BigFloat::add(&xp[col], &h, prec);
                let rp = residuals(&bfs, &xp);
                for row in 0..r.len().min(3) {
                    jac[row][col] =
                        BigFloat::div(&BigFloat::sub(&rp[row], &r[row], prec), &h, prec);
                }
            }
            // 法方程 JᵀJ Δ = −Jᵀ r
            let mut ata = [
                [bf_zero(), bf_zero(), bf_zero()],
                [bf_zero(), bf_zero(), bf_zero()],
                [bf_zero(), bf_zero(), bf_zero()],
            ];
            let mut atb = [bf_zero(), bf_zero(), bf_zero()];
            for i in 0..3 {
                for j in 0..3 {
                    let mut sum = bf_zero();
                    for row in 0..r.len() {
                        let t = BigFloat::mul(&jac[row][i], &jac[row][j], prec);
                        sum = BigFloat::add(&sum, &t, prec);
                    }
                    ata[i][j] = sum;
                }
                let mut sum = bf_zero();
                for row in 0..r.len() {
                    let t = BigFloat::mul(&jac[row][i], &r[row], prec);
                    sum = BigFloat::add(&sum, &t, prec);
                }
                atb[i] = sum.neg();
            }
            let Some(delta) = solve3(ata, atb) else {
                break;
            };
            rank_deficient = false;
            let mut moved = bf_zero();
            for i in 0..3 {
                x[i] = BigFloat::add(&x[i], &delta[i], prec);
                let d = bf_abs(&delta[i]);
                if moved < d {
                    moved = d;
                }
            }
            if moved
                < BigFloat::from_big_rational(&BigRational::new(
                    BigInt::from(1),
                    BigInt::from(10).pow(45),
                ))
            {
                let r = residuals(&bfs, &x);
                converged = r.iter().all(|v| bf_abs(v) < tol);
                break;
            }
        }
        if !converged {
            continue;
        }
        if !valid_xyz(&x, &pi) {
            continue;
        }
        // 去重
        let dup = solutions
            .iter()
            .any(|y| (0..3).all(|i| small_diff(&x[i], &y[i])));
        if !dup {
            solutions.push(x);
        }
    }

    if solutions.is_empty() {
        if rank_deficient {
            return Err(underdetermined_message(k, alpha));
        }
        return Err("三角形求解未收敛".to_string());
    }
    // 按 (A, B) 排序，保证输出稳定
    solutions.sort_by(|p, q| match p[0].partial_cmp(&q[0]) {
        Some(std::cmp::Ordering::Equal) | None => match p[1].partial_cmp(&q[1]) {
            Some(o) => o,
            None => std::cmp::Ordering::Equal,
        },
        Some(o) => o,
    });

    let mut out = Vec::new();
    for x in &solutions {
        let ang_c = BigFloat::sub(&pi, &BigFloat::add(&x[0], &x[1], prec), prec);
        let s = x[2].clone();
        let angs = [x[0].clone(), x[1].clone(), ang_c];
        let mut sides_v: [Number; 3] = [num0(), num0(), num0()];
        for i in 0..3 {
            sides_v[i] = Number::Approx(BigFloat::mul(&s, &bf_sin(&angs[i]), prec));
        }
        // S = ½·a·b·sinC；若"高×底边"已给出精确面积则优先采用
        let area = match &k.area {
            Some(a) => a.clone(),
            None => sides_v[0]
                .mul(&sides_v[1])
                .mul(&Number::Approx(bf_sin(&angs[2])))
                .div(&num2()),
        };
        out.push(TriVals {
            sides: sides_v,
            angles: [
                Number::Approx(angs[0].clone()),
                Number::Approx(angs[1].clone()),
                Number::Approx(angs[2].clone()),
            ],
            area,
        });
    }
    Ok(out)
}

/// 由任一已知边 / 高确定尺度 `s = 2R`
fn seed_scale(
    givens: &[(TriPart, BigFloat)],
    ang_a: &BigFloat,
    ang_b: &BigFloat,
    prec: usize,
) -> Option<BigFloat> {
    let pi = BigFloat::pi(prec);
    let ang_c = BigFloat::sub(&pi, &BigFloat::add(ang_a, ang_b, prec), prec);
    let sines = [bf_sin(ang_a), bf_sin(ang_b), bf_sin(&ang_c)];
    for (part, v) in givens {
        if let TriPart::Side(i) = part {
            let s = &sines[*i as usize];
            if !s.is_zero() {
                return Some(BigFloat::div(v, s, prec));
            }
        }
    }
    for (part, v) in givens {
        if let TriPart::Height(i) = part {
            // hA = s·sinB·sinC 等（轮换）
            let (x, y) = match i {
                0 => (1usize, 2usize),
                1 => (0, 2),
                _ => (0, 1),
            };
            let d = BigFloat::mul(&sines[x], &sines[y], prec);
            if !d.is_zero() {
                return Some(BigFloat::div(v, &d, prec));
            }
        }
    }
    None
}

/// 相对差是否可忽略（去重用）
fn small_diff(a: &BigFloat, b: &BigFloat) -> bool {
    let prec = bigfloat::precision();
    let d = bf_abs(&BigFloat::sub(a, b, prec)).magnitude_log10();
    let scale = bf_abs(a).magnitude_log10().max(0.0);
    d < scale - 30.0
}

/// 解是否落在合法域：`A,B,C ∈ (0,π)`、`s > 0`
fn valid_xyz(x: &Xyz, pi: &BigFloat) -> bool {
    let prec = bigfloat::precision();
    let zero = bf_zero();
    let c = BigFloat::sub(pi, &BigFloat::add(&x[0], &x[1], prec), prec);
    x[0] > zero && x[1] > zero && c > zero && x[0] < *pi && x[1] < *pi && x[2] > zero
}

/* ---------------- 校验与收尾 ---------------- */

/// 逐项回代校验已知量（超定矛盾在这里被拦住）
fn verify_givens(givens: &[(TriPart, Number)], vals: &TriVals) -> Result<(), String> {
    let area = vals.area.clone();
    for (part, v) in givens {
        let calc = match part {
            TriPart::Side(i) => vals.sides[*i as usize].clone(),
            TriPart::Angle(i) => vals.angles[*i as usize].clone(),
            TriPart::Height(i) => num2().mul(&area).div(&vals.sides[*i as usize]),
        };
        if !approx_eq(&calc, v) {
            return Err("已知量之间存在矛盾，无法构成三角形".to_string());
        }
    }
    Ok(())
}

/// 把已知量原样写回（`A=30` 就该显示 `30`，而不是解出来的 `29.9999…`）
fn write_back_givens(vals: &mut TriVals, givens: &[(TriPart, Number)]) {
    for (part, v) in givens {
        match part {
            TriPart::Side(i) => vals.sides[*i as usize] = v.clone(),
            TriPart::Angle(i) => vals.angles[*i as usize] = v.clone(),
            TriPart::Height(_) => {}
        }
    }
}

/// 由三边/三角/面积派生出全部输出量（全走 `Number` 精确运算）；
/// 已知量原样写回，保证 `a=3` 显示 `3` 而不是解出来的近似值
fn to_solution(
    vals: &TriVals,
    givens: &[(TriPart, Number)],
    alpha: &Alphabet,
) -> Result<TriangleSolution, String> {
    let mut sides = vals.sides.clone();
    let mut angles = vals.angles.clone();
    let mut height_override: [Option<Number>; 3] = [None, None, None];
    for (part, v) in givens {
        match part {
            TriPart::Side(i) => sides[*i as usize] = v.clone(),
            TriPart::Angle(i) => angles[*i as usize] = v.clone(),
            TriPart::Height(i) => height_override[*i as usize] = Some(v.clone()),
        }
    }
    let [a, b, c] = sides.clone();
    if a.is_zero() || b.is_zero() || c.is_zero() {
        return Err("三角形求解未收敛".to_string());
    }
    let perimeter = a.add(&b).add(&c);
    // R = abc / 4S（不经过三角函数，可保持精确）
    let four_area = vals.area.mul(&Number::from_int(4));
    if four_area.is_zero() {
        return Err("三角形求解未收敛".to_string());
    }
    let circumradius = a.mul(&b).mul(&c).div(&four_area);
    // r = S / (p/2)
    let semi = perimeter.div(&num2());
    if semi.is_zero() {
        return Err("三角形求解未收敛".to_string());
    }
    let inradius = vals.area.div(&semi);
    let height = |i: usize, side: &Number| match &height_override[i] {
        Some(h) => h.clone(),
        None => num2().mul(&vals.area).div(side),
    };
    Ok(TriangleSolution {
        alphabet: *alpha,
        a: a.clone(),
        b: b.clone(),
        c: c.clone(),
        angle_a: angles[0].clone(),
        angle_b: angles[1].clone(),
        angle_c: angles[2].clone(),
        ha: height(0, &a),
        hb: height(1, &b),
        hc: height(2, &c),
        area: vals.area.clone(),
        perimeter,
        circumradius,
        inradius,
    })
}

/* ---------------- 输出 ---------------- */

fn display_part(n: &Number, mode: DisplayMode) -> String {
    match mode {
        DisplayMode::MathIO => display::format_mathio(n),
        DisplayMode::LineIO => display::format_lineio(n),
    }
}

/// `标签 = 值` / `标签 ≈ 值`（`=`/`≈` 由 `result_prefix` 决定）
fn entry(label: &str, v: &Number, mode: DisplayMode) -> String {
    let prefix = solve_aux::result_prefix(v, mode);
    format!("{0} {1} {2}", label, prefix, display_part(v, mode))
}

fn join_line(items: &[String]) -> String {
    items.join(", ")
}

/// 渲染为一个"解"的多行文本（5 行）
pub fn format_triangle_solution(
    sol: &TriangleSolution,
    mode: DisplayMode,
    angle_mode: AngleMode,
) -> Vec<String> {
    let alpha = &sol.alphabet;
    let da = from_radians(&sol.angle_a, angle_mode);
    let db = from_radians(&sol.angle_b, angle_mode);
    let dc = from_radians(&sol.angle_c, angle_mode);
    vec![
        join_line(&[
            entry(&alpha.side_label(0), &sol.a, mode),
            entry(&alpha.side_label(1), &sol.b, mode),
            entry(&alpha.side_label(2), &sol.c, mode),
        ]),
        join_line(&[
            entry(&alpha.angle_label(0), &da, mode),
            entry(&alpha.angle_label(1), &db, mode),
            entry(&alpha.angle_label(2), &dc, mode),
        ]),
        join_line(&[
            entry(&alpha.height_label(0), &sol.ha, mode),
            entry(&alpha.height_label(1), &sol.hb, mode),
            entry(&alpha.height_label(2), &sol.hc, mode),
        ]),
        join_line(&[
            entry("面积", &sol.area, mode),
            entry("周长", &sol.perimeter, mode),
        ]),
        join_line(&[
            entry("外接圆半径", &sol.circumradius, mode),
            entry("内切圆半径", &sol.inradius, mode),
        ]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{EvalResult, parse_and_eval};

    /// 测试辅助：把裸的"已知量"包成新的 `triangle(...)` 调用形式。
    /// 这样各个用例仍可只写 `a=3 b=4 c=5`，但实际走的是**新语法**（旧裸写法已移除）。
    fn tri(input: &str) -> String {
        if input.trim_start().starts_with("triangle(") {
            input.to_string()
        } else {
            format!("triangle({})", input)
        }
    }

    /// 跑一行三角形输入，按 `handle_triangle` 的规则拼装输出行
    fn run_with(
        input: &str,
        mode: DisplayMode,
        angle_mode: AngleMode,
    ) -> Result<Vec<String>, String> {
        let mut ev = Evaluator::new();
        ev.display_mode = mode;
        ev.angle_mode = angle_mode;
        match parse_and_eval(&tri(input), &mut ev)? {
            EvalResult::Triangle(t) => {
                let sols = solve_triangle(&t, &ev)?;
                let mut lines = Vec::new();
                for (i, sol) in sols.iter().enumerate() {
                    if sols.len() > 1 {
                        lines.push(format!("解 {}:", i + 1));
                    }
                    lines.extend(format_triangle_solution(sol, mode, angle_mode));
                }
                Ok(lines)
            }
            _ => Err("输入未被识别为三角形".to_string()),
        }
    }

    fn deg(input: &str) -> Vec<String> {
        run_with(input, DisplayMode::MathIO, AngleMode::Degree).unwrap()
    }

    fn deg_lineio(input: &str) -> Vec<String> {
        run_with(input, DisplayMode::LineIO, AngleMode::Degree).unwrap()
    }

    fn rad(input: &str) -> Vec<String> {
        run_with(input, DisplayMode::MathIO, AngleMode::Radian).unwrap()
    }

    fn err(input: &str) -> String {
        run_with(input, DisplayMode::MathIO, AngleMode::Degree).unwrap_err()
    }

    fn line_starting(lines: &[String], prefix: &str) -> String {
        lines
            .iter()
            .find(|l| l.starts_with(prefix))
            .cloned()
            .unwrap_or_else(|| panic!("找不到以 {prefix} 开头的行: {lines:?}"))
    }

    #[test]
    fn sss_right_triangle_is_exact() {
        let lines = deg("a=3 b=4 c=5");
        assert_eq!(line_starting(&lines, "a "), "a = 3, b = 4, c = 5");
        // 直角与面积、周长、两半径都精确
        assert!(line_starting(&lines, "A ").contains("C = 90"), "{lines:?}");
        assert_eq!(line_starting(&lines, "hA "), "hA = 4, hB = 3, hC = 12 / 5");
        assert_eq!(line_starting(&lines, "面积 "), "面积 = 6, 周长 = 12");
        assert_eq!(
            line_starting(&lines, "外接圆半径 "),
            "外接圆半径 = 5 / 2, 内切圆半径 = 1"
        );
        // A、B 不是特殊角 ⇒ 近似
        assert!(
            line_starting(&lines, "A ").contains("A ≈ 36.86989764584402"),
            "{lines:?}"
        );
    }

    #[test]
    fn sss_lineio_shows_finite_decimals() {
        let lines = deg_lineio("a=3 b=4 c=5");
        assert!(
            line_starting(&lines, "hA ").contains("hC = 2.4"),
            "{lines:?}"
        );
        assert!(
            line_starting(&lines, "外接圆半径 ").contains("2.5"),
            "{lines:?}"
        );
    }

    #[test]
    fn sas_right_isosceles_is_exact() {
        let lines = deg("a=1 b=1 C=90");
        assert_eq!(line_starting(&lines, "a "), "a = 1, b = 1, c = sqrt(2)");
        assert!(line_starting(&lines, "A ").contains("A = 45"), "{lines:?}");
        assert!(line_starting(&lines, "A ").contains("B = 45"), "{lines:?}");
        assert!(
            line_starting(&lines, "面积 ").contains("面积 = 1 / 2"),
            "{lines:?}"
        );
    }

    #[test]
    fn aas_two_angles_and_side_are_exact() {
        let lines = deg("A=30 B=60 a=5");
        assert_eq!(line_starting(&lines, "a "), "a = 5, b = 5*sqrt(3), c = 10");
        assert!(line_starting(&lines, "A ").contains("C = 90"), "{lines:?}");
    }

    #[test]
    fn asa_with_height_reduces_to_classic_case() {
        // hA 加上邻角能定出两条边，最终等价于 SAS
        let lines = deg("A=30 B=60 hA=3");
        assert!(line_starting(&lines, "a ").contains("b = 3"), "{lines:?}");
        assert!(line_starting(&lines, "A ").contains("C = 90"), "{lines:?}");
    }

    #[test]
    fn height_with_base_and_side_gives_exact_right_triangle() {
        let lines = deg("hA=4 a=3 b=4");
        assert_eq!(line_starting(&lines, "a "), "a = 3, b = 4, c = 5");
        assert!(line_starting(&lines, "A ").contains("C = 90"), "{lines:?}");
    }

    #[test]
    fn ssa_can_have_two_solutions() {
        // b·sinA = 5·sin30° = 2.5 < a = 4 < b = 5 ⇒ 两解
        let lines = deg("a=4 b=5 A=30");
        assert!(lines.iter().any(|l| l == "解 1:"), "{lines:?}");
        assert!(lines.iter().any(|l| l == "解 2:"), "{lines:?}");
        assert_eq!(lines.iter().filter(|l| l.starts_with("a ")).count(), 2);
    }

    #[test]
    fn ssa_with_no_solution_reports_error() {
        // a = 1 < b·sinA = 2.5 ⇒ 无解
        assert!(err("a=1 b=5 A=30").contains("无解"));
    }

    #[test]
    fn area_from_two_heights_ratio_is_reported_when_scale_missing() {
        let e = err("hA=2 hB=3 hC=4");
        assert!(e.contains("形状已确定"), "{e}");
        assert!(e.contains("a : b : c = 6 : 4 : 3"), "{e}");
    }

    #[test]
    fn three_angles_without_length_is_underdetermined() {
        let e = err("A=30 B=60 C=90");
        assert!(e.contains("形状已确定"), "{e}");
    }

    #[test]
    fn contradictory_and_illegal_inputs_are_rejected() {
        assert!(err("a=1 b=1 c=3").contains("三角不等式"));
        assert!(err("a=3 b=4 c=5 A=30").contains("矛盾"));
        assert!(err("a=-3 b=4 c=5").contains("边长必须为正数"));
        assert!(err("A=200 b=1 c=2").contains("角度必须在"));
        assert!(err("a=3 b=4").contains("已知信息不足"));
    }

    #[test]
    fn arbitrary_letters_behave_like_abc() {
        // 3-4-5：字母换成 x/y/z，数值与经典记号一字不差，只是标签跟着用户写
        let lines = deg("x=3 y=4 z=5");
        assert_eq!(line_starting(&lines, "x "), "x = 3, y = 4, z = 5");
        assert!(line_starting(&lines, "X ").contains("Z = 90"), "{lines:?}");
        assert_eq!(line_starting(&lines, "hX "), "hX = 4, hY = 3, hZ = 12 / 5");
        assert_eq!(line_starting(&lines, "面积 "), "面积 = 6, 周长 = 12");
        assert_eq!(
            line_starting(&lines, "外接圆半径 "),
            "外接圆半径 = 5 / 2, 内切圆半径 = 1"
        );
    }

    #[test]
    fn greek_letters_are_supported() {
        let lines = deg("α=3 β=4 γ=5");
        assert_eq!(line_starting(&lines, "α "), "α = 3, β = 4, γ = 5");
        assert!(line_starting(&lines, "Α ").contains("Γ = 90"), "{lines:?}");
        assert_eq!(line_starting(&lines, "hΑ "), "hΑ = 4, hΒ = 3, hΓ = 12 / 5");
    }

    #[test]
    fn sides_and_angles_can_use_any_letters() {
        // 大写 = 该顶点处的角：`M=30` 是 `m` 边的对角 ⇒ SSA 两解
        let lines = deg("m=4 n=5 M=30");
        assert!(lines.iter().any(|l| l == "解 1:"), "{lines:?}");
        assert!(lines.iter().any(|l| l == "解 2:"), "{lines:?}");
        assert!(line_starting(&lines, "M ").contains("M = 30"), "{lines:?}");
        assert!(
            line_starting(&lines, "m ").starts_with("m = 4, n = 5, o"),
            "{lines:?}"
        );
    }

    #[test]
    fn missing_vertex_letters_are_filled_in_order() {
        // 只写了顶点 x：补齐成 x,y,z（从最后一个字母往后顺延）
        let lines = deg("hX=3 x=3 X=30");
        assert!(
            line_starting(&lines, "x ").starts_with("x = 3, y ≈"),
            "{lines:?}"
        );
        assert!(line_starting(&lines, "X ").contains("X = 30"), "{lines:?}");
        // 补齐后的第三顶点拿得到名字，高也用它的字母
        assert!(
            line_starting(&lines, "hX ").starts_with("hX = 3"),
            "{lines:?}"
        );
    }

    #[test]
    fn alphabet_is_built_from_input_letters() {
        let al = Alphabet::from_letters(&['x', 'y', 'z']).unwrap();
        assert_eq!(al.side_label(0), "x");
        assert_eq!(al.angle_label(0), "X");
        assert_eq!(al.height_label(0), "hX");
        assert_eq!(al.side_label(2), "z");
        assert_eq!(al.index_of('y'), Some(1));
        assert_eq!(al.index_of('w'), None);
        // 补齐：从最后一个字母顺延，回绕到 a
        assert_eq!(
            Alphabet::from_letters(&['x', 'y']).unwrap().side_label(2),
            "z"
        );
        assert_eq!(
            Alphabet::from_letters(&['p', 'q']).unwrap().side_label(2),
            "r"
        );
        assert_eq!(
            Alphabet::from_letters(&['y', 'z']).unwrap().side_label(2),
            "a"
        );
        assert_eq!(Alphabet::from_letters(&['m']).unwrap().side_label(1), "n");
        // 同一个顶点的边与角只算一个顶点
        assert_eq!(
            Alphabet::from_letters(&['a', 'a', 'b'])
                .unwrap()
                .side_label(2),
            "c"
        );
        // 四个不同字母 ⇒ 报错
        assert!(Alphabet::from_letters(&['a', 'b', 'c', 'd']).is_err());
        // 希腊字母同样可用
        let g = Alphabet::from_letters(&['α', 'β']).unwrap();
        assert_eq!(g.side_label(0), "α");
        assert_eq!(g.angle_label(0), "Α");
        assert_eq!(g.height_label(0), "hΑ");
    }

    #[test]
    fn part_names_are_parsed_by_case() {
        assert_eq!(parse_part_name("a").unwrap(), (TriRole::Side, 'a'));
        assert_eq!(parse_part_name("A").unwrap(), (TriRole::Angle, 'a'));
        assert_eq!(parse_part_name("hA").unwrap(), (TriRole::Height, 'a'));
        assert_eq!(parse_part_name("Hy").unwrap(), (TriRole::Height, 'y'));
        assert_eq!(parse_part_name("Z").unwrap(), (TriRole::Angle, 'z'));
        assert_eq!(parse_part_name("α").unwrap(), (TriRole::Side, 'α'));
        for bad in ["sinA", "点", "", "ab", "a1"] {
            assert!(parse_part_name(bad).is_err(), "{bad} 应当报错");
        }
    }

    #[test]
    fn error_messages_use_the_users_letters() {
        let e = err("w=1 v=1 u=3");
        assert!(e.contains("三角不等式"), "{e}");
        let e = err("x=3 y=4 z=5 X=30");
        assert!(e.contains("矛盾"), "{e}");
        let e = err("x=-3 y=4 z=5");
        assert!(e.contains("边长必须为正数: x"), "{e}");
        let e = err("P=200 q=1 r=2");
        assert!(e.contains("角度必须在") && e.contains(": P"), "{e}");
        // 形状已定但缺长度：比例标签用用户的字母
        let e = err("hX=2 hY=3 hZ=4");
        assert!(e.contains("x : y : z = 6 : 4 : 3"), "{e}");
    }

    #[test]
    fn radian_mode_keeps_exact_pi_multiples() {
        let lines = rad("A=pi/6 b=5 C=pi/3");
        assert!(
            line_starting(&lines, "A ").contains("A = pi / 6"),
            "{lines:?}"
        );
        assert!(
            line_starting(&lines, "A ").contains("B = pi / 2"),
            "{lines:?}"
        );
        assert_eq!(
            line_starting(&lines, "a "),
            "a = 5 / 2, b = 5, c = 5*sqrt(3) / 2"
        );
    }

    #[test]
    fn numeric_fallback_handles_exotic_combination() {
        // 一边 + 两边上的高：无法归入经典情形，走数值兜底
        let lines = deg("a=5 A=30 hA=3");
        assert!(line_starting(&lines, "a ").contains("a = 5"), "{lines:?}");
        assert!(line_starting(&lines, "A ").contains("A = 30"), "{lines:?}");
        assert!(line_starting(&lines, "hA ").contains("hA = 3"), "{lines:?}");
    }

    #[test]
    fn existing_forms_are_left_alone() {
        // 这些输入不能被当作三角形：必须仍走原有语义
        let mut ev = Evaluator::new();
        ev.display_mode = DisplayMode::MathIO;
        for bad in [
            "a=3",
            "a=3, b=4, c=5",
            "x=1 y=2",
            "(1,2) (3,4)",
            // 旧版的裸写三角形形式已移除 ⇒ 现在必须报错，不能再被认领
            "a=3 b=4 c=5",
            "A=30 a=3 b=4",
        ] {
            let parsed = parse_and_eval(bad, &mut ev);
            let is_triangle = matches!(parsed, Ok(EvalResult::Triangle(_)));
            assert!(!is_triangle, "{bad} 被误判为三角形输入");
        }
        // 新语法仍能正常认领
        assert!(matches!(
            parse_and_eval("triangle(a=3 b=4 c=5)", &mut ev),
            Ok(EvalResult::Triangle(_))
        ));
    }
}
