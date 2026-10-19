//! 桌面模糊（亚克力 / Mica）—— 跨平台门面，Windows 走手写 Win32 FFI。
//!
//! ## 为什么必须自己写
//!
//! `iced::window::Settings::blur` 在 Windows 上**不起作用**。`iced_core-0.14.0`
//! 的 `window/settings.rs` 原文：*"This option is only supported on macOS and Linux."*
//! 想在 Windows 上做出 WinUI3 那种"透出桌面且模糊"的效果，只能调 DWM。
//!
//! ## 怎么拿到窗口句柄
//!
//! iced 0.14 **不向应用暴露原生窗口句柄**（`HasWindowHandle` 只在内部类型上实现）。
//! 所以这里用 **`EnumWindows` 遍历顶层窗口 + `GetWindowThreadProcessId` 匹配当前进程 ID**
//! 反查 —— 对"本进程只开一个主窗口"的 GUI 来说这是可靠且最简单的办法。
//!
//! ## 逐级降级（任一成功即停）
//!
//! 1. **Win11 22H2+（build ≥ 22621）**：`DWMWA_SYSTEMBACKDROP_TYPE = 38`，值 `3`(Acrylic)
//!    或 `2`(Mica)
//! 2. **Win11 21H2**：`DWMWA_MICA_EFFECT = 1029` 置 `1`
//! 3. **Win10 1803+**：未公开的 `SetWindowCompositionAttribute` +
//!    `ACCENT_ENABLE_ACRYLICBLURBEHIND = 4`
//! 4. **全部失败** ⇒ 返回 `false`，GUI 退回不透明背景
//!
//! 附带把**标题栏也染成深色**（`DWMWA_USE_IMMERSIVE_DARK_MODE = 20`）并给窗口加圆角
//! （`DWMWA_WINDOW_CORNER_PREFERENCE = 33`），这两项在 Win10/11 上分别生效。
//!
//! ## 诚实说明
//!
//! **远程桌面 / 无头环境下 DWM 的模糊通常不生效**（组成方式不同）。探测失败时
//! 本模块如实返回 `false`，界面走不透明深色 —— 观感依然是 WinUI3 风格，
//! 只是没有桌面透出。绝不假装成功。

