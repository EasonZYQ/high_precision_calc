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
    /// **抓屏区域的物理尺寸** `(宽, 高)`（＝窗口 + 外扩余量，裁进屏幕之前算好的）。
    ///
    /// ⚠️ 与 [`Backdrop::width`]（降采样后的**纹理像素**尺寸）是两回事：
    /// uv 映射必须用**区域**尺寸 —— `图像uv = 窗口uv × 窗口宽/区域宽 + 原点差/区域宽`。
    /// 上一版把纹理像素尺寸当区域尺寸传给 [`uv_map`]，scale 和 offset 全部
    /// 按降采样倍数翻倍 ⇒ 窗口里显示的是"整个桌面被压扁再 clamp"的鬼影
    /// （用户实测："背景截取的位置非常偏，与实际位置不符，且缩小了很多"）。
    pub rw: u32,
    pub rh: u32,
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
/// - `cap` = 抓屏区域 `(x, y, w, h)`（屏幕**物理**像素；宽高是**区域**尺寸，
///   存在 [`Backdrop::rw`] / [`Backdrop::rh`] —— 不是降采样后的纹理像素尺寸！）
/// - 拿不到当前窗口位置（非 Windows / 句柄丢失）时返回 `None`，
///   调用方退回恒等映射（不偏移，仅首次布局时短暂出现）。
pub fn uv_map(cap: (i32, i32, u32, u32)) -> Option<([f32; 2], [f32; 2])> {
    let win = platform::window_rect()?;
    uv_map_with(cap, win)
}

/// [`uv_map`] 的纯函数版本：窗口矩形由调用方给（可测试、不碰 Win32）。
///
/// 数学：窗口内归一化坐标 `s` 对应的屏幕点 = `win.xy + s × win.wh`，
/// 它在抓屏图里的归一化位置 = `(屏幕点 - cap.xy) / cap.wh`。
/// 展开 ⇒ `图像uv = s × (win.wh / cap.wh) + (win.xy - cap.xy) / cap.wh`。
pub fn uv_map_with(
    cap: (i32, i32, u32, u32),
    win: (i32, i32, u32, u32),
) -> Option<([f32; 2], [f32; 2])> {
    let sw = cap.2 as f32;
    let sh = cap.3 as f32;
    if sw <= 0.0 || sh <= 0.0 {
        return None;
    }
    Some((
        [win.2 as f32 / sw, win.3 as f32 / sh],
        [(win.0 - cap.0) as f32 / sw, (win.1 - cap.1) as f32 / sh],
    ))
}

/// 当前主窗口的屏幕矩形（转发 [`platform::window_rect`]，给拖动检测用）。
pub fn window_rect() -> Option<(i32, i32, u32, u32)> {
    platform::window_rect()
}

/// 开/关"本窗口不出现在任何捕获里"（转发 [`platform::exclude_from_capture`]）
pub fn exclude_from_capture(on: bool) -> bool {
    platform::exclude_from_capture(on)
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
        let bd = Backdrop {
            x,
            y,
            width: dw,
            height: dh,
            rw: w,
            rh: h,
            rgba: raw,
        };
        dump_if_requested(&bd);
        return Some(bd);
    }
    // ⚠️ factor 传 **1**：图已经是目标尺寸，只需要 box blur，不再降采样
    let mut bd = blur_downsample(&raw, dw as usize, dh as usize, 1);
    bd.x = x;
    bd.y = y;
    bd.rw = w;
    bd.rh = h;
    Some(bd)
}

/// 抓**窗口周围一块更大的区域**（含 [`margin`] 外扩）并模糊。
///
/// 窗口在屏幕上的位置由 `GetWindowRect` 现取 —— 这样上层完全不用知道坐标。
/// 外扩的余量让"拖动窗口时按偏移采样旧图"成为可能（见 [`Backdrop::x`]）。
/// **是否允许被截图/录屏**（进程级开关，见 `GuiPrefs::capture`）。
///
/// `true`（默认）= 不开 `WDA_EXCLUDEFROMCAPTURE`：抓到的帧里有自己，但截图/录屏正常。
/// `false` = 排除自己：背景是纯桌面，但截图里没有这个窗口。
static CAPTURE_OK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// 允许被截图吗
pub fn capture_ok() -> bool {
    CAPTURE_OK.load(std::sync::atomic::Ordering::Relaxed)
}

