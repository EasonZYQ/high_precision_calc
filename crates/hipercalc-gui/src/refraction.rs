//! 折射玻璃 —— iOS liquid glass 的"折射扭曲"。
//!
//! ## 为什么需要 shader
//!
//! 亚克力（磨砂）只需要"背景模糊 + 半透明底色"，用 `image` 叠在面板下面就够了。
//! 而 iOS 的玻璃有**折射**：透过它看到的东西在边缘会被"压弯"，像一块凸透镜。
//! 这不是任何 widget 样式能做到的 —— 必须逐像素重采样背景图。
//!
//! ## 实现路线
//!
//! 用 `iced::widget::shader`（底层就是 wgpu）：
//! 1. [`RefractionPipeline::new`]：建渲染管线 + 采样器 + 绑定布局；
//! 2. [`RefractionPrimitive::prepare`]：把抓屏得到的 RGBA 上传成纹理，
//!    并把"扭曲强度"写进 uniform；
//! 3. [`RefractionPrimitive::draw`]：画一个**全屏三角形**（三个顶点盖满整个
//!    bounds，比分两个三角形少一次顶点）；
//! 4. WGSL 里按"到最近窗口边缘的距离"加权偏移 UV —— **中间一大块不动**，
//!    只有靠边的环带（短边的 25%）逐渐弯曲，像一块有厚度的玻璃。
//!
//! ## 为什么不是真的曲面折射
//!
//! 真折射要追光线（Snell 定律），成本高且参数难调。这里用的是**视觉近似**：
//! 偏移量 = 「边缘权重 × 强度」，中心平坦、贴边陡峭 —— 人眼看上去就是一块
//! 有厚度的玻璃，而实现只有几行公式。

use iced::wgpu;
use iced::widget::shader;
use iced::{Rectangle, mouse};
use std::sync::Arc;

/// 折射玻璃：把一张 RGBA 背景图按"凸面玻璃"扭曲后绘制。
///
/// `pixels` 是**已经模糊过**的背景（抓屏 + 降采样 + box blur 的产物），
/// 折射只负责"弯"，模糊由 [`crate::backdrop`] 负责 —— 两件事分开，各自可调。
///
/// ## ⚠️ 为什么 `pixels` 是 `Arc<Vec<u8>>` 而不是 `Vec<u8>`
///
/// `draw()` **每次重绘都会被调用**，如果里面 `self.pixels.clone()`，
/// 800KB 的图就被 memcpy 一遍 —— 60fps 下是 48MB/s 的纯内存带宽浪费。
/// 用户实测玻璃模式 CPU 10–30%，这是主因之一。`Arc` 只加一次引用计数。
#[derive(Debug, Clone)]
pub struct Refraction {
    /// RGBA8 像素，长度必须是 `width * height * 4`
    pub pixels: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    /// 扭曲强度：0 = 完全不弯（退化成普通背景图），1 = 最明显
    pub strength: f32,
    /// **像素版本号**：每次抓到新背景时 +1。
    ///
    /// `prepare()` 每帧都会被调用，而 60fps 下重传 800KB 纹理是纯浪费。
    /// 版本号没变就只更新 uniform，跳过 `write_texture`。
    pub version: u64,
    /// 背景→窗口的 uv 映射：`图像uv = 窗口uv × uv_scale + uv_off`（y 向下）。
    ///
    /// 由 `app::view` 每帧从「抓屏原点 vs 当前窗口原点」算出来
    ///（`backdrop::uv_map`）—— 窗口挪动时它跟着变，背景就"跟手"了，
    /// 拖动全程不需要重抓屏（用户要求拖动 60fps、CPU < 1%）。
    pub uv_scale: [f32; 2],
    pub uv_off: [f32; 2],
}

impl<Message> shader::Program<Message> for Refraction {
    type State = ();
    type Primitive = RefractionPrimitive;

    fn draw(
        &self,
        _state: &Self::State,
        _cursor: mouse::Cursor,
        _bounds: Rectangle,
    ) -> Self::Primitive {
        RefractionPrimitive {
            // ⚠️ `Arc::clone` 只加引用计数（纳秒级），不复制像素
            pixels: Arc::clone(&self.pixels),
            width: self.width,
            height: self.height,
            strength: self.strength,
            version: self.version,
            uv_scale: self.uv_scale,
            uv_off: self.uv_off,
        }
    }
}

/// 交给 GPU 的那一份数据（共享像素，不复制）
#[derive(Debug)]
pub struct RefractionPrimitive {
    pixels: Arc<Vec<u8>>,
    width: u32,
    height: u32,
    strength: f32,
    version: u64,
    uv_scale: [f32; 2],
    uv_off: [f32; 2],
}

impl shader::Primitive for RefractionPrimitive {
    type Pipeline = RefractionPipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        _viewport: &shader::Viewport,
    ) {
        pipeline.upload(device, queue, self, bounds.size());
    }

    fn draw(&self, pipeline: &Self::Pipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        pipeline.paint(render_pass)
    }
}

