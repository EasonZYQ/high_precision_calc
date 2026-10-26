//! 玻璃着色器**共用的一块 WGSL**（窗口折射 与 按键/面板折射 逐字共用）。
//!
//! ## 为什么单独抽一个模块
//!
//! `refraction.rs`（整窗背景）和 `liquid.rs`（每个按键/面板）都要做同一件事：
//! **在边缘环带里把背景样本"翻折"一下**。这两处以前各写一份，公式与常量
//! 慢慢就漂了 —— 表现是"按键里透出来的背景"和"窗口边缘的背景"接不上。
//! 现在两边都调这里的 [`window_refract`]，参数只有一个来源。
//!
//! ## 翻折（fold）是什么
//!
//! 用户 2026-10-26 对着 iOS 的实拍图指出："玻璃边缘约 10% 的地方会做一个
//! 翻折，斜线在那里被向另一个方向弯曲；越靠角上，越向角里扭"。
//!
//! 数学上就是**位移剖面在环带内变号**：
//! - `t = 0`（贴边）：向外推得最狠（`+1`）—— 于是贴边那几像素像"被压扁"；
//! - `t ≈ 0.44`（环带中段）：位移过零 —— **这就是那条翻折线**，两侧的
//!   背景内容朝相反方向弯，看起来像玻璃被折了一下；
//! - `t ≈ 0.78`：反向（向内吸）达到峰值（约 `-0.44`）；
//! - `t = 1`（环带内沿）：**严格归零**，与外圈无缝衔接。
//!
//! 角部再乘 [`corner_gain`]：法线越"斜"（越靠角）位移越强，于是角上的
//! 内容被更狠地拽向角落 —— 与实拍一致。

/// 窗口环带宽度 = **短边的比例**。
///
/// 0.18 是实拍图的量级：800px 高的窗口 ⇒ 环带 ~140px、翻折线在 ~60px 处
///（≈ 窗口的 8%，用户说的"大约 10% 的地方"）。
pub const WIN_BAND_FRAC: f32 = 0.18;

/// 窗口的"圆角半径"——**只用于折射法线**，不用于裁剪。
///
/// 窗口本身是方的（圆角由 DWM 画），但要让**角上**的法线指向对角，
/// SDF 必须带一点圆角，否则方角处的法线永远是轴对齐的，`corner_gain`
/// 就没有斜度可加权（实拍图里角上的扭曲明显强于直边）。
pub const WIN_CORNER_R: f32 = 26.0;

/// 窗口环带的**最大位移**（以窗口 uv 为单位，≈ 窗口尺寸的 11%）。
pub const WIN_AMP_UV: f32 = 0.085;

/// 窗口折射强度（与 `app.rs` 传给 `Refraction` 的值同源）。
///
/// 按键/面板采样背景时也用它 —— 否则"窗口里看到的背景"和"按键里透出来的
/// 背景"是两种弯曲，边界处会露馅。
pub const WIN_STRENGTH: f32 = 0.85;

/// 组装一个完整的 WGSL：常量头 + 共用块 + 各自的末尾（bindings / uniform / vs / fs）。
///
/// ⚠️ **常量用 `format!` 注入**，不在 WGSL 里手抄第二份 —— 抄一份就等于埋一个
/// "改了 Rust 忘了改 WGSL"的定时炸弹（本项目的 `band` 重名事故就是同类问题）。
pub fn shader(tail: &str) -> String {
    // `{:?}` 而不是 `{}`：f32 的 Debug 会保留小数点（`26.0`），Display 会打成 `26`，
    // 而 WGSL 里 `26` 是 i32 字面量，赋给 f32 直接编译失败。
    format!(
        "const WIN_BAND_FRAC: f32 = {band:?};\n\
         const WIN_CORNER_R: f32 = {corner:?};\n\
         const WIN_AMP_UV: f32 = {amp:?};\n\
         const WIN_STRENGTH: f32 = {strength:?};\n\
         {body}{tail}",
        band = WIN_BAND_FRAC,
        corner = WIN_CORNER_R,
        amp = WIN_AMP_UV,
        strength = WIN_STRENGTH,
        body = COMMON_BODY,
    )
}