/// 探测并启用桌面模糊。返回是否真的启用成功。
///
/// 调用时机：窗口**已经创建**之后（`window::Event::Opened`）。太早调用找不到窗口。
pub fn enable(dark_titlebar: bool) -> bool {
    imp::enable(dark_titlebar)
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;

    type Handle = *mut c_void;
    type Hwnd = *mut c_void;
    type Bool = i32;

    const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
    const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    /// Win11 22H2 起可用：系统背景材质
    const DWMWA_SYSTEMBACKDROP_TYPE: u32 = 38;
    /// Win11 21H2 的 Mica（后续被 38 取代）
    const DWMWA_MICA_EFFECT: u32 = 1029;

    /// `DWMSBT_*`
    const DWMSBT_MAINWINDOW: i32 = 2; // Mica
    const DWMSBT_TRANSIENTWINDOW: i32 = 3; // Acrylic
    /// `DWMWCP_ROUND`
    const DWMWCP_ROUND: i32 = 2;

    /// `SetWindowCompositionAttribute` 的 `AccentState`
    const ACCENT_ENABLE_ACRYLICBLURBEHIND: i32 = 4;
    const ACCENT_ENABLE_BLURBEHIND: i32 = 3;

    #[repr(C)]
    struct AccentPolicy {
        accent_state: i32,
        accent_flags: i32,
        gradient_color: u32,
        animation_id: i32,
    }

    #[repr(C)]
    struct WindowCompositionAttributeData {
        attribute: i32,
        data: *mut c_void,
        size_of_data: usize,
    }

    #[link(name = "dwmapi")]
    unsafe extern "system" {
        fn DwmSetWindowAttribute(hwnd: Hwnd, attr: u32, value: *const c_void, size: u32) -> i32;
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn EnumWindows(cb: extern "system" fn(Hwnd, isize) -> Bool, lparam: isize) -> Bool;
        fn GetWindowThreadProcessId(hwnd: Hwnd, pid: *mut u32) -> u32;
        fn IsWindowVisible(hwnd: Hwnd) -> Bool;
        fn GetWindow(hwnd: Hwnd, cmd: u32) -> Hwnd;
        fn GetWindowLongW(hwnd: Hwnd, index: i32) -> i32;
        fn GetModuleHandleW(name: *const u16) -> Handle;
        fn GetProcAddress(module: Handle, name: *const u8) -> *mut c_void;
    }

    /// `GW_OWNER`
    const GW_OWNER: u32 = 4;
    /// `GWL_EXSTYLE`
    const GWL_EXSTYLE: i32 = -20;
    /// `WS_EX_TOOLWINDOW`
    const WS_EX_TOOLWINDOW: i32 = 0x0000_0080;

    /// 遍历状态：把当前 PID 与找到的窗口塞进去
    struct Search {
        pid: u32,
        found: Hwnd,
    }

    extern "system" fn enum_proc(hwnd: Hwnd, lparam: isize) -> Bool {
        // SAFETY: lparam 由 `find_main_window` 传入，指向栈上的 `Search`，存活于整次遍历
        let search = unsafe { &mut *(lparam as *mut Search) };
        unsafe {
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid != search.pid {
                return 1; // 继续找
            }
            // 排除工具窗口与有 owner 的窗口（消息框、任务栏按钮之类），
            // 只认"顶层、可见、无 owner、非 toolwindow"的那个 —— 那就是主窗口
            if IsWindowVisible(hwnd) == 0 {
                return 1;
            }
            if !GetWindow(hwnd, GW_OWNER).is_null() {
                return 1;
            }
            if GetWindowLongW(hwnd, GWL_EXSTYLE) & WS_EX_TOOLWINDOW != 0 {
                return 1;
            }
            search.found = hwnd;
            0 // 找到了，停止遍历
        }
    }

    /// 反查本进程的主窗口句柄。
    ///
    /// `pub(super)`：下面的抓屏实现也要用它（抓之前要把自己排除在捕获之外，
    /// 否则抓到的会是自己）。
    ///
    /// ## ⚠️ 结果**必须缓存**
    ///
    /// `EnumWindows` 是**全系统**的窗口遍历，实测一次 0.2–2.3ms。而
    /// `capture_window_blurred` 一轮里会调它 **两遍**（`window_rect` 一次、
    /// `grab_scaled` 一次）—— 抓屏本来就 40–95ms，再白扔几毫秒纯属浪费。
    /// 主窗口句柄建好之后**不会再变**（本进程只开这一个顶层窗口），
    /// 所以第一次找到就存下来，之后直接读。
    ///
    /// 缓存**失败结果**（`null`）会永久卡死，所以只有成功才写进缓存。
    pub(super) fn find_main_window() -> Option<Hwnd> {
        let cached = HWND_CACHE.load(std::sync::atomic::Ordering::Relaxed);
        if cached != 0 {
            return Some(cached as Hwnd);
        }
        let r = find_main_window_inner();
        if let Some(h) = r {
            HWND_CACHE.store(h as isize, std::sync::atomic::Ordering::Relaxed);
        }
        r
    }

    /// 主窗口句柄缓存（`0` = 还没找到）
    static HWND_CACHE: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

    fn find_main_window_inner() -> Option<Hwnd> {
        let pid = std::process::id();
        let mut search = Search {
            pid,
            found: std::ptr::null_mut(),
        };
        unsafe {
            EnumWindows(enum_proc, &mut search as *mut Search as isize);
        }
        if search.found.is_null() {
            None
        } else {
            Some(search.found)
        }
    }

    /// 设置一个 DWM 属性（用 i32 值），返回是否成功
    unsafe fn set_dwm_i32(hwnd: Hwnd, attr: u32, value: i32) -> bool {
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                attr,
                &value as *const i32 as *const c_void,
                std::mem::size_of::<i32>() as u32,
            ) == 0
        }
    }

    /// 调未公开的 `SetWindowCompositionAttribute`（Win10 的亚克力走这条路）
    unsafe fn set_accent(hwnd: Hwnd, state: i32, gradient_abgr: u32) -> bool {
        unsafe {
            let module = GetModuleHandleW(w!("user32.dll").as_ptr());
            if module.is_null() {
                return false;
            }
            let name = b"SetWindowCompositionAttribute\0";
            let proc = GetProcAddress(module, name.as_ptr());
            if proc.is_null() {
                return false;
            }
            type Fn = extern "system" fn(Hwnd, *mut WindowCompositionAttributeData) -> Bool;
            let f: Fn = std::mem::transmute(proc);

            let mut policy = AccentPolicy {
                accent_state: state,
                accent_flags: 2, // 0x2 = 让模糊覆盖整个窗口
                gradient_color: gradient_abgr,
                animation_id: 0,
            };
            let mut data = WindowCompositionAttributeData {
                attribute: 19, // WCA_ACCENT_POLICY
                data: &mut policy as *mut AccentPolicy as *mut c_void,
                size_of_data: std::mem::size_of::<AccentPolicy>(),
            };
            f(hwnd, &mut data) != 0
        }
    }

    /// 把 `wide string` 字面量转成以 NUL 结尾的 u16 数组
    macro_rules! w {
        ($s:literal) => {{
            const BYTES: &[u8] = concat!($s, "\0").as_bytes();
            const LEN: usize = BYTES.len();
            let mut buf = [0u16; LEN];
            let mut i = 0;
            while i < LEN {
                buf[i] = BYTES[i] as u16;
                i += 1;
            }
            buf
        }};
    }
    use w;

    /// 探测并启用桌面模糊
    pub fn enable(dark_titlebar: bool) -> bool {
        let Some(hwnd) = find_main_window() else {
            return false;
        };
        unsafe {
            // 深色标题栏与窗口圆角：与材质无关，Win10/11 各自支持，尽量都设上
            if dark_titlebar {
                let _ = set_dwm_i32(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, 1);
            }
            let _ = set_dwm_i32(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND);

            // 逐级尝试材质，任一成功即返回 true
            if set_dwm_i32(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, DWMSBT_TRANSIENTWINDOW) {
                return true;
            }
            if set_dwm_i32(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, DWMSBT_MAINWINDOW) {
                return true;
            }
            if set_dwm_i32(hwnd, DWMWA_MICA_EFFECT, 1) {
                return true;
            }
            // Win10：亚克力（半透明 + 模糊），失败再退纯模糊
            // gradient_color 是 ABGR，这里给"接近透明的深色染色"
            if set_accent(hwnd, ACCENT_ENABLE_ACRYLICBLURBEHIND, 0x99_20_1A_18) {
                return true;
            }
            if set_accent(hwnd, ACCENT_ENABLE_BLURBEHIND, 0) {
                return true;
            }
            false
        }
    }
}

