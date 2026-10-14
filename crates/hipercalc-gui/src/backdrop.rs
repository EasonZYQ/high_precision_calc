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

    /// 反查本进程的主窗口句柄
    fn find_main_window() -> Option<Hwnd> {
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

#[cfg(not(windows))]
mod imp {
    /// 非 Windows 平台：iced 自身的 `window::Settings::blur` 已覆盖 macOS/Linux，
    /// 这里不需要做额外的事。返回 `false` 让 GUI 用不透明背景（功能性不受影响）。
    pub fn enable(_dark_titlebar: bool) -> bool {
        false
    }
}
