//! 液态玻璃按键外观 —— iOS liquid glass 在 iced 里的实现。
//!
//! ## 参考配方
//!
//! 项目根目录 `纯CSS液态玻璃/css/styles.css`。那份 CSS 的每个 `.box` 是 5 层叠加，
//! 缺一层就不像玻璃：
//!
//! | 层 | CSS 写法 | 作用 |
//! |---|---|---|
//! | ① 主体 | `background: rgba(255,255,255,0.04)` + `filter: brightness(0.9)` | 玻璃本体（微透） |
//! | ② 扫光 | `:after` 的 45° 渐变 + `blur(3px)` | 两端各一条高光带，玻璃的"反光" |
//! | ③ 内环高光 | `inset ±2px / ±6px` 白色 + `inset 0 0 2px` 黑色 | 玻璃边缘那圈亮边 |
//! | ④ 内腔暗环 | `:before` 的 `border + blur(8px)`，偏下 | 玻璃有厚度的暗部 |
//! | ⑤ 外边缘 | `1px double rgba(51,51,51,0.08)` + 外投影 | 轮廓 |
//!
//! 还有一条关键的**整体**手段：容器上的 `filter: contrast(3)`。它把上面那些
//! 模糊过的软边压成硬边，才是"液态"观感的来源。
//!
//! ## 为什么用 shader 而不是 iced 的样式
//!
//! iced 的 `button::Style` 只有「1 个背景 + 1 个边框 + 1 个外阴影」：
//! - **没有 inset 阴影** ⇒ ③ 和 ④（玻璃感的两大来源）根本表达不出来；
//! - 容器上的 `filter: contrast()`、`backdrop-filter` 也一概没有。
//!
//! 所以 ②③④ 必须自己逐像素算。可选的三条路：
//!
//! 1. **`widget::canvas`**：能用，但它的几何缓存只在**尺寸变化**时失效，
//!    hover / 主题切换不会失效（见 `iced_graphics::geometry::Cache`）；
//!    不用缓存则每个按键每帧都要重新三角化，37 个按键 × 5 个图元太贵。
//! 2. **叠容器**：iced 的 `padding` 只缩小子元素、**不内缩边框**，
//!    要做出内环得 `container(container(..).style(ring)).padding(n)`，
//!    每个按键多 4 个 widget，37 个按键就是 200 多个元素。
//! 3. **shader + SDF（本模块的做法）**：圆角矩形用有向距离场算，
//!    GPU 一个片元里把 5 层全算完 —— 每帧 CPU 只拷一个 64 字节的 uniform，
//!    每个按键一次 3 顶点的 draw call。
//!
//! 顺便，shader 里的 `smoothstep` 天然就是 CSS 里的 `blur()` + `contrast()`
//! 的合体：软边由 `smoothstep` 的宽度控制，边沿硬度由区间的窄度控制。
//!
//! ## 关于"每个按键各自的 uniform"
//!
//! iced 的 wgpu 后端是**先把所有图元 `prepare` 一遍，再逐个 `draw`**
//!（`iced_wgpu/src/lib.rs` 的 `layer.primitives` 循环），所以共享一个 uniform
//! buffer 会让所有按键都用到**最后一个**的参数。
//!
//! 这里按"外观参数"哈希出一批 buffer 共用：相同外观的按键（同一种分组、
//! 同一种状态、同一个尺寸）共用一份，参数变了才落到另一个槽位。
//! 槽位数有上限，超了整体清空，避免窗口拖动时尺寸连续变化导致无限增长。

use iced::wgpu;
use iced::widget::shader;
use iced::{Rectangle, mouse};
use std::sync::{Arc, RwLock};

// ── 背景共享 ────────────────────────────────────────────────────────────────
//
// 按钮的折射需要**采样背景**（用户规格："按钮是在背景扭曲过的基础上再扭曲"）。
// 背景像素在 `app` 的 `bg_pixels` 里（抓屏 + 模糊的产物），但 `Glass` 是 `Copy`
// 的小结构体，不宜把 800KB 的 `Arc` 塞进去逐层传递 —— 这里用一个进程级槽位，
// `app::view()` 每帧把最新像素同步进来（`Arc` 引用计数，零拷贝）。
//
// 着色器管线只在**版本号变化**时才重传纹理（与 `refraction.rs` 同一套策略）。