/// 所有 [`RefractionPrimitive`] 共用的 GPU 资源（iced 保证只 `new` 一次）
pub struct RefractionPipeline {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// 背景纹理（尺寸随抓屏结果变化，变化时重建）
    texture: Option<(wgpu::Texture, wgpu::TextureView, (u32, u32))>,
    bind_group: Option<wgpu::BindGroup>,
    /// uniform：`size.xy` + `strength` + padding，共 16 字节
    uniform: wgpu::Buffer,
    /// 上一次上传的像素版本号（相同就跳过 `write_texture`）
    last_version: u64,
}

const WGSL: &str = r#"
struct Uniforms {
    // 目标区域尺寸（逻辑像素）—— 边缘环带宽度按它算
    size: vec2<f32>,
    strength: f32,
    _pad: f32,
    // 背景图映射：图像uv = 窗口uv × map.xy + map.zw
    //（窗口挪动时按原点差平移采样 —— 拖动全程零抓屏、零 CPU）
    map: vec4<f32>,
};

@group(0) @binding(0) var bg: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> u: Uniforms;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// 全屏三角形：三个顶点盖满整个 bounds。
// 为什么不用两个三角形（四边形）：多一组顶点、多一次三角形设置，没好处。
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
    // NDC [-1,1] → UV [0,1]
    o.uv = (q + vec2<f32>(1.0)) * 0.5;
    return o;
}

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
    // ⚠️ **先把 uv 转成 y 向下的屏幕坐标**。`in.uv` 来自 NDC（y 向上：
    // uv.y = 1 是顶边），而抓屏的行序是 top-down（第 0 行 = 屏幕最上面，
    // 对应纹理 v = 0）。直接拿 in.uv 当 v 采样就会**上下颠倒**
    //（用户实测 bug：两种质感的背景都是倒的）。转成 y 向下之后
    // v = s.y 与抓屏行序天然一致，两层 shader 都按这个约定来。
    let s = vec2<f32>(in.uv.x, 1.0 - in.uv.y);

    // ── 边缘环带扭曲（用户规格：中间一大块不动，只有靠窗口边缘才扭曲）─────
    //
    // 旧版是纯径向公式 `偏移 ∝ r³`：全窗口都有形变，实测观感是"整个背景
    // 都被拧过"。现在改成**按"到最近窗口边缘的距离"加权**：
    // - `d` = 到最近边的距离（像素，取四面里最近的那个）
    // - 环带宽度 = 短边的 25%（用户拍板）：环带内扭曲线性增强，出环带严格为 0
    // - `pow(.., 1.6)` 让贴边处弯得最狠、环带外沿平滑收零 —— 模仿按键边缘
    //   那种"玻璃有厚度"的观感
    let dx = min(s.x, 1.0 - s.x) * u.size.x;
    let dy = min(s.y, 1.0 - s.y) * u.size.y;
    // 环带从 25% 收到 18%：同样的弯折量集中在更窄的一圈里，边缘才有"厚度"
    let band = 0.18 * min(u.size.x, u.size.y);
    let dmin = min(dx, dy);

    // ⚠️ 剖面：**两段指数**，不是一条平滑隆起。
    //
    // 用户反馈："边缘完全是简单的扭曲，而不是 iOS 的折射"。之前的
    // `pow(1-smoothstep, 1.6)` 是一条宽而均匀的隆起 —— 观感是"整块被轻轻推了一下"。
    // 真实玻璃的边缘是**一条很陡的壁 + 内侧一条缓坡**：
    // - `wall`（特征宽度 ≈ 6% 环带）：贴边 1~3px 内的强弯折，这一条决定"像不像玻璃"；
    // - `bevel`（≈ 75% 环带）：把桌面内容缓慢压向边缘，给出"厚度"的错觉；
    // - `cut`：到环带外沿**严格归零**，否则会看到一条突兀的接缝。
    let wall = exp(-dmin / max(0.06 * band, 1.0));
    let bevel = exp(-dmin / max(0.75 * band, 1.0));
    let cut = 1.0 - smoothstep(0.55 * band, band, dmin);
    let w = (0.70 * wall + 0.30 * bevel) * cut;
    // 偏移方向仍用径向：在边缘附近它近似垂直于边缘（= 玻璃厚度的法线），
    // 在角上则指向对角 —— 与按键拐角的弯法一致。
    // `+ 1e-5` 防 `normalize(0)`：中心处 w 已是 0，方向随便给个非零向量即可。
    let c = s * 2.0 - vec2<f32>(1.0);
    let n = normalize(c + vec2<f32>(1e-5));
    // 峰值偏移（× 滑条强度）。中心 0，贴边最弯。
    let k = w * u.strength * 0.11;
    // ── 窗口→背景图映射：窗口挪动时按原点差平移采样（拖动零抓屏）────────
    // 抓屏区域带外扩余量（见 backdrop::margin），余量内的偏移都落在
    // 真实像素上 —— 不再 clamp 到抓屏边缘（那会把最外一行拉成高饱和色块）。
    var f = s * u.map.xy + u.map.zw + n * k;
    f = clamp(f, vec2<f32>(0.001), vec2<f32>(0.999));
    return textureSample(bg, samp, f);
}
"#;

