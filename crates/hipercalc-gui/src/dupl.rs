//! Windows **桌面复制**（Desktop Duplication）抓屏 —— 取代 GDI 的"屏幕 DC 回读"。
//!
//! ## 为什么必须换
//!
//! GDI 那套（`GetDC(NULL)` + `StretchBlt`）实测 **65–90ms/次**，绝大部分时间花在
//! DWM 把桌面合成到内存里。于是窗口内背景最快只能 2fps，而且每提一档刷新率
//! CPU 就涨 1% 以上 —— 用户要的「背景 60 帧 + CPU < 0.5%」两条一起提，
//! GDI 在数学上就做不到（60fps × 90ms = 每秒 5.4 秒的 CPU）。
//!
//! 桌面复制走的是另一条路：
//!
//! 1. 拿的是 **GPU 侧的桌面帧**（DWM 每帧本来就要合成一份）；
//! 2. 桌面没变化时 `AcquireNextFrame(0)` 立刻超时返回 —— **约 0.1ms**，
//!    这就是"桌面静止时 CPU 接近 0"的来源；
//! 3. 有变化时，把目标区域拷进一张带 mip 链的纹理，**在 GPU 上**用
//!    `GenerateMips` 缩到 1/2 或 1/8，再把那张小图读回 CPU（3–5ms）——
//!    缩小这活儿正是 GDI 那 90ms 里最贵的部分，交给 GPU 后就不值一提了。
//!
//! ## 与上层的关系
//!
//! [`poll`] 三种结果必须分开处理（这是整套设计的关键）：
//!
//! - `Fresh`   —— 有新像素，交给着色器；
//! - `Unchanged` —— 桌面没动，**绝不能**顺手退回 GDI 重抓（那就等于
//!   每 16ms 花 90ms）；上层只需沿用上一张纹理；
//! - `Unsupported` —— 这条路不通（远程桌面 / 受保护内容 / 没有 D3D11），
//!   才退回 GDI。
//!
//! ⚠️ 轮询必须发生在 **Task 的 future 内部**（见 `app::State::bg_cycle`）：
//! 否则"每 16ms 一个 Unchanged"会让 iced 每 16ms 收到一条消息 ⇒ 每 16ms
//! 重建一次 view，CPU 直接爆表。

#![cfg(windows)]
#![allow(clippy::upper_case_acronyms)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
// `cast()`（`IDXGIResource` → `ID3D11Texture2D`、`IDXGIOutput` → `IDXGIOutput1`）
use windows::core::Interface;

/// 抓一帧的结果（三态，缺一不可 —— 见模块头）
pub enum Poll {
    /// 桌面有变化：`rgba` 是 `w × h × 4` 的像素（已转成 RGBA），
    /// `x / y / rw / rh` 是**实际抓到的区域**（可能比请求的小 —— 裁进显示器后）
    Fresh {
        rgba: Vec<u8>,
        w: u32,
        h: u32,
        x: i32,
        y: i32,
        rw: u32,
        rh: u32,
    },
    /// 桌面没变化
    Unchanged,
    /// 这条路走不通，调用方请退回 GDI
    Unsupported,
}

/// 上一帧抓到的像素（给 [`dedup`] 做内容去重）。
///
/// ⚠️ 必须**在轮询循环内部**比（也就是在 runtime 线程上），而不是等消息回到
/// `update` 再比：实测 DWM 每秒会给出约 20 个"新帧"，但其中 100% 的内容与
/// 上一帧完全相同。每来一条消息 iced 就要重建一次 `view()` ⇒ 空闲时界面被
/// 强行重建 20 次/秒 —— 用户反馈的"不拖动时帧率非常低，操作很卡"就是它。
/// 在这里比掉之后：**内容没变就一条消息都不发**，界面彻底静止。
static LAST: Mutex<Vec<u8>> = Mutex::new(Vec::new());

