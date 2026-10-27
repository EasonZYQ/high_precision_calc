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

/// 窗口环带宽度 = **短边的比例**（用户 2026-10-27 定为 15%）。
///
/// 折痕落在离边 `band` 处：940×900 的窗口 ⇒ 折痕在离边 135px（≈15%）的地方。
pub const WIN_BAND_FRAC: f32 = 0.15;

/// 窗口的"圆角半径"——**只用于折射**，不用于裁剪。
///
/// 两处用到：① 到边界的 SDF；② **角部圆弧折痕的半径**（折痕 = 以该角圆心为
/// 极点的同心圆弧，半径 R + band）。窗口本身是方的（圆角由 DWM 画），
/// 但折射必须按圆角来算，否则角上会出现尖点（见 `glass_fold` 的注释）。
pub const WIN_CORNER_R: f32 = 26.0;

/// 镜面系数：位移 = `WIN_MIRROR × 环带`（贴边处）。
///
/// `2.0` = **精确镜像**：采样点与屏幕点关于折痕等距对称 ⇒ 折痕外侧画的是
/// 倒影（用户要的"以边为镜面反射"）。`1.0` 会退化成"糊到折痕那一条线"，
/// `< 1` 只是压缩、`> 2` 是放大的倒影。
pub const WIN_MIRROR: f32 = 2.0;

/// 角部加成：`幅度 × (1 + WIN_CORNER_BOOST · 角权)`。
///
/// 用户 2026-10-27："越靠近角扭曲越大"（而不是越靠边越大）。
pub const WIN_CORNER_BOOST: f32 = 0.6;

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
         const WIN_MIRROR: f32 = {mirror:?};\n\
         const WIN_CORNER_BOOST: f32 = {boost:?};\n\
         const WIN_STRENGTH: f32 = {strength:?};\n\
         {body}{tail}",
        band = WIN_BAND_FRAC,
        corner = WIN_CORNER_R,
        mirror = WIN_MIRROR,
        boost = WIN_CORNER_BOOST,
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

// 平滑阶跃（0→1，两端斜率为 0）——角权用它，保证处处 C1
fn smooth01(x: f32) -> f32 {
    let t = clamp(x, 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

// 玻璃折痕：返回**朝内**的像素位移向量（调用方加在采样坐标上）。
//
// ## 规则（用户 2026-10-27 定稿，仿真 `glass-sim8.py` 验证过）
//
// - **直边**：两轴镜像折叠。屏幕点离边 `d`、折痕在离边 `band` 处，
//   贴边处的采样点关于折痕对称 ⇒ 折痕外侧画的是**倒影**；
// - **角部**：折痕换成**以该角圆角圆心为极点的同心圆弧（半径 R + band）**，
//   方向沿径向指向角心，幅度再乘 `(1 + boost·角权)` ⇒ **越靠角越强**；
// - 角权 `wc` 用 smoothstep 连续过渡，**没有任何分支切换** ⇒ 不存在
//   "某条斜线"这种分界（上一版的 45° 斜线就是这么来的）。
//
// ## 为什么角部必须换成圆弧（关键）
//
// 矩形内缩 `band` 后的折痕在 `band > R` 时会**退化成直角**：两条折痕相交处
// 变成一个尖点，镜像把内容全挤到那儿 —— 用户实测"角上是尖的"。
// 换成同心圆弧后折痕是光滑曲线，尖点自动消失，而且内容会顺着弧被绕向角。
//
// ⚠️ 不能写成"取最近边的法线 + 角上加权"：`min(dx,dy)` 在直边中部也≈0，
// 会把整圈都当成角区；必须用**到角弧的距离**（这里的 `ρ`）。
fn glass_fold(p: vec2<f32>, half: vec2<f32>, radius: f32, band: f32,
              amp: f32, boost: f32) -> vec2<f32> {
    let d = -sd_rr(p, half, radius);
    let b = max(band, 1.0);
    if (d >= b) {
        return vec2<f32>(0.0);
    }
    // 到左右/上下边的距离，以及"朝外"的符号
    let dx = half.x - abs(p.x);
    let dy = half.y - abs(p.y);
    let sx = select(-1.0, 1.0, p.x >= 0.0);
    let sy = select(-1.0, 1.0, p.y >= 0.0);

    // 角部：以该角的圆角圆心为极点
    let cc = vec2<f32>((half.x - radius) * -sx, (half.y - radius) * -sy);
    let rel = p - cc;
    let rho = max(length(rel), 1e-4);
    let wc = smooth01((radius + b - rho) / b);

    // 距离：直边用"到最近边"，角部用"到圆弧"，按角权混合
    let dm = mix(min(dx, dy), rho - radius, wc);

    // 方向：直边法线 → 径向（都指向"朝外"），混合后归一
    let ne = normalize(vec2<f32>(sx / (dx + 1.0), sy / (dy + 1.0)));
    let nc = -rel / rho;
    let n = normalize(mix(ne, nc, wc));

    // 幅度：贴边最大（t=1）、折痕处为 0；角部再放大
    let t = max(0.0, 1.0 - dm / b);
    let mag = amp * t * t * (1.0 + boost * wc);
    // `n` 朝外 ⇒ 采样朝内用负号
    return -n * mag;
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
    let band = WIN_BAND_FRAC * min(win.x, win.y);
    let off = glass_fold(p, half, WIN_CORNER_R, band, WIN_MIRROR * band * strength,
                         WIN_CORNER_BOOST);
    return off / win;
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
            "fn smooth01",
            "fn glass_fold",
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
        assert!(s.contains(&format!("const WIN_MIRROR: f32 = {WIN_MIRROR:?};")));
        assert!(s.contains(&format!(
            "const WIN_CORNER_BOOST: f32 = {WIN_CORNER_BOOST:?};"
        )));
        assert!(s.contains(&format!("const WIN_STRENGTH: f32 = {WIN_STRENGTH:?};")));
        // 整数形式（`26;`）会让着色器编译失败，这里直接盯住它
        assert!(s.contains("WIN_CORNER_R: f32 = 26.0;"));
    }
}