/// 当前可供玻璃着色器采样的背景快照。
#[derive(Debug, Default)]
struct Backdrop {
    /// `None` = 没有背景可采样（关闭质感 / 首帧还没抓到屏）
    pixels: Option<Arc<Vec<u8>>>,
    width: u32,
    height: u32,
    /// 每次抓到新背景 +1；管线拿它跳过重复的纹理上传
    version: u64,
    /// 窗口→背景图的 uv 映射（y 向下）：`图像uv = 窗口uv × scale + off`。
    /// 窗口挪动时它跟着变 —— 按钮里的折射背景就"跟手"，拖动零抓屏。
    map: ([f32; 2], [f32; 2]),
}

static BACKDROP: RwLock<Backdrop> = RwLock::new(Backdrop {
    pixels: None,
    width: 0,
    height: 0,
    version: 0,
    map: ([1.0, 1.0], [0.0, 0.0]),
});

/// `app::view()` 每帧调用：把最新的背景像素（或"没有背景"）同步给着色器。
///
/// 全是引用计数与整数比较，每帧开销可忽略。
pub fn sync_backdrop(
    pixels: Option<&Arc<Vec<u8>>>,
    width: u32,
    height: u32,
    version: u64,
    map: ([f32; 2], [f32; 2]),
) {
    if let Ok(mut b) = BACKDROP.write() {
        b.pixels = pixels.cloned();
        b.width = width;
        b.height = height;
        b.version = version;
        b.map = map;
    }
}

/// 液态玻璃的外观参数。
///
/// ⚠️ **尺寸不在里面**：`Program::draw` 拿得到真实的 `bounds`，比调用方猜的准。
/// 也不必有 hover 状态 —— `draw` 同样拿得到 `cursor`，自己判定即可（见下）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glass {
    /// 圆角半径（胶囊形 = `height / 2`）
    pub radius: f32,
    /// 底色（`[r, g, b, a]`，各通道 0..1）。分组色由 `theme` 烘进来。
    pub base: [f32; 4],
    /// 深色主题（内腔暗环在深色下要更重一点）
    pub dark: bool,
    /// 高光总强度（0 = 无高光的纯色块，1 = 照抄参考稿）
    pub spec: f32,
    /// **边缘折射厚度**（逻辑像素）：从边缘向内这么深的范围里，背景被
    /// "凸透镜"式地弯出去 —— 越厚玻璃显得越厚。按键 ~15px，大面板要
    /// 30px+ 才有"厚玻璃板"的分量（用户反馈："做好四周的折射"）。
    pub thick: f32,
    /// 边缘折射**强度**：边缘处背景最大偏移（归一化 uv）。
    /// 按键 0.05（小件轻弯），面板 0.10~0.14（大板的边看得见明显的弯）。
    pub bend: f32,
}

impl<Message> shader::Program<Message> for Glass {
    type State = ();
    type Primitive = GlassPrimitive;

    fn draw(
        &self,
        _state: &Self::State,
        cursor: mouse::Cursor,
        bounds: Rectangle,
    ) -> Self::Primitive {
        GlassPrimitive {
            glass: *self,
            w: bounds.width,
            h: bounds.height,
            // ⚠️ hover 只能在这里判：`view()` 构造 widget 树时还没有光标信息，
            // 而 `.style()` 闭包虽然拿得到 `button::Status`，但那已经是**绘制**阶段了
            //（那时图元已经建好）。这里是唯一同时有 bounds 与 cursor 的地方。
            hover: cursor.position_over(bounds).is_some(),
            ox: bounds.x,
            oy: bounds.y,
        }
    }
}

/// 交给 GPU 的那一份（= `Glass` + 布局结果与 hover）
#[derive(Debug, Clone, Copy)]
pub struct GlassPrimitive {
    glass: Glass,
    w: f32,
    h: f32,
    hover: bool,
    /// 本部件在**窗口**里的位置（逻辑像素）—— 折射采样背景时要用
    /// 窗口坐标：`窗口uv = (origin + 局部uv × size) / 窗口尺寸`。
    ox: f32,
    oy: f32,
}