/// 这一帧与上一帧**内容真的一样吗**？一样就返回 `false`（调用方应当继续轮询）。
///
/// 不一样时顺手记下这一帧（下一轮拿它当基准）。
///
/// ⚠️ 为什么这里只需要"整张比较"：抓到的帧里**包含我们自己的窗口**（不再用
/// `WDA_EXCLUDEFROMCAPTURE` 排除自己，那会让用户截不了图），但
/// [`crate::backdrop`] 在返回前已经用**窗口外**的颜色把那一块填平了 ——
/// 于是"我们重绘"不会改变抓到的像素 ⇒ 不会自激。
pub fn dedup(rgba: &[u8]) -> bool {
    let Ok(mut g) = LAST.lock() else { return true };
    if g.len() == rgba.len() && g[..] == rgba[..] {
        return false;
    }
    g.clear();
    g.extend_from_slice(rgba);
    true
}

/// 桌面复制的会话（一个显示器输出一份）
struct Dupl {
    device: ID3D11Device,
    ctx: ID3D11DeviceContext,
    dupl: IDXGIOutputDuplication,
    /// 该输出在虚拟屏幕上的位置（把虚拟坐标换算成输出内坐标）
    ox: i32,
    oy: i32,
    ow: u32,
    oh: u32,
    /// 目标纹理（带 mip 链）+ 它的 SRV + 尺寸 + 格式
    tex: Option<(
        ID3D11Texture2D,
        ID3D11ShaderResourceView,
        u32,
        u32,
        DXGI_FORMAT,
    )>,
    /// 回读用的暂存纹理（GPU 只能读它）+ 尺寸
    stg: Option<(ID3D11Texture2D, u32, u32)>,
    /// 有活儿已经排进 GPU、还没取回来：`(区域宽, 区域高, 区域原点x, 区域原点y)`
    pending: Option<(u32, u32, i32, i32)>,
}

// SAFETY: 只在本模块的锁里访问，跨线程移动由 Mutex 保证
unsafe impl Send for Dupl {}

fn slot() -> &'static Mutex<Option<Dupl>> {
    static S: OnceLock<Mutex<Option<Dupl>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(None))
}

/// 桌面复制已经被证明不可用（远程桌面 / 受保护内容等）：别再反复重试了，
/// 每次初始化都要枚举适配器和建 D3D 设备，很贵。
static BROKEN: AtomicBool = AtomicBool::new(false);

/// 让正在轮询的循环**尽快退出**（质感关掉时用）。
///
/// ⚠️ 没有这个开关的话：桌面静止时轮询 future 可能永远不返回，于是每切一次
/// 质感就永久泄漏一条线程池线程（iced 的线程池只有 CPU 核心数那么多）。
static STOP: AtomicBool = AtomicBool::new(false);

/// 请求停止轮询（见 `STOP`）
pub fn stop() {
    STOP.store(true, Ordering::Relaxed);
}

/// 恢复轮询（质感重新打开时用）
pub fn resume() {
    STOP.store(false, Ordering::Relaxed);
}

/// 是否应当停止轮询
pub fn stopped() -> bool {
    STOP.load(Ordering::Relaxed)
}

fn lock() -> MutexGuard<'static, Option<Dupl>> {
    // 会话丢失时可能有人 panic 过；把毒化的锁就地复用即可（我们只存句柄）
    slot().lock().unwrap_or_else(|e| e.into_inner())
}

/// 桌面复制可用吗？**会顺手初始化**（第一次调用时才真的去连 D3D）。
///
/// 上层用它决定"走轮询循环"还是"走 GDI 的定时抓屏"。
pub fn available() -> bool {
    if BROKEN.load(Ordering::Relaxed) {
        return false;
    }
    let mut g = lock();
    if g.is_some() {
        return true;
    }
    match Dupl::new() {
        Some(d) => {
            *g = Some(d);
            true
        }
        None => {
            BROKEN.store(true, Ordering::Relaxed);
            false
        }
    }
}

