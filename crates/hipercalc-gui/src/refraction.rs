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
//! 4. WGSL 里按"到中心的距离"偏移 UV —— 中心几乎不动、边缘明显偏移，
//!    这就是凸面玻璃的成像特征。
//!
//! ## 为什么不是真的曲面折射
//!
//! 真折射要追光线（Snell 定律），成本高且参数难调。这里用的是**视觉近似**：
//! `偏移量 ∝ pow(半径, 3)`，中心平坦、边缘陡峭 —— 人眼看上去就是一块
//! 有厚度的玻璃，而实现只有一行公式。

use iced::wgpu;
use iced::widget::shader;
use iced::{Rectangle, mouse};

/// 折射玻璃：把一张 RGBA 背景图按"凸面玻璃"扭曲后绘制。
///
/// `pixels` 是**已经模糊过**的背景（抓屏 + 降采样 + box blur 的产物），
/// 折射只负责"弯"，模糊由 [`crate::backdrop`] 负责 —— 两件事分开，各自可调。
#[derive(Debug, Clone)]
pub struct Refraction {
    /// RGBA8 像素，长度必须是 `width * height * 4`
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// 扭曲强度：0 = 完全不弯（退化成普通背景图），1 = 最明显
    pub strength: f32,
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
            pixels: self.pixels.clone(),
            width: self.width,
            height: self.height,
            strength: self.strength,
        }
    }
}

/// 交给 GPU 的那一份数据（自带像素，不借用 `Refraction`）
#[derive(Debug)]
pub struct RefractionPrimitive {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    strength: f32,
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
}

const WGSL: &str = r#"
struct Uniforms {
    // 目标区域尺寸（逻辑像素）—— 目前只用于让偏移量跟尺寸成比例，
    // 真正的比例常数写在下面的 `0.08` 里。
    size: vec2<f32>,
    strength: f32,
    _pad: f32,
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
    // 中心为 0、边缘为 1 的径向坐标
    let c = in.uv * 2.0 - vec2<f32>(1.0);
    let r = length(c);
    // 折射的核心：偏移量 ∝ r³。
    // 一次方会让整块均匀偏移（像平移），平方会让中间也明显变形；
    // 三次方让**中心几乎不动、边缘陡增** —— 这正是凸透镜的成像特征。
    let k = pow(r, 3.0) * u.strength * 0.08;
    // 沿径向向外偏移（采样更外面一点的内容 = 图像被"压"向边缘）
    let uv = clamp(in.uv + c * k, vec2<f32>(0.002), vec2<f32>(0.998));
    return textureSample(bg, samp, uv);
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
            // vec2<f32> + f32 + f32 = 16 字节（std140 对齐要求）
            size: 16,
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

        // uniform：size.xy / strength / pad
        let mut data = [0u8; 16];
        data[0..4].copy_from_slice(&size.width.to_le_bytes());
        data[4..8].copy_from_slice(&size.height.to_le_bytes());
        data[8..12].copy_from_slice(&prim.strength.clamp(0.0, 1.0).to_le_bytes());
        queue.write_buffer(&self.uniform, 0, &data);

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