impl GlassPrimitive {
    /// 缓存键：外观完全一致（含尺寸与 hover）的部件共用一份 uniform
    fn key(self) -> Key {
        // 尺寸量化到 0.5px —— 布局取整造成的 0.01px 抖动不该产生新槽位
        Key {
            w: (self.w * 2.0).round() as i32,
            h: (self.h * 2.0).round() as i32,
            radius: (self.radius() * 2.0).round() as i32,
            spec: (self.glass.spec * 100.0).round() as i32,
            thick: (self.glass.thick * 2.0).round() as i32,
            bend: (self.glass.bend * 1000.0).round() as i32,
            hover: self.hover,
            dark: self.glass.dark,
            base: [
                (self.glass.base[0] * 255.0).round() as i32,
                (self.glass.base[1] * 255.0).round() as i32,
                (self.glass.base[2] * 255.0).round() as i32,
                (self.glass.base[3] * 255.0).round() as i32,
            ],
        }
    }

    /// 圆角不能超过短边的一半，否则 SDF 会退化（`b - r` 变负导致形状破面）
    fn radius(self) -> f32 {
        self.glass.radius.min(self.w.min(self.h) * 0.5).max(0.0)
    }

    /// uniform 内容（96 字节，std140：所有字段按 vec4 对齐）。
    ///
    /// ⚠️ 窗口尺寸在这里（而不是 `Glass` 里）传入：只有 `prepare` 拿得到
    /// `shader::Viewport`（= 整个窗口的逻辑尺寸），`Program::draw` 没有。
    fn data(self, win: (f32, f32), has_bg: bool, map: ([f32; 2], [f32; 2])) -> [u8; 96] {
        let g = self.glass;
        let mut d = [0u8; 96];
        d[0..4].copy_from_slice(&self.w.to_le_bytes());
        d[4..8].copy_from_slice(&self.h.to_le_bytes());
        d[8..12].copy_from_slice(&self.radius().to_le_bytes());
        d[12..16].copy_from_slice(&g.spec.to_le_bytes());
        for (i, c) in g.base.iter().enumerate() {
            d[16 + i * 4..20 + i * 4].copy_from_slice(&c.to_le_bytes());
        }
        // p：x = hover，y = 深色主题，z = 有无背景可采样（w 备用）
        d[32..36].copy_from_slice(&(if self.hover { 1.0f32 } else { 0.0 }).to_le_bytes());
        d[36..40].copy_from_slice(&(if g.dark { 1.0f32 } else { 0.0 }).to_le_bytes());
        d[40..44].copy_from_slice(&(if has_bg { 1.0f32 } else { 0.0 }).to_le_bytes());
        // p.w = **模糊半径**（窗口逻辑像素）—— 着色器里折算成 texel 后做多 tap 采样
        d[44..48].copy_from_slice(&crate::theme::blur_px().to_le_bytes());
        // win / origin：折射采样的窗口坐标换算用
        d[48..52].copy_from_slice(&win.0.to_le_bytes());
        d[52..56].copy_from_slice(&win.1.to_le_bytes());
        d[56..60].copy_from_slice(&self.ox.to_le_bytes());
        d[60..64].copy_from_slice(&self.oy.to_le_bytes());
        // map：窗口→背景图 uv 映射（scale.xy + off.xy，y 向下）
        d[64..68].copy_from_slice(&map.0[0].to_le_bytes());
        d[68..72].copy_from_slice(&map.0[1].to_le_bytes());
        d[72..76].copy_from_slice(&map.1[0].to_le_bytes());
        d[76..80].copy_from_slice(&map.1[1].to_le_bytes());
        // edge：x = 边缘折射厚度（px），y = 折射强度（边缘最大偏移）
        d[80..84].copy_from_slice(&g.thick.to_le_bytes());
        d[84..88].copy_from_slice(&g.bend.to_le_bytes());
        d
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    w: i32,
    h: i32,
    radius: i32,
    spec: i32,
    /// 边缘折射厚度（0.5px 量化）—— 不同厚度的面板/按键不能共享槽位
    thick: i32,
    /// 边缘折射强度（1/1000 量化）
    bend: i32,
    hover: bool,
    dark: bool,
    base: [i32; 4],
}

impl shader::Primitive for GlassPrimitive {
    type Pipeline = GlassPipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &shader::Viewport,
    ) {
        // 先确保背景纹理是最新的一版（版本号没变就是纯整数比较）
        let bg = pipeline.ensure_backdrop(device, queue);
        // 窗口逻辑尺寸只有这里拿得到（`Program::draw` 没有 Viewport）
        let ls = viewport.logical_size();
        let (has_bg, map) = match bg {
            Some(m) => (true, m),
            None => (false, ([1.0, 1.0], [0.0, 0.0])),
        };
        pipeline.upload(
            device,
            queue,
            self.key(),
            self.data((ls.width, ls.height), has_bg, map),
        );
        // ⚠️ `bounds` 参数与 `Program::draw` 的 bounds 是同一个，
        // 这里取一次的 `bounds.x/y` 与 draw 时一致；ox/oy 已在 draw 里带上。
        let _ = bounds;
    }