/// 设置"允许被截图"，并**立刻**把窗口亲和性同步过去（不用重启）
pub fn set_capture_ok(ok: bool) {
    CAPTURE_OK.store(ok, std::sync::atomic::Ordering::Relaxed);
    exclude_from_capture(!ok);
}

/// 调试用：把"喂给玻璃的背景纹理"导出成 PPM，并把几何打到 stderr。
///
/// 只有 `HIPERCALC_DUMP_BG=1` 时才动作 —— 排查"背景里为何有自己的窗口 / 为何发虚"
/// 这类问题，看这张图比看屏幕快得多（它是**纹理原图**，不是最终合成）。
fn dump_if_requested(bd: &Backdrop) {
    if std::env::var("HIPERCALC_DUMP_BG").ok().as_deref() != Some("1") {
        return;
    }
    let (w, h) = (bd.width as usize, bd.height as usize);
    let path = std::env::temp_dir().join("hc_bg_dump.ppm");
    let mut out = Vec::with_capacity(w * h * 3 + 32);
    out.extend_from_slice(format!("P6\n{w} {h}\n255\n").as_bytes());
    for i in 0..(w * h) {
        out.push(bd.rgba[i * 4]);
        out.push(bd.rgba[i * 4 + 1]);
        out.push(bd.rgba[i * 4 + 2]);
    }
    let _ = std::fs::write(&path, out);
    eprintln!(
        "[bg-dump] tex={}x{} rw={} rh={} origin=({},{}) win={:?} -> {:?}",
        bd.width,
        bd.height,
        bd.rw,
        bd.rh,
        bd.x,
        bd.y,
        window_rect(),
        path
    );
}