/// 共用代码块。**不声明 bindings**（两套 shader 的 binding 顺序不同），
/// 但函数体可以直接引用 `bg` / `samp` —— WGSL 的作用域是整模块。
const COMMON_BODY: &str = r#"
/// 背景采样：**半径是浮点**，`rad` 用 uv 表达（0 附近直接单次采样）。
///
/// 8 tap = 中心（权重 2）+ 8 个环点，且**每个像素旋转一个随机角度** ——
/// 这是少采样也能得到平滑模糊的关键：固定图案在大半径下会看出"花瓣"，
/// 随机旋转把图案打散成人眼认不出的噪声。
fn sample_blur(uv: vec2<f32>, rad: vec2<f32>, seed: vec2<f32>) -> vec3<f32> {
    let lo = vec2<f32>(0.001);
    let hi = vec2<f32>(0.999);
    if (rad.x < 1e-4 && rad.y < 1e-4) {
        return textureSampleLevel(bg, samp, clamp(uv, lo, hi), 0.0).rgb;
    }
    let h = fract(sin(dot(seed, vec2<f32>(12.9898, 78.233))) * 43758.5453);
    let a0 = h * 6.2831853;
    let ca = cos(a0);
    let sa = sin(a0);
    // **两圈共 8 个环点**：内圈 4 个（0.55r）+ 外圈 4 个（1.0r）。
    // 单圈在半径大时会看出"星芒"（只有几根辐条），加一圈后图案密一倍。
    var ring = array<vec2<f32>, 8>(
        vec2<f32>(0.55, 0.00),
        vec2<f32>(-0.55, 0.00),
        vec2<f32>(0.00, 0.55),
        vec2<f32>(0.00, -0.55),
        vec2<f32>(0.72, 0.69),
        vec2<f32>(-0.72, 0.69),
        vec2<f32>(0.72, -0.69),
        vec2<f32>(-0.72, -0.69),
    );
    var acc = textureSampleLevel(bg, samp, clamp(uv, lo, hi), 0.0).rgb * 2.0;
    var wsum = 2.0;
    for (var i: i32 = 0; i < 8; i = i + 1) {
        let o = ring[i];
        let r = vec2<f32>(o.x * ca - o.y * sa, o.x * sa + o.y * ca);
        acc = acc + textureSampleLevel(bg, samp, clamp(uv + r * rad, lo, hi), 0.0).rgb;
        wsum = wsum + 1.0;
    }
    return acc / wsum;
}