impl RefractionPipeline {
    /// 建管线 / 采样器 / 绑定布局（iced 保证只调用一次）
    fn build(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("hipercalc-refraction"),
            source: wgpu::ShaderSource::Wgsl(WGSL.into()),
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hipercalc-refraction-layout"),
            entries: &[
                // 背景纹理
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
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
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // uniform
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("hipercalc-refraction-pipeline-layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("hipercalc-refraction-pipeline"),
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
                    // 背景图是不透明的，但用 alpha blending 更保险：
                    // 万一将来想让它半透，不用再改管线。
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
            label: Some("hipercalc-refraction-sampler"),
            // 边缘夹取：UV 被 clamp 到 0.002..0.998，夹取模式只是最后一道保险
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hipercalc-refraction-uniform"),
            // vec2 + f32 + f32 + vec4 = 32 字节（std140 对齐要求）
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            layout,
            sampler,
            texture: None,
            bind_group: None,
            uniform,
            last_version: u64::MAX, // 保证第一次一定会传
        }
    }

    /// 上传像素 + 写 uniform + （必要时）重建 bind group
    fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        prim: &RefractionPrimitive,
        size: iced::Size,
    ) {
        if prim.width == 0 || prim.height == 0 {
            return;
        }
        let expected = (prim.width as usize) * (prim.height as usize) * 4;
        if prim.pixels.len() < expected {
            // 像素长度对不上就什么都不画（宁可空着，也不要读到越界数据）
            return;
        }

        // ── ① uniform **每帧都写**：`strength` 可能被拖滑块改、映射随窗口位置变 ──
        let mut data = [0u8; 32];
        data[0..4].copy_from_slice(&size.width.to_le_bytes());
        data[4..8].copy_from_slice(&size.height.to_le_bytes());
        data[8..12].copy_from_slice(&prim.strength.clamp(0.0, 1.0).to_le_bytes());
        data[16..20].copy_from_slice(&prim.uv_scale[0].to_le_bytes());
        data[20..24].copy_from_slice(&prim.uv_scale[1].to_le_bytes());
        data[24..28].copy_from_slice(&prim.uv_off[0].to_le_bytes());
        data[28..32].copy_from_slice(&prim.uv_off[1].to_le_bytes());
        queue.write_buffer(&self.uniform, 0, &data);

        // ── ② 像素**没变就到此为止** ──────────────────────────────────────
        //
        // 用户实测玻璃模式 CPU 10–30%。`prepare()` 是每帧调用的，而抓屏 500ms
        // 才来一帧新数据 —— 中间那 29 帧全在做 800KB 的 `write_texture`，
        // 纯浪费。版本号一比对就能跳过。
        if prim.version == self.last_version && self.bind_group.is_some() {
            return;
        }

        // 纹理尺寸变了才重建（抓屏的降采样倍数可能变）
        let need_new = match &self.texture {
            Some((_, _, (w, h))) => *w != prim.width || *h != prim.height,
            None => true,
        };
        if need_new {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("hipercalc-refraction-bg"),
                size: wgpu::Extent3d {
                    width: prim.width,
                    height: prim.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                // 抓屏拿到的是 sRGB 数据，这里必须用 sRGB 格式，
                // 否则颜色会偏亮（gamma 被当线性处理）
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.texture = Some((texture, view, (prim.width, prim.height)));
            // 纹理换了，bind group 必须跟着换
            self.bind_group = None;
        }

        if let Some((texture, _, _)) = &self.texture {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &prim.pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(prim.width * 4),
                    rows_per_image: Some(prim.height),
                },
                wgpu::Extent3d {
                    width: prim.width,
                    height: prim.height,
                    depth_or_array_layers: 1,
                },
            );
        }

        // uniform 已经在函数开头写过了（见 ①）—— 这里只补 bind group

        if self.bind_group.is_none()
            && let Some((_, view, _)) = &self.texture
        {
            self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("hipercalc-refraction-bind-group"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: self.uniform.as_entire_binding(),
                    },
                ],
            }));
        }

        // 记下这次传的是哪一版像素 —— 下一帧就能靠它跳过重传
        self.last_version = prim.version;
    }

    /// 画全屏三角形
    fn paint(&self, pass: &mut wgpu::RenderPass<'_>) -> bool {
        let Some(bind_group) = &self.bind_group else {
            return false;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..3, 0..1);
        true
    }
}

impl shader::Pipeline for RefractionPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Self::build(device, format)
    }
}