/// 把"自己窗口那一块"从抓到的桌面里**补掉**（四边平均色 + 双线性 + 抖动）。
///
/// ## 为什么需要它
///
/// 「允许截图」时不能用 `WDA_EXCLUDEFROMCAPTURE`（那会让微信/PrntSc 拍到空白），
/// 于是 **DXGI 桌面复制抓到的帧里有我们自己**（用户 2026-10-26："一打开允许截图，
/// 背景就会把自己加进去"）。
///
/// ## 为什么不逐行/逐列取边界色（上一版，被否）
///
/// 上一版按每一行取"窗口左边那一列 / 右边那一列"的颜色再横向插值。桌面在
/// **窗口左右边缘那两列**上的任何明暗结构（别的窗口边、壁纸的条带）都会被
/// **原样拉成横贯整个窗口的条带**。用户实测："允许截图就会变成这样"（截图里
/// 整片横向条纹）。
///
/// ## 现在的做法
///
/// 四条边各取一条 24px 宽**条带的平均色**（各一个颜色），窗口内部用这四个
/// 颜色做双线性混合，再叠 ±1 个量化级的**抖动**（否则这么大的平坦渐变在
/// 8bit 下会看到一条条等值线）。结果是一片**平滑、无结构**的过渡 —— 和被
/// 模糊过的桌面在观感上是同一类东西，不会再出现假结构。
///
/// `rect` 是**纹理坐标**（`x0, y0, x1, y1`，右/下为开区间）。
fn inpaint_self_rect(rgba: &mut [u8], tw: usize, th: usize, rect: (i32, i32, i32, i32)) {
    let (mut x0, mut y0, mut x1, mut y1) = rect;
    x0 = x0.max(0);
    y0 = y0.max(0);
    x1 = x1.min(tw as i32);
    y1 = y1.min(th as i32);
    if x1 - x0 < 2 || y1 - y0 < 2 {
        return;
    }
    let (x0, y0, x1, y1) = (x0 as usize, y0 as usize, x1 as usize, y1 as usize);
    let at = |x: usize, y: usize| -> [f32; 3] {
        let i = (y * tw + x) * 4;
        [rgba[i] as f32, rgba[i + 1] as f32, rgba[i + 2] as f32]
    };
    // 条带取样：离窗口边界 3px 起，宽 24px（远离自己的圆角/抗锯齿）
    const OUT: usize = 3;
    const STRIP: usize = 24;
    let has_l = x0 > OUT;
    let has_r = x1 + OUT < tw;
    let has_t = y0 > OUT;
    let has_b = y1 + OUT < th;
    if !(has_l || has_r || has_t || has_b) {
        return;
    }

    // 每条边一个平均色（条带内的像素平均）—— 刻意**丢掉**逐行结构
    let mut acc = [[0.0f32; 3]; 4];
    let mut cnt = [0.0f32; 4];
    for y in y0..y1 {
        if has_l {
            let xa = x0 - OUT - STRIP.min(x0 - OUT);
            for x in xa..x0 - OUT {
                let c = at(x, y);
                acc[0] = add(acc[0], c);
                cnt[0] += 1.0;
            }
        }
        if has_r {
            for x in x1 + OUT..(x1 + OUT + STRIP).min(tw) {
                let c = at(x, y);
                acc[1] = add(acc[1], c);
                cnt[1] += 1.0;
            }
        }
    }
    for x in x0..x1 {
        if has_t {
            let ya = y0 - OUT - STRIP.min(y0 - OUT);
            for y in ya..y0 - OUT {
                let c = at(x, y);
                acc[2] = add(acc[2], c);
                cnt[2] += 1.0;
            }
        }
        if has_b {
            for y in y1 + OUT..(y1 + OUT + STRIP).min(th) {
                let c = at(x, y);
                acc[3] = add(acc[3], c);
                cnt[3] += 1.0;
            }
        }
    }
    let mut side = [[0.0f32; 3]; 4];
    let mut have = [false; 4];
    for k in 0..4 {
        if cnt[k] > 0.0 {
            side[k] = scale(acc[k], 1.0 / cnt[k]);
            have[k] = true;
        }
    }
    // 缺哪边就用其余各边的平均（贴屏边时抓屏矩形被裁，那一侧本来就没有像素）
    let mut all = [0.0f32; 3];
    let mut n_all = 0.0f32;
    for k in 0..4 {
        if have[k] {
            all = add(all, side[k]);
            n_all += 1.0;
        }
    }
    let fallback = scale(all, 1.0 / n_all.max(1.0));
    for k in 0..4 {
        if !have[k] {
            side[k] = fallback;
        }
    }
    let (l, r, t, b) = (side[0], side[1], side[2], side[3]);

    let w = (x1 - x0) as f32;
    let h = (y1 - y0) as f32;
    // 到四条边的归一化距离：越贴哪条边，那条边的颜色权重越大（1/d²）。
    // ⚠️ 这样**每条边上的颜色就等于那条边的平均色**，与外侧真实像素**连续**；
    // 内部自然过渡到四色平均。用"两组线性插值再平均"做不到这一点（实测接缝差 65 级）。
    const EDGE_EPS: f32 = 0.02;
    for y in y0..y1 {
        let v = (y - y0) as f32 / h;
        let (dv_t, dv_b) = (v + EDGE_EPS, 1.0 - v + EDGE_EPS);
        for x in x0..x1 {
            let u = (x - x0) as f32 / w;
            let (du_l, du_r) = (u + EDGE_EPS, 1.0 - u + EDGE_EPS);
            let (wl, wr) = (1.0 / (du_l * du_l), 1.0 / (du_r * du_r));
            let (wt, wb) = (1.0 / (dv_t * dv_t), 1.0 / (dv_b * dv_b));
            let sum = wl + wr + wt + wb;
            let mut c = scale(
                add(
                    add(scale(l, wl), scale(r, wr)),
                    add(scale(t, wt), scale(b, wb)),
                ),
                1.0 / sum,
            );
            // ±1 级抖动：打断 8bit 平坦渐变的等值线（用户看到的"横向条带"有一部分
            // 就是量化条带）
            let n = ((x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663)) & 0xFF) as f32;
            let d = (n / 255.0 - 0.5) * 1.6;
            c = add(c, [d, d, d]);
            let i = (y * tw + x) * 4;
            rgba[i] = c[0].clamp(0.0, 255.0) as u8;
            rgba[i + 1] = c[1].clamp(0.0, 255.0) as u8;
            rgba[i + 2] = c[2].clamp(0.0, 255.0) as u8;
            rgba[i + 3] = 255;
        }
    }
}

#[inline]
fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

