//! 图形界面里的「命令行版」：**起独立进程**、以及**带参数时自己就当 CLI 跑**。
//!
//! ## 为什么要"独立进程"而不是线程
//!
//! 用户的要求是「点击一次启用一个 cli，gui 关闭后 cli 不受影响」——
//! 同一进程里开线程做不到这点（GUI 退出 = 进程退出 = 线程一起没）。
//! 所以这里**重新启动自己这个 exe**：GUI 二进制里已经链进了整套 CLI 逻辑
//! （`hipercalc::run()`），于是"打包进 GUI"是天然的，不需要再带一个 exe。
//!
//! ## 为什么 Windows 上要处理控制台
//!
//! `main.rs` 里有 `windows_subsystem = "windows"`（否则双击会先弹黑框），
//! 这个属性是**整个进程**的：同一个 exe 当 CLI 用时，系统不给它控制台 ——
//! `println!` 出来的东西没人接。所以两处都要补：
//!
//! - **子进程**：用 `CREATE_NEW_CONSOLE` 启动 ⇒ 新窗口，输出可见，且与 GUI 无关；
//! - **自己带参数跑**：先看有没有可用的标准输出 ——
//!   **有就不动**（用户可能在用 `> out.txt` 重定向，动 handle 会把管道弄坏），
//!   没有才 `AttachConsole`（父进程是终端时）或 `AllocConsole`（双击/计划任务时）。

use std::io;

/// 当前进程是否应该**按 CLI 跑**：只要带了任何命令行参数就算。
///
/// 为什么不区分具体参数：CLI 的开关（`-e` / `-f` / `--stdin` / `--lang` / `-q`）
/// 本来就该整段交给 CLI 解析，GUI 侧再判断一遍只会多一份会漂移的规则。
pub fn wants_cli() -> bool {
    std::env::args_os().len() > 1
}

/// 在当前进程里**直接跑 CLI**，返回退出码（带参数启动时用）。
pub fn run_cli_here() -> i32 {
    #[cfg(windows)]
    ensure_console();
    hipercalc::run()
}

/// 起一个**独立的 CLI 进程**（点「命令行」按钮时用），返回子进程 pid。
///
/// ⚠️ **Windows 上必须经 `cmd.exe` 起，不能直接起自己**（用户 2026-10-23 反馈）：
/// 直接用 `CREATE_NEW_CONSOLE` 拉起来的是**裸 conhost**，它默认不解释 ANSI 转义，
/// 屏幕上是 `[1;94m…` 这种乱码，而且没有命令行历史/补全的终端环境。
/// 走 `cmd /k` 就是"正经终端里跑程序"：彩色与输入都正常，程序退出后窗口还留着
///（用 `/k` 不用 `/c`），最后的输出不会一闪就没。
pub fn spawn_cli() -> io::Result<u32> {
    let exe = std::env::current_exe()?;

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        // 引号规则：命令串里**引号多于两个**时，cmd 会剥掉最外层一对。
        // 所以整串写成 `""<exe>" --cli"` —— 剥完剩下 `"<exe>" --cli`，带空格的路径也安全。
        //
        // ⚠️ **必须用 `raw_arg`**（用户 2026-10-27 实测报错
        // `'"...\hipercalc-gui.exe" --cli"' 不是内部或外部命令）：
        // `.arg()` 会按 MSVCRT 规则再给参数加一层引号、并把内部的 `"` 转义成 `\"`，
        // 于是 cmd 收到的是带字面反斜杠的乱码。`raw_arg` 才是"原样拼进命令行"。
        let line = format!("\"\"{}\" --cli\"", exe.display());
        let spawned = std::process::Command::new("cmd.exe")
            .arg("/k")
            .raw_arg(line)
            .creation_flags(CREATE_NEW_CONSOLE)
            .spawn();
        if let Ok(child) = spawned {
            return Ok(child.id());
        }
        // 极端情况（系统被裁剪到没有 cmd.exe）：退回直接起自己。
        // 颜色仍是对的 —— `ensure_console()` 会给控制台打开 VT 处理。
        Ok(std::process::Command::new(exe)
            .arg("--cli")
            .creation_flags(CREATE_NEW_CONSOLE)
            .spawn()?
            .id())
    }

    // 其它平台的终端本身就能解释 ANSI，直接起自己即可
    #[cfg(not(windows))]
    {
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("--cli");
        Ok(cmd.spawn()?.id())
    }
}