    fn draw(&self, pipeline: &Self::Pipeline, pass: &mut wgpu::RenderPass<'_>) -> bool {
        pipeline.paint(pass, self.key())
    }
}

/// 所有 [`GlassPrimitive`] 共用的 GPU 资源（iced 保证只 `new` 一次）
pub struct GlassPipeline {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// 外观 → 参数槽。相同外观的按键共用一份 buffer + bind group。
    ///
    /// ⚠️ **必须要有上限**：窗口拖动时按键宽度是连续变化的，量化到 0.5px 之后
    /// 仍然会有很多种尺寸；不加限制的话这张表会一直长。
    ///
    /// 第三项是**上次写进 buffer 的字节**，用来跳过没变化的 `write_buffer`
    ///（见 [`GlassPipeline::upload`]）。
    slots: std::collections::HashMap<Key, (wgpu::Buffer, wgpu::BindGroup, [u8; 96])>,
    /// 背景纹理（抓屏 + 模糊的产物，与 `refraction.rs` 看到的是同一份像素）。
    /// 没有背景时绑 `fallback`（1×1 黑）—— WGSL 不能"不绑定"，用 uniform 里的
    /// `has_bg` 标志决定是否采样。
    texture: Option<(wgpu::Texture, wgpu::TextureView, (u32, u32))>,
    fallback: (wgpu::Texture, wgpu::TextureView),
    /// 上一次上传的背景版本号
    tex_version: u64,
}

/// 槽位上限。超过就整表清空（下一帧按需重建）—— 64 字节 × 1 个 buffer，
/// 即便全占满也只几十 KB，代价可以忽略。
const MAX_SLOTS: usize = 256;

const WGSL_TAIL: &str = r#"
struct U {
    size: vec2<f32>,
    radius: f32,
    spec: f32,
    base: vec4<f32>,
    // x = hover，y = 深色主题，z = 有无背景可采样，w = 模糊半径（逻辑像素）
    p: vec4<f32>,
    // win = 窗口逻辑尺寸，origin = 本部件在窗口里的位置
    win: vec2<f32>,
    origin: vec2<f32>,
    // 窗口→背景图 uv 映射：图像uv = 窗口uv × map.xy + map.zw（y 向下）
    map: vec4<f32>,
    // x = 边缘折射厚度（px），y = 边缘折射强度（边缘处背景最大偏移）
    edge: vec2<f32>,
};