/// 探测并启用桌面模糊（**异步版**：在 runtime 线程里跑，给窗口一点时间出现在
/// `EnumWindows` 的列表里，再反查句柄）。
///
/// ## 为什么不挂在 `window::Event::Opened` 上
///
/// 实测那一版**根本没生效**：`window::events()` 订阅从没发出过 `Opened` ——
/// 控制台里连 `[hipercalc-gui]` 的报告都没有。于是亚克力从头到尾没被尝试，
/// 界面永远停在"氛围底"那档。
///
/// 用启动 Task 延迟 150ms 再探测，可靠性高得多：那时窗口必然已在任务列表里。
pub async fn probe(dark_titlebar: bool) -> bool {
    // 150ms 是实测出的下限：太短（<50ms）时窗口还没进 EnumWindows 的列表，
    // 反查会空手而归。UI 线程完全不受影响 —— 这个 future 跑在 runtime 的
    // 线程池里，睡一个 worker 150ms 无妨（iced 默认是多线程调度器）。
    // ⚠️ 不用 `tokio::time::sleep`：gui crate 没直接依赖 tokio，不想为这一处
    // 把它引进来；`std::thread::sleep` 在 async 块里同样成立。
    #[cfg(windows)]
    std::thread::sleep(std::time::Duration::from_millis(150));
    #[cfg(not(windows))]
    let _ = dark_titlebar;
    #[allow(unreachable_code)]
    imp::enable(dark_titlebar)
}