#[inline]
fn scale(a: [f32; 3], k: f32) -> [f32; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

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
    let w = (x1 - x0) as u32;
    let h = (y1 - y0) as u32;

    // ── ⚠️ 桌面复制优先（Windows）────────────────────────────────────────
    //
    // 它能在 60Hz 上轮询（桌面没变化时每次 0.1ms），把 GDI 那 65–90ms 的
    // 回读换成 GPU 侧缩小 + 读小图（3–5ms）⇒ 背景终于能到 60fps。
    //
    // 三态必须分开：`Unchanged` 直接返回 `None`（上层沿用上一张纹理，
    // **绝不能**顺手退回 GDI）；只有 `Unsupported` 才走下面 GDI 那条路。
    #[cfg(windows)]
    {
        // ⚠️ **1 就是 1，不许偷偷降成 2**（用户反馈："模糊为 0 背景还是不够清晰，
        // 与桌面本身相差甚远"）。这里以前写死 `if factor == 1 { 2 }` —— 于是
        // "不模糊"档实际是**半分辨率**：1175 物理像素宽只用 588 个 texel，
        // 相当于放大 2 倍，文字必然发虚（实测 dump：抓屏 2160×1440、纹理 1080×720）。
        // 现在 sample=1 ⇒ 1 texel 对 1 像素，逐像素清晰。
        let sample = factor.max(1);
        let polled = crate::dupl::poll(x0, y0, w, h, sample);
        if std::env::var("HIPERCALC_DUMP_BG").ok().as_deref() == Some("1") {
            eprintln!(
                "[bg-dump] dupl = {}",
                match &polled {
                    crate::dupl::Poll::Fresh { .. } => "Fresh",
                    crate::dupl::Poll::Unchanged => "Unchanged",
                    crate::dupl::Poll::Unsupported => "Unsupported",
                }
            );
        }
        match polled {
            crate::dupl::Poll::Fresh {
                rgba,
                w: fw,
                h: fh,
                x: fx,
                y: fy,
                rw: frw,
                rh: frh,
            } => {
                // ── 把自己的窗口从这一帧里**补掉** ───────────────────────────
                //
                // 「允许截图」时我们**不能**用 `WDA_EXCLUDEFROMCAPTURE`（那会让
                // 微信/PrntSc 拍到空白），于是桌面复制抓到的帧里必然有自己
                //（用户 2026-10-26："一打开允许截图，背景就会把自己加进去"）。
                // 这里用四边插值把窗口那块补成周围的渐变（见 `inpaint_self_rect`）。
                //
                // 坐标换算：纹理 `fw×fh` 覆盖屏幕区域 `(fx, fy, frw, frh)`。
                let mut rgba = rgba;
                if capture_ok() && frw > 0 && frh > 0 {
                    let sx = fw as f32 / frw as f32;
                    let sy = fh as f32 / frh as f32;
                    let rect = (
                        (((wx - fx) as f32) * sx).round() as i32,
                        (((wy - fy) as f32) * sy).round() as i32,
                        (((wx + ww as i32 - fx) as f32) * sx).round() as i32,
                        (((wy + wh as i32 - fy) as f32) * sy).round() as i32,
                    );
                    inpaint_self_rect(&mut rgba, fw as usize, fh as usize, rect);
                    // 只报**一次**（排障用）：每帧都打会把 stderr 刷满
                    static ONCE: std::sync::atomic::AtomicBool =
                        std::sync::atomic::AtomicBool::new(true);
                    if ONCE.swap(false, std::sync::atomic::Ordering::Relaxed)
                        && std::env::var("HIPERCALC_DUMP_BG").ok().as_deref() == Some("1")
                    {
                        eprintln!("[bg-dump] inpaint 已启用，rect={rect:?}（纹理 {fw}×{fh}）");
                    }
                }
                // ⚠️ **GPU 的 mip 链已经把 2×2 盒式平均做了 log2(sample) 遍**，
                // 再加 CPU 那两趟 radius=4 的 box blur 是**重复劳动**
                //（实测每次 0.3–0.6ms，在 30fps 上就是 1–2% 单核）。
                // 1/8 那一层本身就是"8×8 像素平均"，与 GDI 路径（降采样 +
                // box blur）的观感等价 —— 所以这里直接用 mip 的结果。
                let mut bd = Backdrop {
                    x: 0,
                    y: 0,
                    width: fw,
                    height: fh,
                    rw: 0,
                    rh: 0,
                    rgba,
                };
                bd.x = fx;
                bd.y = fy;
                // ⚠️ 必须是**实际**抓到的矩形（可能比请求的小 —— 裁进显示器后）
                bd.rw = frw;
                bd.rh = frh;
                // ⚠️ **不再用 `WDA_EXCLUDEFROMCAPTURE` 排除自己**（用户反馈：
                // "微信和 Windows PrtSc 都无法截图，必须修复"）—— 那个开关对
                // **所有**捕获者生效，一开我们窗口在截图里就消失。
                //
                // 于是抓到的帧里包含我们自己；这里用**窗口外那一圈的平均色**
                // 把窗口那块填平，既没有"自己的倒影"，也不会因为界面重绘
                // 而让"桌面看起来变了"（内容恒定 ⇒ 不会自激空转）。
                // 开关是**每帧同步**的：用户一按设置就生效，不需要重启
                let ok = capture_ok();
                #[cfg(windows)]
                platform::exclude_from_capture(!ok);
                // ⚠️ 这里**不再**说"允许截图就得接受自己的倒影"：上一版是这么定的，
                // 用户 2026-10-26 实测（截图里左侧那一列模糊文字就是自己）否掉了。
                // 现在自己在 `inpaint_self_rect` 里被补掉，两种模式都看不到自己。
                dump_if_requested(&bd);
                return Some(bd);
            }
            crate::dupl::Poll::Unchanged => return None,
            crate::dupl::Poll::Unsupported => {}
        }
    }

    capture_blurred(x0, y0, w, h, factor)
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
    // x/y/rw/rh 由调用方填（这里不知道屏幕坐标），默认 0 占位
    Backdrop {
        x: 0,
        y: 0,
        width: dw as u32,
        height: dh as u32,
        rw: 0,
        rh: 0,
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

    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetClientRect(hwnd: Hwnd, rect: *mut Rect) -> Bool;
        fn ClientToScreen(hwnd: Hwnd, pt: *mut Point) -> Bool;
        fn GetSystemMetrics(index: i32) -> i32;
    }

    /// 本进程主窗口**客户区**在屏幕上的矩形 `(x, y, w, h)`（物理像素）。
    ///
    /// ⚠️ 必须用**客户区**而不是 `GetWindowRect`：后者包含 DWM 的不可见
    /// 阴影边框（四边各 ~7px），拿它做 uv 映射会让背景整体偏移、比例也差一点
    ///（用户实测："背景截取的位置非常偏，与实际位置不符"）。
    /// 客户区才是 iced 真正渲染内容的那块 —— `uv [0,1]` 应当恰好铺在它上面。
    pub fn window_rect() -> Option<(i32, i32, u32, u32)> {
        let hwnd = super::imp::find_main_window()?;
        // SAFETY: `hwnd` 是本进程的主窗口；`rect` / `pt` 是栈上的、大小正确
        unsafe {
            let mut r = Rect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            if GetClientRect(hwnd, &mut r) == 0 {
                return None;
            }
            let mut pt = Point { x: 0, y: 0 };
            if ClientToScreen(hwnd, &mut pt) == 0 {
                return None;
            }
            let w = (r.right - r.left).max(0) as u32;
            let h = (r.bottom - r.top).max(0) as u32;
            if w == 0 || h == 0 {
                return None;
            }
            Some((pt.x, pt.y, w, h))
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

    /// **开/关**"本窗口不出现在任何捕获里"（`WDA_EXCLUDEFROMCAPTURE`）。
    ///
    /// 抓屏必须排除自己 —— 否则抓到的是"桌面 + 自己半透明的界面"混在一起，
    /// 当背景用会出重影；桌面复制更是会形成**自激循环**（见 [`crate::dupl`]）：
    /// 界面重绘 → 桌面变了 → 抓 → 触发重绘 → ……
    ///
    /// ⚠️ 副作用：开着它的时候，用户的截图/录屏里本窗口会是"看穿"的。
    /// 所以 GDI 那条路只在抓屏那一刻开（约 90ms）；桌面复制若要 60fps
    /// 就必须常开。
    pub fn exclude_from_capture(on: bool) -> bool {
        let Some(hwnd) = super::imp::find_main_window() else {
            return false;
        };
        // SAFETY: `hwnd` 是本进程主窗口；affinity 是该 API 的两个合法取值之一
        unsafe {
            SetWindowDisplayAffinity(hwnd, if on { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE }) != 0
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

    /// 非 Windows：没有抓屏，也就不需要排除
    pub fn exclude_from_capture(_on: bool) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **自己的窗口必须被"补掉"**（用户 2026-10-26："允许截图时背景里有自己"）。
    ///
    /// 造一张"左红右蓝、中间一块纯白（= 自己）"的纹理，补完之后：
    /// 1. 白块必须消失（不能残留高亮）；
    /// 2. 补出来的是**左红→右蓝的渐变**（不是一块死色）；
    /// 3. 接缝处要连续（块内靠近左缘 ≈ 块外左侧的真实像素）。
    #[test]
    fn inpaint_removes_self_and_keeps_a_gradient() {
        let (tw, th) = (60usize, 40usize);
        let mut img = vec![0u8; tw * th * 4];
        let put = |img: &mut Vec<u8>, x: usize, y: usize, c: [u8; 3]| {
            let i = (y * tw + x) * 4;
            img[i] = c[0];
            img[i + 1] = c[1];
            img[i + 2] = c[2];
            img[i + 3] = 255;
        };
        for y in 0..th {
            for x in 0..tw {
                // 左半红、右半蓝，中间（x 20..40）画成"自己"（纯白）
                let c = if (20..40).contains(&x) {
                    [255, 255, 255]
                } else if x < 20 {
                    [200, 30, 30]
                } else {
                    [30, 30, 200]
                };
                put(&mut img, x, y, c);
            }
        }
        inpaint_self_rect(&mut img, tw, th, (20, 0, 40, th as i32));
        let at = |x: usize, y: usize| {
            let i = (y * tw + x) * 4;
            [img[i] as i32, img[i + 1] as i32, img[i + 2] as i32]
        };
        // ① 白块没了
        for y in 0..th {
            for x in 20..40 {
                let c = at(x, y);
                assert!(c.iter().all(|v| *v < 240), "({x},{y}) 还残留自己（{c:?}）");
            }
        }
        // ② 左端偏红、右端偏蓝
        let l = at(21, th / 2);
        let r = at(38, th / 2);
        assert!(l[0] > l[2], "补完的左端应当偏红：{l:?}");
        assert!(r[2] > r[0], "补完的右端应当偏蓝：{r:?}");
        // ③ 接缝连续：块内左缘（x=20）与块外左缘（x=19）颜色接近
        let inside = at(20, th / 2);
        let outside = at(19, th / 2);
        for k in 0..3 {
            assert!(
                (inside[k] - outside[k]).abs() <= 12,
                "接缝不连续：{inside:?} vs {outside:?}"
            );
        }
    }

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

    /// uv 映射的**端到端数学**：抓屏区域里"窗口那一块"必须映射回 `[0,1]²`。
    ///
    /// 这是 change_log81 真实屏幕 bug 的回归测试：上一版把**纹理像素尺寸**
    /// 当区域尺寸传进来，scale/offset 全部翻倍 ⇒ 窗口里显示的是整个桌面
    /// 被压扁的鬼影。快照测试（无真实抓屏）根本测不出来，只能在这里锁。
    #[test]
    fn uv_map_maps_window_rect_onto_unit_square() {
        // 桌面 1920×1080，窗口客户区 (400, 200, 1000, 800)，
        // 抓屏区域 = 窗口 ± margin(500)，即 (-100, -300, 2000, 1800)
        let win = (400, 200, 1000, 800);
        let cap = (-100, -300, 2000u32, 1800u32);
        let (scale, off) = uv_map_with(cap, win).expect("合法参数必须给映射");

        // 四个角必须精确落回图像的对应位置
        for (sx, sy) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
            let u = sx * scale[0] + off[0];
            let v = sy * scale[1] + off[1];
            // 屏幕点 = win 原点 + s×win 尺寸；图像坐标 = (屏幕点 - cap 原点)/cap 尺寸
            let eu = (win.0 as f32 + sx * win.2 as f32 - cap.0 as f32) / cap.2 as f32;
            let ev = (win.1 as f32 + sy * win.3 as f32 - cap.1 as f32) / cap.3 as f32;
            assert!((u - eu).abs() < 1e-5, "u({sx}) = {u} ≠ {eu}");
            assert!((v - ev).abs() < 1e-5, "v({sy}) = {v} ≠ {ev}");
        }

        // 窗口恰好等于抓屏区域时必须退化成恒等映射
        let (scale, off) = uv_map_with((10, 20, 800, 600), (10, 20, 800, 600)).unwrap();
        assert_eq!(scale, [1.0, 1.0]);
        assert_eq!(off, [0.0, 0.0]);
    }

    /// 拿**纹理像素尺寸**冒充区域尺寸必须被测试抓住（就是这次的 bug 形态）
    #[test]
    fn uv_map_with_pixel_dims_would_break_alignment() {
        let win = (400, 200, 1000, 800);
        let region = (-100, -300, 2000u32, 1800u32);
        // 正确：区域尺寸
        let (_, off_ok) = uv_map_with(region, win).unwrap();
        // 错误：把降采样 1/2 后的像素尺寸当区域尺寸
        let (_, off_bad) = uv_map_with((-100, -300, 1000, 900), win).unwrap();
        assert!(
            (off_ok[0] - off_bad[0]).abs() > 0.1,
            "两种传法的 offset 必须差得远，否则这条测试没有分辨力"
        );
    }
}