@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var bg: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// 全屏三角形：三个顶点盖满整个 bounds（比分两个三角形少一次顶点设置）
@vertex
fn vs(@builtin(vertex_index) i: u32) -> VOut {
    var p = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    let q = p[i];
    var o: VOut;
    o.pos = vec4<f32>(q, 0.0, 1.0);
    // NDC [-1,1] → UV [0,1]。⚠️ wgpu 里 NDC 的 y 向上，所以 **uv.y = 1 是顶边**
    o.uv = (q + vec2<f32>(1.0)) * 0.5;
    return o;
}

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let half = u.size * 0.5;
    // 以按钮中心为原点、以逻辑像素为单位的坐标
    let p = (uv - vec2<f32>(0.5)) * u.size;
    let d = sd_rr(p, half, max(u.radius, 0.0));
    // 距边缘**向内**的距离（px）；边缘外为负
    let inward = -d;
    // 边缘反锯齿：1 个逻辑像素的过渡带
    let edge = 1.0 - smoothstep(-0.6, 0.6, d);
    if (edge <= 0.001) {
        discard;
    }

    // 受光方向：1 = 左上（光源方向），0 = 右下
    let dir = clamp(((1.0 - uv.x) + uv.y) * 0.5, 0.0, 1.0);

    // ── ① 主体：顶边受光提亮、底边背光压暗 ──────────────────────────────
    var rgb = u.base.rgb;
    rgb = mix(rgb, vec3<f32>(1.0), 0.07 * uv.y);
    rgb = mix(rgb, vec3<f32>(0.0), 0.05 * (1.0 - uv.y));

    // ── ⓪ 背景：在"已被窗口边缘扭曲过的背景"之上再做按键自己的凸透镜 ──
    //（用户规格："按钮是在背景扭曲过的基础上再扭曲"）
    //
    // ⚠️ **坐标系**：抓屏行序是 top-down（纹理 v = 0 是屏幕最上面），
    // 而 `in.uv` 是 NDC 的 y 向上 —— 直接当 v 用会上下颠倒。这里统一转成
    // **y 向下**的窗口坐标 `s`（与 refraction.rs 同一约定），采样 v = s.y。
    //
    // 两层扭曲按顺序叠加：
    // a. **窗口边缘环带**：调用与 `refraction.rs` **同一个** `window_refract`
    //    —— 按键采到的就是用户在外面看到的那个"已经翻折过的背景"；
    // b. **按钮局部透镜**：同一个 `glass_fold`，尺寸按部件短边缩放 ——
    //    这就是玻璃按键"透出来的东西在边上先鼓出、再折回来"的来源。
    if (u.p.z > 0.5) {
        let s = vec2<f32>(
            u.origin.x + uv.x * u.size.x,
            u.origin.y + (1.0 - uv.y) * u.size.y,
        ) / u.win;
        let short = min(u.size.x, u.size.y);
        // 局部环带：`thick`（随圆角走）是下限，同时不小于短边的 15%
        //（与窗口同一个 `WIN_BAND_FRAC`：折痕落在离边 15% 处）
        let lens_band = max(max(u.edge.x, 1.0), 0.15 * short);

        // 未经任何位移的采样点（窗口 uv → 图像 uv）
        let base = s * u.map.xy + u.map.zw;
        // 模糊半径：逻辑像素 → 窗口 uv → 图像 uv → texel
        let rad = (u.p.w * u.map.xy / u.win) * vec2<f32>(textureDimensions(bg));
        let seed = u.origin + uv * u.size;

        // 窗口那一段的位移（与 refraction.rs 逐字同源）
        let ring_off = window_refract(s, u.win, WIN_STRENGTH);

        var bgc: vec3<f32>;
        // ══ 快路径：面板"中间那块" ════════════════════════════════════════
        //
        // 用户要求"中间只用一次采样"，但**必须无缝** —— 所以判据不能是"猜一个阈值"，
        // 必须是**位移恰好为 0** 的地方：
        // - 局部透镜：`glass_fold` 在 `d >= band` 时**提前 return 0**；
        // - 窗口环带：`window_refract` 在 `t >= 1` 时**提前 return 0**。
        //
        // 两处都是"严格 0"而不是"接近 0"，所以快路径采的就是 `base` 本身，
        // 与完整路径**数学上完全等价** ⇒ 不会出现接缝/割裂（拖动窗口、背景变化都不怕）。
        // 模糊仍然照做（否则中间清晰、边缘糊，那才是真正的割裂）。
        if (inward >= lens_band && ring_off.x == 0.0 && ring_off.y == 0.0) {
            bgc = sample_blur(base, rad, seed);
        } else {
            // ── 完整路径：窗口环带 + 面板自身的翻折透镜 ──────────────────
            //
            // **和窗口用同一个 `glass_fold`**（直边镜像折叠、角部走圆弧折痕、
            // 越靠角越强），只是尺寸按部件短边缩放 —— 于是按键/面板与窗口
            // 折的是同一个形状，边界处不会露馅。
            //
            // `p` 是 y 向上的局部坐标，采样空间 y 向下 ⇒ 位移的 y 要取反。
            let amp_px = u.edge.y * 4.5 * lens_band;
            let fo = glass_fold(p, half, max(u.radius, 0.0), lens_band, amp_px,
                                WIN_CORNER_BOOST);
            let dloc = vec2<f32>(fo.x, -fo.y) * (u.map.xy / u.win);

            let disp = base + ring_off + dloc;
            bgc = sample_blur(disp, rad, seed);
            // ⚠️ **色散要作用在"已位移"的点上**，而且绿通道也必须位移 ——
            // 上一版把 G 换成了 `sample_blur(base)`，等于把透镜位移丢掉了
            // （只剩彩边、没有弯折）。这里三个通道都从 `disp` 出发。
            if (u.p.w < 2.5) {
                let lo = vec2<f32>(0.001);
                let hi = vec2<f32>(0.999);
                let kr = textureSampleLevel(bg, samp, clamp(disp + dloc * 0.12, lo, hi), 0.0).r;
                let kb = textureSampleLevel(bg, samp, clamp(disp - dloc * 0.12, lo, hi), 0.0).b;
                bgc = vec3<f32>(kr, bgc.g, kb);
            }
        }
        // **提饱和**：玻璃会把背后的颜色"喂"得更浓郁（iOS 特征之一）。
        // 不做的话背景被 t 色压淡之后会显得灰。
        let luma = dot(bgc, vec3<f32>(0.2126, 0.7152, 0.0722));
        bgc = mix(vec3<f32>(luma), bgc, 1.18);
        // 玻璃本体 = 折射后的背景染上分组底色。底色 alpha 越大越"实"，
        // 0.18 的保底让全透明的分组也带一点玻璃白。
        let tint_k = clamp(u.base.a * 1.6 + 0.18, 0.0, 1.0);
        rgb = mix(bgc, rgb, tint_k);
    }

    // ── ② 扫光：45° 轴上两端各一条高光带 ────────────────────────────────
    // `t` = 沿 ↗ 轴的归一化位置（0 = 左下，1 = 右上）
    let t = (uv.x + uv.y) * 0.5;
    // `s` = 到两端的接近程度：1 = 两端，0 = 正中
    let s = abs(t * 2.0 - 1.0);
    // 二次幂让高光只集中在两端，中间留干净 —— 这就是 CSS 里
    // `transparent 25% … 75%` 那两个空档加上 `blur(3px)` 的效果
    let sheen = pow(clamp(s, 0.0, 1.0), 2.2);

    // ── ③ 内环高光：贴着边缘最亮，向内 4.5px 衰减到 0 ───────────────────
    let rim_t = clamp(1.0 - inward / 4.5, 0.0, 1.0);
    // 左上更亮、右下几乎不亮 —— 等价于 CSS 那两对 `inset ±2px / ±6px`
    let rim = pow(rim_t, 1.5) * mix(0.20, 1.0, dir);

    // ── ④ 内腔暗环：1.2px 处起、10px 处消，偏右下 ──────────────────────
    // 对应 CSS `:before` 那个 `border + blur(8px)`、`top: 35%` 的暗环
    let cav = (1.0 - clamp(inward / 10.0, 0.0, 1.0))
            * clamp(inward / 1.2, 0.0, 1.0)
            * mix(1.0, 0.12, dir);

    // 悬停时高光更亮（按下的反馈不在这里做 —— `Program::draw` 只有 cursor，
    // 拿不到 button 的按下状态；按下由外面 `button` 自己的底色变化负责）
    let lit = select(1.0, 1.22, u.p.x > 0.5);
    let dark_k = u.p.y;

    // ── ③' 边缘镜面高光（M2）：法线 · 光源 ────────────────────────────────
    // 旧实现的高光只看 `uv` 的固定 45° 方向，四条边的亮度是"画上去的"。
    // 用真法线点乘光源方向后，**曲率大的地方自己就亮** —— 圆角比直边亮、
    // 朝光的那一侧亮，这正是 iOS 玻璃的 rim light。
    //
    // ⚠️ 这一段在 `if (u.p.z > 0.5)` 之外，所以法线要**无条件**算一次。
    // 上面折射段里那份在 `if` 内、作用域不同 —— 这里按同一套解析式再算一遍，
    // 仍然比"再来 4 次 sd_rr"便宜得多。
    let rr2 = max(u.radius, 0.0);
    let inner2 = max(half - vec2<f32>(rr2), vec2<f32>(0.0));
    let dv2 = p - clamp(p, -inner2, inner2);
    let ex2 = half.x - abs(p.x);
    let ey2 = half.y - abs(p.y);
    let n_r = select(
        select(vec2<f32>(sign(p.x), 0.0), vec2<f32>(0.0, sign(p.y)), ey2 < ex2),
        normalize(dv2),
        length(dv2) > 1e-4,
    );
    // 光从左上来（y 向上 ⇒ 上方是 +y）
    let ldir = normalize(vec2<f32>(-0.45, 0.75));
    let ndl = clamp(dot(n_r, ldir), 0.0, 1.0);
    let rim_t2 = clamp(1.0 - inward / max(0.12 * min(u.size.x, u.size.y), 10.0), 0.0, 1.0);
    let spec_rim = pow(ndl, 5.0) * pow(rim_t2, 2.2);
    // 背光侧给一点点压暗，让"厚度"读得出来（玻璃厚的地方会吃光）
    rgb = rgb * (1.0 - 0.10 * rim_t2 * (1.0 - ndl));

    // 高光是**反射**，用加算；内腔是遮挡，用减算
    let hl = (rim * 0.70 + sheen * 0.85 + spec_rim * 0.55) * u.spec * lit;
    rgb = rgb + vec3<f32>(1.0) * hl;
    // 深色主题下暗环更重一点（浅色下太重会脏）
    rgb = rgb - vec3<f32>(1.0) * cav * mix(0.22, 0.34, dark_k);
    rgb = clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0));

    // alpha：玻璃越透，边缘那圈高光也该看得见（真实的玻璃边就是亮的），
    // 所以高光本身要贡献一部分不透明度
    let a = clamp(u.base.a + hl * 0.42, 0.0, 1.0) * edge;

    return vec4<f32>(rgb, a);
}
"#;