#[cfg(not(windows))]
mod imp {
    /// 非 Windows 平台：iced 自身的 `window::Settings::blur` 已覆盖 macOS/Linux，
    /// 这里不需要做额外的事。返回 `false` 让 GUI 用不透明背景（功能性不受影响）。
    pub fn enable(_dark_titlebar: bool) -> bool {
        false
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 应用内自绘模糊（"真的能看到桌面"）
// ─────────────────────────────────────────────────────────────────────────────
//
// ## 为什么不能靠 DWM
//
// 实测结论：`DWMWA_SYSTEMBACKDROP_TYPE = 3`（Acrylic）**返回成功**
//（控制台会打印"已启用"），但窗口仍然完全看不到桌面。
// 原因是架构冲突：iced 在**整个客户区**用 wgpu 自绘，DWM 的材质铺在它**之下**，
// 被完整盖住。这不是配置问题，调 API 也解决不了。
//
// ## 所以自己来
//
// 抓窗口所在的那块屏幕 → 降采样 → 模糊 → 当成根背景画出来。
// 视觉上等价于"亚克力"，代价是要自己管刷新时机。

/// 一张可以直接当背景用的图（RGBA8）
#[derive(Debug, Clone)]
pub struct Backdrop {
    /// 抓屏区域**左上角在屏幕上的位置**（物理像素）。
    ///
    /// 抓屏区域比窗口**大一圈**（见 [`capture_window_blurred`]），所以窗口
    /// 挪动时不必立刻重抓 —— 着色器按 `(当前窗口原点 - 抓屏原点)` 平移采样，
    /// 拖动全程背景都是几何正确的，一滴 CPU 都不花（用户要求拖动 60fps）。
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` 字节，顺序是 R,G,B,A
    pub rgba: Vec<u8>,
}

/// 抓屏区域向外扩多少：**短边的一半**。
///
/// 抓大一点都不增加成本 —— `StretchBlt` 的开销主要在目标尺寸（降采样后很小），
/// 源矩形大一些只是多读点屏。换来的是：
///
/// 1. 拖动窗口时，偏移采样在半个窗口范围内都落在真实像素上（而不是 clamp 拖影）；
/// 2. 玻璃的边缘环带扭曲往外采样时采到的也是真实桌面 ——
///    之前 clamp 到抓屏边缘会把最外一行像素拉成**高饱和色块**
///    （用户反馈"背景饱和度比桌面高"主要就是这些色块）。
pub fn margin(w: u32, h: u32) -> i32 {
    (w.min(h) / 2).max(1) as i32
}

/// 当前窗口 → 背景图的 uv 映射：`图像uv = 窗口uv × scale + offset`。
///
/// 坐标系是 **y 向下**的屏幕坐标（与抓屏行序一致）—— 着色器里必须先把
/// `in.uv`（wgpu 的 NDC，y 向上）转成 y 向下再用这个映射，否则背景上下颠倒
///（用户实测 bug：两种质感的背景都是倒的）。
///
/// - `cap` = 抓屏区域 `(x, y, w, h)`（屏幕物理像素，存在 [`Backdrop::x`] 里）
/// - 拿不到当前窗口位置（非 Windows / 句柄丢失）时返回 `None`，
///   调用方退回恒等映射（不偏移，仅首次布局时短暂出现）。
pub fn uv_map(cap: (i32, i32, u32, u32)) -> Option<([f32; 2], [f32; 2])> {
    let (wx, wy, ww, wh) = platform::window_rect()?;
    let sw = cap.2 as f32;
    let sh = cap.3 as f32;
    if sw <= 0.0 || sh <= 0.0 {
        return None;
    }
    Some((
        [ww as f32 / sw, wh as f32 / sh],
        [(wx - cap.0) as f32 / sw, (wy - cap.1) as f32 / sh],
    ))
}

/// 当前主窗口的屏幕矩形（转发 [`platform::window_rect`]，给拖动检测用）。
pub fn window_rect() -> Option<(i32, i32, u32, u32)> {
    platform::window_rect()
}

/// 抓屏幕上一块区域，降采样 `factor` 倍后再模糊。
///
/// - `factor` 越大越模糊、也越省算力：**降采样本身就是最强的低通滤波**，
///   先缩小再放大天然丢掉高频细节。8 是实测够用的值
///   （940×860 的窗口 → 118×108，后续模糊几乎不要钱）。
/// - 抓屏前会把自己的窗口**临时**排除在捕获之外（`WDA_EXCLUDEFROMCAPTURE`），
///   否则抓到的会是自己（半透明界面与桌面混在一起 ⇒ 重影）。
///   抓完立刻恢复，所以用户截图/录屏里窗口**照常出现**。
pub fn capture_blurred(x: i32, y: i32, w: u32, h: u32, factor: u32) -> Option<Backdrop> {
    if w == 0 || h == 0 {
        return None;
    }
    // ⚠️ **让 GDI 在拷贝时就缩小**（`StretchBlt`），不要"抓全分辨率再在 CPU 上缩"。
    //
    // 用户实测：之前抓 940×860 的全分辨率（3.2MB），再在 160 万像素上做 box blur，
    // **CPU 直接 47%**，界面延迟 5 秒以上（事件循环被饿死，连滑块都拖不动）。
    // 改成缩放到 1/factor 再抓，数据量降 f² 倍 —— f=8 时只剩 1/64。
    let f = factor.max(1);
    // **`f == 1` 表示"不模糊"**（用户要求："模糊强度为 0 时不是不模糊，
    // 而是直接关闭所有模糊，这是 bug" ⇒ 0 就该是清晰的桌面）。
    //
    // 但仍然做 **2 倍**降采样：全分辨率在轮询循环里太重，
    // 而 2 倍像素平均带来的轻微平滑肉眼接近清晰，代价只有 1/4 数据量。
    let sample = if f == 1 { 2 } else { f };
    let dw = (w / sample).max(1);
    let dh = (h / sample).max(1);
    let raw = platform::grab_scaled(x, y, w, h, dw, dh)?;
    if f == 1 {
        // 不模糊：`grab_scaled` 已经做完 BGRA→RGBA，直接当背景用
        return Some(Backdrop {
            x,
            y,
            width: dw,
            height: dh,
            rgba: raw,
        });
    }
    // ⚠️ factor 传 **1**：图已经是目标尺寸，只需要 box blur，不再降采样
    let mut bd = blur_downsample(&raw, dw as usize, dh as usize, 1);
    bd.x = x;
    bd.y = y;
    Some(bd)
}

/// 抓**窗口周围一块更大的区域**（含 [`margin`] 外扩）并模糊。
///
/// 窗口在屏幕上的位置由 `GetWindowRect` 现取 —— 这样上层完全不用知道坐标。
/// 外扩的余量让"拖动窗口时按偏移采样旧图"成为可能（见 [`Backdrop::x`]）。
pub fn capture_window_blurred(factor: u32) -> Option<Backdrop> {
    let (wx, wy, ww, wh) = platform::window_rect()?;
    let m = margin(ww, wh);
    let x0 = wx - m;
    let y0 = wy - m;
    let x1 = wx + ww as i32 + m;
    let y1 = wy + wh as i32 + m;
    // ⚠️ **裁进虚拟屏幕**：越界的源矩形在 GDI 里是未定义行为（实测给黑色），
    // 窗口贴边时那圈黑会被玻璃的边缘环带采样到，背景就"发黑/发脏"。
    // 裁完之后外扩余量在贴边那侧会变小 —— 无所谓，`uv_map` 用的是**实际**
    // 抓屏原点，映射照样精确；只是那一侧的拖动免抓范围相应缩短。
    let (sx0, sy0, sx1, sy1) = platform::screen_bounds();
    let x0 = x0.max(sx0);
    let y0 = y0.max(sy0);
    let x1 = x1.min(sx1);
    let y1 = y1.min(sy1);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    capture_blurred(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32, factor)
}

/// 降采样 + 两趟 box blur（近似高斯）。
///
/// 两趟 box blur 叠加已经足够接近高斯，而且可以**分离**成"横一趟、竖一趟"，
/// 复杂度从 O(n·r²) 降到 O(n·r) —— 对背景这种大幅度模糊完全够用。
fn blur_downsample(src: &[u8], w: usize, h: usize, factor: usize) -> Backdrop {
    let factor = factor.max(1);
    let dw = (w / factor).max(1);
    let dh = (h / factor).max(1);
    let mut out = vec![0u8; dw * dh * 4];

    // ① 盒式降采样：每个目标像素取源图 factor×factor 块的平均
    for dy in 0..dh {
        for dx in 0..dw {
            let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
            for sy in 0..factor {
                let py = dy * factor + sy;
                if py >= h {
                    break;
                }
                for sx in 0..factor {
                    let px = dx * factor + sx;
                    if px >= w {
                        break;
                    }
                    let i = (py * w + px) * 4;
                    r += src[i] as u32;
                    g += src[i + 1] as u32;
                    b += src[i + 2] as u32;
                    n += 1;
                }
            }
            let n = n.max(1);
            let o = (dy * dw + dx) * 4;
            out[o] = (r / n) as u8;
            out[o + 1] = (g / n) as u8;
            out[o + 2] = (b / n) as u8;
            out[o + 3] = 255;
        }
    }

    // ② 横竖各一趟 box blur
    let radius = 4usize;
    box_blur(&mut out, dw, dh, radius);
    // x/y 由调用方填（这里不知道屏幕坐标），默认 0 占位
    Backdrop {
        x: 0,
        y: 0,
        width: dw as u32,
        height: dh as u32,
        rgba: out,
    }
}

/// 分离式 box blur：先横后竖。
///
/// ## ⚠️ 为什么必须**滑动窗口**
///
/// 上一版对每个像素都重新累加 `lo..=hi`（`O(n·r)`）。在**未优化构建**下
/// 470×430 的图要跑约 1100 万次内层循环 ⇒ 单次模糊上百毫秒 —— 用户实测
/// 玻璃模式 CPU 10–30%，这是一大块。改成滑动窗口后复杂度是 `O(n)`，
/// **与半径无关**。
///
/// ## ⚠️ 为什么两趟都必须用临时缓冲
///
/// 上一版的**纵向**那趟是"边读边写同一个 `buf`"：算 `y` 时要读 `[y-r, y+r]`，
/// 但 `y-r .. y-1` 已经被写成了模糊后的值 ⇒ 那不是 box blur，
/// 而是一个**向下拖尾的 IIR 滤波**（上下不对称）。现在先把整行/整列拷进
/// `tmp` 再算，结果才是对称的、真正意义上的盒式模糊。
fn box_blur(buf: &mut [u8], w: usize, h: usize, radius: usize) {
    let r = radius.max(1);
    let mut tmp = vec![0u8; w.max(h).max(1) * 4];

    // ① 横向：整行拷进 `tmp` → 从 `tmp` 滑动求和 → 写回 `buf`
    for y in 0..h {
        let base = y * w * 4;
        tmp[..w * 4].copy_from_slice(&buf[base..base + w * 4]);

        let hi0 = r.min(w - 1);
        let mut acc = [0u32; 3];
        for xx in 0..=hi0 {
            for c in 0..3 {
                acc[c] += tmp[xx * 4 + c] as u32;
            }
        }
        let mut n = (hi0 + 1) as u32;

        for x in 0..w {
            for c in 0..3 {
                buf[base + x * 4 + c] = (acc[c] / n) as u8;
            }
            // 窗口右移一格：右边进来一个、左边出去一个
            let add = x + r + 1;
            if add < w {
                for c in 0..3 {
                    acc[c] += tmp[add * 4 + c] as u32;
                }
                n += 1;
            }
            if x >= r {
                let sub = x - r;
                for c in 0..3 {
                    acc[c] -= tmp[sub * 4 + c] as u32;
                }
                n -= 1;
            }
        }
    }

    // ② 纵向：同样先把整列拷进 `tmp`（步长是整行），再滑动
    for x in 0..w {
        for y in 0..h {
            let i = (y * w + x) * 4;
            tmp[y * 4..y * 4 + 4].copy_from_slice(&buf[i..i + 4]);
        }

        let hi0 = r.min(h - 1);
        let mut acc = [0u32; 3];
        for yy in 0..=hi0 {
            for c in 0..3 {
                acc[c] += tmp[yy * 4 + c] as u32;
            }
        }
        let mut n = (hi0 + 1) as u32;

        for y in 0..h {
            let o = (y * w + x) * 4;
            for c in 0..3 {
                buf[o + c] = (acc[c] / n) as u8;
            }
            let add = y + r + 1;
            if add < h {
                for c in 0..3 {
                    acc[c] += tmp[add * 4 + c] as u32;
                }
                n += 1;
            }
            if y >= r {
                let sub = y - r;
                for c in 0..3 {
                    acc[c] -= tmp[sub * 4 + c] as u32;
                }
                n -= 1;
            }
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::ffi::c_void;

    type Hdc = *mut c_void;
    type Hbitmap = *mut c_void;
    type Hgdiobj = *mut c_void;
    type Hwnd = *mut c_void;
    type Bool = i32;

    /// GDI 的 `SRCCOPY`
    const SRCCOPY: u32 = 0x00CC_0020;
    /// `DIB_RGB_COLORS`
    const DIB_RGB_COLORS: u32 = 0;
    /// `BI_RGB`（未压缩）
    const BI_RGB: u32 = 0;
    /// 让本窗口在**捕获**中不可见（Win10 2004+）
    const WDA_EXCLUDEFROMCAPTURE: u32 = 0x11;
    /// 恢复正常（可被捕获）
    const WDA_NONE: u32 = 0x00;

    #[repr(C)]
    struct BitmapInfoHeader {
        size: u32,
        width: i32,
        height: i32,
        planes: u16,
        bit_count: u16,
        compression: u32,
        size_image: u32,
        x_pels_per_meter: i32,
        y_pels_per_meter: i32,
        clr_used: u32,
        clr_important: u32,
    }

    #[repr(C)]
    struct RgbQuad {
        blue: u8,
        green: u8,
        red: u8,
        reserved: u8,
    }

    #[repr(C)]
    struct BitmapInfo {
        header: BitmapInfoHeader,
        colors: [RgbQuad; 1],
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetDC(hwnd: Hwnd) -> Hdc;
        fn ReleaseDC(hwnd: Hwnd, hdc: Hdc) -> i32;
        fn SetWindowDisplayAffinity(hwnd: Hwnd, affinity: u32) -> Bool;
    }

    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn CreateCompatibleDC(hdc: Hdc) -> Hdc;
        fn CreateCompatibleBitmap(hdc: Hdc, w: i32, h: i32) -> Hbitmap;
        fn SelectObject(hdc: Hdc, obj: Hgdiobj) -> Hgdiobj;
        /// 缩放拷贝：源矩形 `(sx, sy, sw, sh)` → 目标矩形 `(x, y, w, h)`。
        ///
        /// 用它替代"BitBlt 抓全分辨率 + 在 CPU 上缩"—— GDI 在拷贝时就把图缩小，
        /// 数据量降 `factor²` 倍（940×860 抓成 118×108 时只剩 1/64）。
        fn StretchBlt(
            dst: Hdc,
            x: i32,
            y: i32,
            w: i32,
            h: i32,
            src: Hdc,
            src_x: i32,
            src_y: i32,
            src_w: i32,
            src_h: i32,
            rop: u32,
        ) -> Bool;
        /// 设置缩放模式：`HALFTONE` 会做像素平均（平滑），`COLORONCOLOR` 直接丢像素（更快）。
        fn SetStretchBltMode(hdc: Hdc, mode: i32) -> i32;
        fn GetDIBits(
            hdc: Hdc,
            bmp: Hbitmap,
            start: u32,
            lines: u32,
            bits: *mut c_void,
            info: *mut BitmapInfo,
            usage: u32,
        ) -> i32;
        fn DeleteObject(obj: Hgdiobj) -> Bool;
        fn DeleteDC(hdc: Hdc) -> Bool;
    }

    /// `SetStretchBltMode` 的 `HALFTONE`：缩小的时候做**像素平均**。
    /// 质量最好，但 GDI 要为每一个目标像素去平均一大块源像素 —— 最慢。
    const HALFTONE: i32 = 4;
    /// `COLORONCOLOR`：直接**丢掉**多余的行列，不做平均。快得多。
    const COLORONCOLOR: i32 = 3;
    /// `GetSystemMetrics` 下标：虚拟屏幕原点与尺寸（多显示器合并后的整块桌面）
    const SM_XVIRTUALSCREEN: i32 = 76;
    const SM_YVIRTUALSCREEN: i32 = 77;
    const SM_CXVIRTUALSCREEN: i32 = 78;
    const SM_CYVIRTUALSCREEN: i32 = 79;

    #[repr(C)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetWindowRect(hwnd: Hwnd, rect: *mut Rect) -> Bool;
        fn GetSystemMetrics(index: i32) -> i32;
    }

    /// 本进程主窗口在屏幕上的矩形 `(x, y, w, h)`
    pub fn window_rect() -> Option<(i32, i32, u32, u32)> {
        let hwnd = super::imp::find_main_window()?;
        // SAFETY: `hwnd` 是本进程的主窗口；`rect` 是栈上的、大小正确
        unsafe {
            let mut r = Rect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            if GetWindowRect(hwnd, &mut r) == 0 {
                return None;
            }
            let w = (r.right - r.left).max(0) as u32;
            let h = (r.bottom - r.top).max(0) as u32;
            if w == 0 || h == 0 {
                return None;
            }
            Some((r.left, r.top, w, h))
        }
    }

    /// **虚拟屏幕**的边界 `(left, top, right, bottom)`（多显示器合并后的整块桌面）。
    ///
    /// 抓屏区域必须裁到这块里：越界的 `StretchBlt` 源矩形在 GDI 里是未定义行为，
    /// 实测拿到的是**黑色**，于是窗口贴着屏幕边缘时背景外圈会出现黑带
    ///（玻璃边缘环带往外采样正好采到它们）。
    pub fn screen_bounds() -> (i32, i32, i32, i32) {
        // SAFETY: 无参数、无指针，`GetSystemMetrics` 失败时返回 0
        unsafe {
            let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
            let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
            if w <= 0 || h <= 0 {
                return (0, 0, i32::MAX, i32::MAX);
            }
            (x, y, x + w, y + h)
        }
    }

    /// 抓屏幕上一块区域并**直接缩放到 `dw × dh`**，返回 RGBA（alpha 恒 255）。
    ///
    /// 抓之前把自己的窗口排除在捕获之外，抓完立刻恢复 —— 否则抓到的是
    /// "桌面 + 自己半透明的界面"混在一起，当背景用会出重影。
    pub fn grab_scaled(x: i32, y: i32, w: u32, h: u32, dw: u32, dh: u32) -> Option<Vec<u8>> {
        // SAFETY: 所有句柄都在本函数内成对获取/释放；缓冲区长度按 dw*dh*4 分配，
        // 与传给 `GetDIBits` 的尺寸一致。
        unsafe {
            let hwnd = super::imp::find_main_window();
            let excluded = match hwnd {
                Some(h) => SetWindowDisplayAffinity(h, WDA_EXCLUDEFROMCAPTURE) != 0,
                None => false,
            };
            let result = grab_inner(x, y, w, h, dw, dh);
            if excluded && let Some(h) = hwnd {
                SetWindowDisplayAffinity(h, WDA_NONE);
            }
            result
        }
    }

    unsafe fn grab_inner(x: i32, y: i32, w: u32, h: u32, dw: u32, dh: u32) -> Option<Vec<u8>> {
        unsafe {
            let screen = GetDC(std::ptr::null_mut());
            if screen.is_null() {
                return None;
            }
            let mem = CreateCompatibleDC(screen);
            // ⚠️ **位图按目标尺寸建**（`dw × dh`），不是源尺寸 ——
            // 这就是"抓屏时就缩小"的关键：GDI 直接往这块小位图里画。
            let bmp = CreateCompatibleBitmap(screen, dw as i32, dh as i32);
            if mem.is_null() || bmp.is_null() {
                if !mem.is_null() {
                    DeleteDC(mem);
                }
                ReleaseDC(std::ptr::null_mut(), screen);
                return None;
            }
            let old = SelectObject(mem, bmp as Hgdiobj);

            let mut out = None;
            // ⚠️ **缩放模式按缩小倍数决定**（用户要求 CPU < 5%）：
            // - 只缩到 1/2（模糊强度 = 0，用户要的是"清晰的桌面"）⇒ 必须
            //   `HALFTONE`，否则直接丢像素会出现明显锯齿。
            // - 缩到 1/4 及更小 ⇒ 换 `COLORONCOLOR`。反正后面还有 box blur，
            //   丢掉的高频细节本来也会被抹平，而 GDI 这一趟能快不少。
            let ratio = w / dw.max(1);
            SetStretchBltMode(mem, if ratio <= 2 { HALFTONE } else { COLORONCOLOR });
            if StretchBlt(
                mem, 0, 0, dw as i32, dh as i32, screen, x, y, w as i32, h as i32, SRCCOPY,
            ) != 0
            {
                let mut info = BitmapInfo {
                    header: BitmapInfoHeader {
                        size: std::mem::size_of::<BitmapInfoHeader>() as u32,
                        width: dw as i32,
                        // ⚠️ **负高度 = top-down**：否则位图是倒的
                        height: -(dh as i32),
                        planes: 1,
                        bit_count: 32,
                        compression: BI_RGB,
                        size_image: 0,
                        x_pels_per_meter: 0,
                        y_pels_per_meter: 0,
                        clr_used: 0,
                        clr_important: 0,
                    },
                    colors: [RgbQuad {
                        blue: 0,
                        green: 0,
                        red: 0,
                        reserved: 0,
                    }],
                };
                let mut buf = vec![0u8; (dw * dh * 4) as usize];
                let lines = GetDIBits(
                    mem,
                    bmp,
                    0,
                    dh,
                    buf.as_mut_ptr() as *mut c_void,
                    &mut info,
                    DIB_RGB_COLORS,
                );
                if lines != 0 {
                    // GDI 给的是 BGRA（而且抓屏得来的 alpha 恒为 0）
                    for px in buf.chunks_exact_mut(4) {
                        px.swap(0, 2);
                        px[3] = 255;
                    }
                    out = Some(buf);
                }
            }

            SelectObject(mem, old);
            DeleteObject(bmp as Hgdiobj);
            DeleteDC(mem);
            ReleaseDC(std::ptr::null_mut(), screen);
            out
        }
    }
}

#[cfg(not(windows))]
mod platform {
    /// 非 Windows：暂时没有抓屏实现（iced 自身的 `window::Settings::blur`
    /// 在 macOS/Linux 上已能工作）。返回 `None` ⇒ 界面退回氛围底色。
    pub fn grab_scaled(_x: i32, _y: i32, _w: u32, _h: u32, _dw: u32, _dh: u32) -> Option<Vec<u8>> {
        None
    }

    pub fn window_rect() -> Option<(i32, i32, u32, u32)> {
        None
    }

    /// 非 Windows 没有抓屏，边界给"无限大"即可（裁剪变成恒等）。
    pub fn screen_bounds() -> (i32, i32, i32, i32) {
        (i32::MIN, i32::MIN, i32::MAX, i32::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 降采样 + 模糊这部分是**纯函数**，可以脱开抓屏单独验证。
    ///
    /// 用黑白棋盘做输入：任一块降采样后都该接近中灰（黑白各半），
    /// 再经过两趟 box blur 会更加均匀。这同时验证了"不 panic"与"alpha 补满"。
    #[test]
    fn blur_downsample_reduces_size_and_smooths() {
        let (w, h, factor) = (16usize, 16usize, 4usize);
        let mut src = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let v = if (x + y) % 2 == 0 { 0 } else { 255 };
                let i = (y * w + x) * 4;
                src[i] = v;
                src[i + 1] = v;
                src[i + 2] = v;
                src[i + 3] = 255;
            }
        }

        let bd = blur_downsample(&src, w, h, factor);
        assert_eq!((bd.width, bd.height), (4, 4), "应降采样到 1/4 尺寸");
        assert_eq!(bd.rgba.len(), 4 * 4 * 4, "RGBA 长度应为 w*h*4");

        for px in bd.rgba.chunks_exact(4) {
            assert!(
                (px[0] as i32 - 127).abs() < 40,
                "棋盘降采样后应接近中灰，实际 {}",
                px[0]
            );
            assert_eq!(px[3], 255, "GDI 抓来的 alpha 恒为 0，必须补成 255");
        }
    }

    /// box blur 必须**上下左右对称**。
    ///
    /// 上一版的纵向那趟是"边读边写同一个缓冲"：算第 `y` 行时要读 `[y-r, y+r]`，
    /// 可 `y-r .. y-1` 已经被写成了模糊后的值 ⇒ 那不是盒式模糊，而是一个
    /// **向下拖尾**的 IIR 滤波，上下不对称。这条用例专门锁住对称性。
    #[test]
    fn box_blur_is_symmetric() {
        let (w, h) = (9usize, 9usize);
        let mut buf = vec![0u8; w * h * 4];
        for px in buf.chunks_exact_mut(4) {
            px[3] = 255;
        }
        let at = |x: usize, y: usize| (y * w + x) * 4;
        let c = at(4, 4);
        buf[c] = 255;
        buf[c + 1] = 255;
        buf[c + 2] = 255;
        box_blur(&mut buf, w, h, 2);

        let v = |x: usize, y: usize| buf[at(x, y)] as i32;
        assert_eq!(v(2, 4), v(6, 4), "左右应对称：{} vs {}", v(2, 4), v(6, 4));
        assert_eq!(v(4, 2), v(4, 6), "上下应对称：{} vs {}", v(4, 2), v(4, 6));
        assert_eq!(v(3, 4), v(4, 3), "横竖两个方向等价位置应相等");
        assert!(v(4, 4) > 0, "中心不该被抹成 0");
    }

    /// 极端参数不能 panic（0 尺寸、factor 比图还大）
    #[test]
    fn blur_downsample_survives_degenerate_input() {
        let src = vec![128u8; 8 * 8 * 4];
        for (w, h, f) in [(8usize, 8usize, 1usize), (8, 8, 64), (1, 1, 8)] {
            let bd = blur_downsample(&src, w, h, f);
            assert!(bd.width >= 1 && bd.height >= 1, "尺寸不能塌成 0");
        }
    }
}