/// Windows：确保这个进程有地方输出（**只在原本没有标准输出时才动手**）。
#[cfg(windows)]
fn ensure_console() {
    use std::ffi::c_void;

    unsafe extern "system" {
        fn GetConsoleWindow() -> *mut c_void;
        fn GetStdHandle(which: u32) -> *mut c_void;
        fn AttachConsole(pid: u32) -> i32;
        fn AllocConsole() -> i32;
        fn SetStdHandle(which: u32, handle: *mut c_void) -> i32;
        fn GetConsoleMode(handle: *mut c_void, mode: *mut u32) -> i32;
        fn SetConsoleMode(handle: *mut c_void, mode: u32) -> i32;
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            sa: *mut c_void,
            disposition: u32,
            flags: u32,
            template: *mut c_void,
        ) -> *mut c_void;
    }

    const STD_OUTPUT_HANDLE: u32 = 0xFFFF_FFF5; // -11
    const STD_ERROR_HANDLE: u32 = 0xFFFF_FFF4; // -12
    const ATTACH_PARENT_PROCESS: u32 = 0xFFFF_FFFF;
    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const FILE_SHARE_READ_WRITE: u32 = 0x3;
    const OPEN_EXISTING: u32 = 0x3;
    const INVALID_HANDLE: isize = -1;

    /// 句柄"有值"吗（0 与 -1 都算无效）
    fn valid(h: *mut c_void) -> bool {
        let v = h as isize;
        v != 0 && v != INVALID_HANDLE
    }

    /// 让这个控制台**解释 ANSI 转义**（`\e[1;94m` 这类彩色码）。
    ///
    /// 不做这一步，彩色输出就原样显示成 `[1;94m…` —— 用户截图里正是这个。
    /// `GetConsoleMode` 对管道/文件会失败 ⇒ 天然做到"只在真控制台上动手"，
    /// 因此 `> out.txt` 重定向时这里自动跳过，不会往文件里写垃圾。
    fn enable_vt(handle: *mut c_void) {
        const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;
        let mut mode: u32 = 0;
        // SAFETY: 句柄来自 Win32，失败只是返回 0，不会越界
        let ok = unsafe { GetConsoleMode(handle, &mut mode) };
        let mut after = mode;
        if ok != 0 && (mode & ENABLE_VIRTUAL_TERMINAL_PROCESSING) == 0 {
            // SAFETY: 同上，只是把同一位打开
            let _ = unsafe { SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) };
            let mut m2: u32 = 0;
            if unsafe { GetConsoleMode(handle, &mut m2) } != 0 {
                after = m2;
            }
        }
        // 排查用探针（`HIPERCALC_CONSOLE_PROBE=1`）：把模式位落到文件里。
        // 控制台本身可能显示不出东西（就是要修的那个问题），所以不能 `eprintln!`。
        if std::env::var("HIPERCALC_CONSOLE_PROBE").ok().as_deref() == Some("1") {
            let line = format!(
                "handle={handle:?} is_console={ok} mode_before={mode:#010x} mode_after={after:#010x} vt={}\n",
                after & ENABLE_VIRTUAL_TERMINAL_PROCESSING != 0
            );
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(std::env::temp_dir().join("hc_console_probe.txt"))
            {
                use std::io::Write;
                let _ = f.write_all(line.as_bytes());
            }
        }
    }

    // SAFETY: 全部是 Win32 的标准调用，参数都是常量或本函数内构造的缓冲；
    // `name` 的 UTF-16 缓冲在调用期间一直存活。
    unsafe {
        let have_out = valid(GetStdHandle(STD_OUTPUT_HANDLE));
        let have_err = valid(GetStdHandle(STD_ERROR_HANDLE));
        // 已经有输出（终端继承了句柄，或被重定向到文件/管道）⇒ **不动句柄**，
        // 但要**顺手开 VT**：从 cmd 继承来的控制台同样可能没开，
        // 那样彩色会退化成 `[1;94m` 乱码。重定向时 `GetConsoleMode` 会失败 ⇒ 自动跳过。
        if have_out && have_err {
            enable_vt(GetStdHandle(STD_OUTPUT_HANDLE));
            enable_vt(GetStdHandle(STD_ERROR_HANDLE));
            return;
        }
        if GetConsoleWindow().is_null() {
            // 父进程是终端 ⇒ 借它的窗口；否则新开一个
            if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
                let _ = AllocConsole();
            }
        }
        // 借来的控制台不会自动成为本进程的标准输出 ⇒ 自己打开 CONOUT$ 接上
        let name: Vec<u16> = "CONOUT$\0".encode_utf16().collect();
        let h = CreateFileW(
            name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ_WRITE,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        );
        if valid(h) {
            if !have_out {
                SetStdHandle(STD_OUTPUT_HANDLE, h);
            }
            if !have_err {
                SetStdHandle(STD_ERROR_HANDLE, h);
            }
            // 新开/借来的控制台默认**不开** VT 处理 ⇒ 必须显式开，否则就是乱码
            enable_vt(h);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 无参数 ⇒ GUI；带参数 ⇒ CLI。规则很简单，但它决定了**双击 exe 会不会变成命令行**，
    /// 值得钉住。
    #[test]
    fn wants_cli_follows_argument_count_only() {
        // 这条用例只能在"测试进程确实没有额外参数"时验证 $0 分支，
        // 所以只断言"不 panic 且与 args 数量一致"这个等价关系。
        let by_args = std::env::args_os().len() > 1;
        assert_eq!(wants_cli(), by_args);
    }

    /// `spawn_cli` 至少要能拿到自己的路径（真起进程留给人工验证 ——
    /// CI 里开控制台窗口没有意义，还可能卡住 runner）。
    #[test]
    fn current_exe_is_available() {
        assert!(std::env::current_exe().is_ok());
    }
}