impl GlassPipeline {
    fn build(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("hipercalc-liquid"),
            source: wgpu::ShaderSource::Wgsl(crate::glass_wgsl::shader(WGSL_TAIL).into()),
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hipercalc-liquid-layout"),
            entries: &[
                // uniform（外观参数）
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // 背景纹理（抓屏 + 模糊的产物）
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // 采样器
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("hipercalc-liquid-pipeline-layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("hipercalc-liquid-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                // 全屏三角形不吃顶点缓冲 —— 顶点坐标在 WGSL 里按 index 算
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("hipercalc-liquid-sampler"),
            // 折射的 uv 可能越界（边缘弯出去一点），夹取 = 最后一道保险
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // 1×1 黑色兜底纹理：没有背景可采样（关闭质感 / 首帧）时绑定它，
        // WGSL 里配合 `has_bg` 标志跳过采样 —— 管线不能"什么都不绑"。
        let fallback_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("hipercalc-liquid-fallback"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let fallback_view = fallback_tex.create_view(&wgpu::TextureViewDescriptor::default());

        Self {
            pipeline,
            layout,
            sampler,
            slots: std::collections::HashMap::new(),
            texture: None,
            fallback: (fallback_tex, fallback_view),
            // ⚠️ 用 `MAX` 起步：首次抓屏的 version 一定 ≠ MAX ⇒ 第一帧必上传
            tex_version: u64::MAX,
        }
    }

    /// 把 [`BACKDROP`] 里最新的背景上传成纹理。
    ///
    /// 返回 `Some((scale, off))` = 有背景可采样（连同 uv 映射一起给 uniform）；
    /// `None` = 没有背景。版本号没变就只做一次整数比较 ——
    /// 与 `refraction.rs` 同一套省带宽策略。
    fn ensure_backdrop(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Option<([f32; 2], [f32; 2])> {
        let Ok(b) = BACKDROP.read() else {
            return None;
        };
        let Some(pixels) = &b.pixels else {
            return None;
        };
        if b.width == 0 || b.height == 0 {
            return None;
        }
        let expected = (b.width as usize) * (b.height as usize) * 4;
        if pixels.len() < expected {
            return None;
        }

        let dims_ok =
            matches!(&self.texture, Some((_, _, (w, h))) if *w == b.width && *h == b.height);
        if dims_ok && b.version == self.tex_version {
            return Some(b.map); // 纹理已是最新 —— 这次调用只花了纳秒级
        }

        if !dims_ok {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("hipercalc-liquid-bg"),
                size: wgpu::Extent3d {
                    width: b.width,
                    height: b.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                // 与 refraction.rs 一致：抓屏是 sRGB 数据，格式必须带 sRGB，
                // 否则颜色会偏亮（gamma 被当线性处理）
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.texture = Some((texture, view, (b.width, b.height)));
            // ⚠️ 纹理换了，所有槽位的 bind group 还指着旧 view —— 全部作废，
            // 下一帧按需重建（bind group = 80 字节 uniform + 两个引用，重建极快）
            self.slots.clear();
        }

        if let Some((texture, _, _)) = &self.texture {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &pixels[..],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(b.width * 4),
                    rows_per_image: Some(b.height),
                },
                wgpu::Extent3d {
                    width: b.width,
                    height: b.height,
                    depth_or_array_layers: 1,
                },
            );
        }
        self.tex_version = b.version;
        Some(b.map)
    }

    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, key: Key, data: [u8; 96]) {
        if self.slots.len() > MAX_SLOTS {
            self.slots.clear();
        }
        // ⚠️ **uniform 内容没变就不写**：`write_buffer` 每次都有固定的驱动开销，
        // 44 个玻璃件 × 60fps = 每秒 2600 次纯浪费的驱动调用。
        // 槽位里记下上次写过的字节，逐字节相同就跳过 —— 静止桌面/静止鼠标时
        // 绝大多数帧一次都不用写（用户要求 CPU < 0.5%）。
        if let Some((_, _, last)) = self.slots.get(&key)
            && *last == data
        {
            return;
        }
        // ⚠️ 借用拆分：下面的闭包要读 `layout` / 纹理 view / `sampler`，
        // 而 `slots.entry` 可变借用的是另一个字段 —— 逐字段取引用即可过 borrowck。
        let layout = &self.layout;
        let sampler = &self.sampler;
        let bg_view: &wgpu::TextureView = match &self.texture {
            Some((_, v, _)) => v,
            None => &self.fallback.1,
        };
        let (buf, _, last) = self.slots.entry(key).or_insert_with(|| {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("hipercalc-liquid-uniform"),
                // vec2×2 + f32×2 + vec4×3 + vec2×2 + vec4(map) + vec2(edge) = 96 字节（std140 对齐）
                size: 96,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("hipercalc-liquid-bind-group"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(bg_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            });
            (buffer, group, [0xAA; 96])
        });
        queue.write_buffer(buf, 0, &data);
        *last = data;
    }

    fn paint(&self, pass: &mut wgpu::RenderPass<'_>, key: Key) -> bool {
        let Some((_, group, _)) = self.slots.get(&key) else {
            return false;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..3, 0..1);
        true
    }
}

impl shader::Pipeline for GlassPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Self::build(device, format)
    }
}

/// 造一层"液态玻璃"背景，铺满父容器。
///
/// ⚠️ 只负责**外观**（铺在按键文字下面），点击与状态仍由外面的 `button` 管。
pub fn layer<'a, Message>(g: Glass) -> iced::Element<'a, Message>
where
    Message: 'a,
{
    iced::widget::shader(g)
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .into()
}