/// 抓一帧。
///
/// - `x, y, w, h`：要抓的**虚拟屏幕**区域（会被裁进目标显示器）
/// - `sample`：缩到 1/sample（2 或 8；1 表示不缩）
pub fn poll(x: i32, y: i32, w: u32, h: u32, sample: u32) -> Poll {
    if BROKEN.load(Ordering::Relaxed) {
        return Poll::Unsupported;
    }
    let mut g = lock();
    if g.is_none() {
        match Dupl::new() {
            Some(d) => *g = Some(d),
            None => {
                BROKEN.store(true, Ordering::Relaxed);
                return Poll::Unsupported;
            }
        }
    }
    let Some(d) = g.as_mut() else {
        return Poll::Unsupported;
    };
    match d.grab(x, y, w, h, sample) {
        Ok(Some(f)) => f,
        // 没变化 / 会话丢失：都当作"沿用上一张"。
        // 会话丢失时把会话丢掉 —— 下次轮询会重建（`DXGI_ERROR_ACCESS_LOST`
        // 通常意味着分辨率/显示器变了，重建一次就好）。
        Ok(None) => Poll::Unchanged,
        Err(()) => {
            *g = None;
            Poll::Unchanged
        }
    }
}

/// `sample` 对应第几层 mip（1→0，2→1，4→2，8→3）
fn mip_of(sample: u32) -> u32 {
    (sample.max(1) as f32).log2().round().max(0.0) as u32
}