// 圆角矩形的有向距离：负 = 在内部（单位 = 逻辑像素）
fn sd_rr(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// 圆角矩形的**解析法线**（单位向量，指向外）：直边上轴对齐、圆角处指向圆心
fn rr_normal(p: vec2<f32>, half: vec2<f32>, r: f32) -> vec2<f32> {
    let rr = max(r, 0.0);
    let inner = max(half - vec2<f32>(rr), vec2<f32>(0.0));
    let dv = p - clamp(p, -inner, inner);
    let ex = half.x - abs(p.x);
    let ey = half.y - abs(p.y);
    let flat_n = select(vec2<f32>(sign(p.x), 0.0), vec2<f32>(0.0, sign(p.y)), ey < ex);
    return select(flat_n, normalize(dv), length(dv) > 1e-4);
}

// 翻折剖面（iOS 液态玻璃的核心手感，见模块文档）
//
// `t` = 到边缘的距离 / 环带宽度（0 = 贴边，1 = 环带内沿）。
// 返回**带符号**的位移权重：> 0 向外推（外圈鼓出），< 0 向内吸（回折）。
//
// ⚠️ 参数名不能叫 `t`：WGSL（naga）**不允许 `let` 覆盖同名参数**，
// 写成 `let t = clamp(t, ..)` 会报 `redefinition of t`（实测踩过）。
fn fold_profile(dist: f32) -> f32 {
    let t = clamp(dist, 0.0, 1.0);
    // 外圈：贴边最猛、向里快速衰减（决定"够不够像玻璃"）
    let out_w = exp(-t / 0.22);
    // 内圈：靠近环带内沿才起来的反向吸力（翻折的来源）。
    // 系数 0.45 是**实测调出来的**：0.9 时反向位移几乎和正向一样大，
    // 在角上会把背景"撕"出一个硬三角（快照里一眼可见）；0.45 才像实拍图
    // 那种"折一下、但不翻脸"的程度。
    let in_w = 0.45 * exp(-(1.0 - t) / 0.36);
    // 到环带内沿**严格归零**，否则会出现一条硬接缝
    let cut = 1.0 - smoothstep(0.82, 1.0, t);
    return (out_w - in_w) * cut;
}

// 角部加权：法线越"斜"（越靠角）位移越强 —— 实拍图里角上折得更狠
fn corner_gain(n: vec2<f32>) -> f32 {
    let diag = clamp(abs(n.x * n.y) * 2.0, 0.0, 1.0);
    return 1.0 + 0.40 * diag * diag;
}

// 到窗口边缘的**向内**距离（px，负 = 在窗口外）
fn window_inward(s: vec2<f32>, win: vec2<f32>) -> f32 {
    let half = win * 0.5;
    return -sd_rr((s - vec2<f32>(0.5)) * win, half, WIN_CORNER_R);
}

// 窗口环带的折射位移（返回**窗口 uv** 的偏移量）。
//
// `s` = 窗口 uv（y 向下）。两套着色器都用它 —— 这样按键里透出来的背景
// 与窗口边缘看到的是同一次弯曲。
fn window_refract(s: vec2<f32>, win: vec2<f32>, strength: f32) -> vec2<f32> {
    let half = win * 0.5;
    let p = (s - vec2<f32>(0.5)) * win;
    let inward = -sd_rr(p, half, WIN_CORNER_R);
    let band = WIN_BAND_FRAC * min(win.x, win.y);
    let t = inward / max(band, 1.0);
    // 环带之外：**严格零位移**（快路径要靠这个判据，不能"接近 0"）
    if (t >= 1.0) {
        return vec2<f32>(0.0);
    }
    let n = rr_normal(p, half, WIN_CORNER_R);
    return n * (fold_profile(t) * corner_gain(n) * WIN_AMP_UV * strength);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// 两套着色器都必须真的**带上共用块**（否则窗口和按键各弯各的）。
    ///
    /// 这是"字符串级"的守卫：WGSL 不能在这里编译，但至少能保证
    /// 有人把 `shader()` 改回裸 tail、或把共用块删掉时测试会红。
    #[test]
    fn shader_always_includes_shared_helpers() {
        let s = shader("// tail\n");
        for needle in [
            "fn sample_blur",
            "fn sd_rr",
            "fn rr_normal",
            "fn fold_profile",
            "fn corner_gain",
            "fn window_refract",
            "fn window_inward",
        ] {
            assert!(s.contains(needle), "共用块缺少 {needle}");
        }
        assert!(s.ends_with("// tail\n"), "tail 必须拼在共用块之后");
    }

    /// Rust 常量要以**字面量**注入 WGSL（改常量时两边不会漂）。
    ///
    /// ⚠️ `{:?}` 而不是 `{}`：`26.0` 必须带小数点，否则 WGSL 里是 i32。
    #[test]
    fn rust_constants_are_injected_with_float_literals() {
        let s = shader("");
        assert!(s.contains(&format!("const WIN_BAND_FRAC: f32 = {WIN_BAND_FRAC:?};")));
        assert!(s.contains(&format!("const WIN_CORNER_R: f32 = {WIN_CORNER_R:?};")));
        assert!(s.contains(&format!("const WIN_AMP_UV: f32 = {WIN_AMP_UV:?};")));
        assert!(s.contains(&format!("const WIN_STRENGTH: f32 = {WIN_STRENGTH:?};")));
        // 整数形式（`26;`）会让着色器编译失败，这里直接盯住它
        assert!(s.contains("WIN_CORNER_R: f32 = 26.0;"));
    }
}