impl Dupl {
    /// 连接桌面复制：找到**主显示器**所在的输出，建 D3D11 设备，开复制。
    ///
    /// ⚠️ 用「主显示器」而不是"窗口所在显示器"：窗口跨屏时选哪边都不对，
    /// 而主屏是绝大多数场景的答案；跨屏时本模块会裁到主屏那部分（见 [`Dupl::grab`]）。
    fn new() -> Option<Self> {
        unsafe {
            let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
            // 优先选"原点在 (0,0)"且已连接的输出（= 主显示器），
            // 找不到就退回第一块连着的输出。
            let mut primary = None;
            let mut first = None;
            let mut ai = 0u32;
            while let Ok(adapter) = factory.EnumAdapters1(ai) {
                let mut oj = 0u32;
                while let Ok(output) = adapter.EnumOutputs(oj) {
                    if let Ok(desc) = output.GetDesc()
                        && desc.AttachedToDesktop.as_bool()
                    {
                        let is_primary =
                            desc.DesktopCoordinates.left == 0 && desc.DesktopCoordinates.top == 0;
                        if is_primary && primary.is_none() {
                            primary = Some((adapter.clone(), output.clone(), desc));
                        }
                        if first.is_none() {
                            first = Some((adapter.clone(), output, desc));
                        }
                    }
                    oj += 1;
                }
                ai += 1;
            }
            let (adapter, output, desc) = primary.or(first)?;

            let mut device = None;
            let mut ctx = None;
            D3D11CreateDevice(
                &adapter,
                windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut ctx),
            )
            .ok()?;
            let device: ID3D11Device = device?;
            let ctx: ID3D11DeviceContext = ctx?;

            let out1: IDXGIOutput1 = output.cast().ok()?;
            let dupl = out1.DuplicateOutput(&device).ok()?;
            Some(Self {
                device,
                ctx,
                dupl,
                ox: desc.DesktopCoordinates.left,
                oy: desc.DesktopCoordinates.top,
                ow: (desc.DesktopCoordinates.right - desc.DesktopCoordinates.left) as u32,
                oh: (desc.DesktopCoordinates.bottom - desc.DesktopCoordinates.top) as u32,
                tex: None,
                stg: None,
                pending: None,
            })
        }
    }

    /// 抓一帧。**两阶段流水线**（见 [`Dupl::read_pending`] 的注释）：
    /// 先取回上一轮排进 GPU 的结果，再把这一轮的新帧排进去。
    ///
    /// `Ok(None)` = 桌面没变化（或上一帧 GPU 还没画完）；`Err(())` = 会话丢了。
    fn grab(&mut self, x: i32, y: i32, w: u32, h: u32, sample: u32) -> Result<Option<Poll>, ()> {
        let done = self.read_pending()?;
        self.enqueue(x, y, w, h, sample)?;
        Ok(done)
    }

    /// 取回上一轮排进 GPU 的那一帧（**绝不阻塞**）。
    ///
    /// ⚠️ `Map` 默认是同步的（实测 5.3ms —— 整条链最贵的一步），
    /// 改成"拷完立刻读"必然拿到 `WAS_STILL_DRAWING`。所以这里读的是
    /// **上一轮**排进去的活儿：中间隔了一次 16ms 的轮询，GPU 早就画完了，
    /// `Map` 立即返回 —— CPU 从此不被 GPU 流水线按住。
    fn read_pending(&mut self) -> Result<Option<Poll>, ()> {
        let Some((rw, rh, ax, ay)) = self.pending.take() else {
            return Ok(None);
        };
        let Some((stg, dw, dh)) = &self.stg else {
            return Ok(None);
        };
        let mut out = vec![0u8; (dw * dh * 4) as usize];
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            match self.ctx.Map(
                stg,
                0,
                D3D11_MAP_READ,
                D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32,
                Some(&mut mapped),
            ) {
                Ok(()) => {}
                // GPU 还没画完：保留 pending，下一轮再取（不重排，不浪费）
                Err(e) if e.code() == DXGI_ERROR_WAS_STILL_DRAWING => {
                    self.pending = Some((rw, rh, ax, ay));
                    return Ok(None);
                }
                Err(_) => return Err(()),
            }
            let src = mapped.pData as *const u8;
            if !src.is_null() {
                for row in 0..*dh {
                    std::ptr::copy_nonoverlapping(
                        src.add((row * mapped.RowPitch) as usize),
                        out.as_mut_ptr().add((row * dw * 4) as usize),
                        (dw * 4) as usize,
                    );
                }
            }
            self.ctx.Unmap(stg, 0);
        }
        // 桌面复制的帧是 BGRA；本项目其它路径都用 RGBA（见 `grab_scaled`）
        for px in out.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
        Ok(Some(Poll::Fresh {
            rgba: out,
            w: *dw,
            h: *dh,
            x: ax,
            y: ay,
            rw,
            rh,
        }))
    }

    /// 取一帧桌面帧，把"拷区域 → GPU 上缩小"这串活儿排进 GPU 队列。
    fn enqueue(&mut self, x: i32, y: i32, w: u32, h: u32, sample: u32) -> Result<(), ()> {
        // ── ① 裁进这块显示器 ─────────────────────────────────────────────
        // 请求区域（窗口 + 外扩余量）可能越出显示器边缘，也可能跨屏。
        // 越界的部分 `CopySubresourceRegion` 不会给，所以先裁，
        // 并**把实际抓到的矩形记下来** —— uv 映射必须用真实的那一块。
        let lx = (x - self.ox).max(0) as u32;
        let ly = (y - self.oy).max(0) as u32;
        let rx = ((x - self.ox + w as i32).min(self.ow as i32)).max(0) as u32;
        let ry = ((y - self.oy + h as i32).min(self.oh as i32)).max(0) as u32;
        if rx <= lx || ry <= ly {
            return Ok(());
        }
        let cw = (rx - lx).min(self.ow.saturating_sub(lx));
        let ch = (ry - ly).min(self.oh.saturating_sub(ly));
        if cw == 0 || ch == 0 {
            return Ok(());
        }

        // ── ② 取桌面帧（桌面没动 → 立刻超时，约 0.1ms）───────────────────
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut res: Option<IDXGIResource> = None;
        match unsafe { self.dupl.AcquireNextFrame(0, &mut info, &mut res) } {
            Ok(()) => {}
            // 没有新帧 —— 这正是"桌面静止时零开销"的那条路径
            Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(()),
            Err(_) => return Err(()),
        }
        let Some(res) = res else {
            return Err(());
        };
        // 拿不到帧就必须释放，否则下一次 Acquire 永远拿不到新帧
        let guard = ReleaseGuard(&self.dupl);
        let frame: ID3D11Texture2D = res.cast().map_err(|_| ())?;

        // ── ③ 准备纹理（尺寸/格式变了才重建）─────────────────────────────
        let mut fdesc = D3D11_TEXTURE2D_DESC::default();
        unsafe { frame.GetDesc(&mut fdesc) };
        let fmt = fdesc.Format;
        let mip = mip_of(sample);
        let dw = (cw >> mip).max(1);
        let dh = (ch >> mip).max(1);
        if self
            .tex
            .as_ref()
            .is_none_or(|(_, _, w, h, f)| *w != cw || *h != ch || *f != fmt)
        {
            let mut tex = None;
            let mut srv = None;
            unsafe {
                let desc = D3D11_TEXTURE2D_DESC {
                    Width: cw,
                    Height: ch,
                    // 0 = 生成完整 mip 链（GenerateMips 需要它）
                    MipLevels: 0,
                    ArraySize: 1,
                    Format: fmt,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_DEFAULT,
                    // ⚠️ windows-rs 里这三个字段是 `u32`（不是标志类型），
                    // 所以要从常量上取 `.0`。
                    BindFlags: (D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_RENDER_TARGET).0 as u32,
                    CPUAccessFlags: 0,
                    // ⚠️ 没有它 GenerateMips 会直接失败
                    MiscFlags: D3D11_RESOURCE_MISC_GENERATE_MIPS.0 as u32,
                };
                self.device
                    .CreateTexture2D(&desc, None, Some(&mut tex))
                    .map_err(|_| ())?;
                let tex = tex.ok_or(())?;
                // 传 None 的 desc ⇒ 覆盖整条 mip 链（正是我们要的）
                self.device
                    .CreateShaderResourceView(&tex, None, Some(&mut srv))
                    .map_err(|_| ())?;
                let srv = srv.ok_or(())?;
                self.tex = Some((tex, srv, cw, ch, fmt));
            }
        }
        if self
            .stg
            .as_ref()
            .is_none_or(|(_, w, h)| *w != dw || *h != dh)
        {
            let mut stg = None;
            unsafe {
                let desc = D3D11_TEXTURE2D_DESC {
                    Width: dw,
                    Height: dh,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: fmt,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_STAGING,
                    BindFlags: 0,
                    CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                    MiscFlags: 0,
                };
                self.device
                    .CreateTexture2D(&desc, None, Some(&mut stg))
                    .map_err(|_| ())?;
            }
            self.stg = Some((stg.ok_or(())?, dw, dh));
        }
        let (tex, srv, _, _, _) = self.tex.as_ref().ok_or(())?;
        let (stg, _, _) = self.stg.as_ref().ok_or(())?;

        // ── ④ 拷 → GPU 上缩小（异步，不等）───────────────────────────────
        unsafe {
            let b = D3D11_BOX {
                left: lx,
                top: ly,
                front: 0,
                right: lx + cw,
                bottom: ly + ch,
                back: 1,
            };
            self.ctx
                .CopySubresourceRegion(tex, 0, 0, 0, 0, &frame, 0, Some(&b));
            // 缩小的活儿全在这里（GPU 侧），代价与 GDI 的 StretchBlt 不在一个量级
            self.ctx.GenerateMips(srv);
            self.ctx
                .CopySubresourceRegion(stg, 0, 0, 0, 0, tex, mip, None);
        }
        drop(guard);
        // 一轮只允许有一份待取的结果：还没取走的就作废（反正它已经旧了）
        self.pending = Some((cw, ch, self.ox + lx as i32, self.oy + ly as i32));
        Ok(())
    }
}

/// 保证 `ReleaseFrame` 一定被调用（哪怕中间 `?` 提前返回）。
///
/// 不释放的话，桌面复制会卡在同一帧上 —— 之后每次 `AcquireNextFrame` 都超时，
/// 背景就再也不刷新了。
struct ReleaseGuard<'a>(&'a IDXGIOutputDuplication);

impl Drop for ReleaseGuard<'_> {
    fn drop(&mut self) {
        unsafe {
            let _ = self.0.ReleaseFrame();
        }
    }
}
