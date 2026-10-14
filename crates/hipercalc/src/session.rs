//! 供 GUI（`hipercalc-gui`）复用的**求值会话**。
//!
//! 设计原则：
//! - **不改行为**：这里只是把 REPL 已有的能力包一层薄公开外壳，求值路径仍走 `super::run_line`，
//!   显示模式 / 角度模式 / 变量表 / 语言都仍是同一份 `AppState` 与同一批全局开关。
//!   于是 GUI 与 CLI 的结果必然一致，不会出现"两个计算器"。
//! - **不落盘、不打印**：`Session::new` 不读 `~/.hipercalc_state` —— 这层文件是 CLI 的会话状态，
//!   GUI 有自己的配置；两者混用会让 GUI 一启动就悄悄改掉用户的 CLI 配置。
//! - **不吐颜色**：REPL 的结果串经 `colored` 着色，颜色由 `IsTerminal` 判定。GUI 不是终端，
//!   正常情况下本来就无色；但进程可能同时附着终端（从控制台启动 GUI exe），此时 `IsTerminal`
//!   为真、结果串会夹带 `\u{1b}[31m` 之类的转义。因此这里**双重防护**：
//!   进入 Session 时 `colored::control::set_override(false)`，返回前再 `strip_ansi` 兜底。

use crate::i18n::{self, Lang};
use crate::parser::{DisplayMode, Evaluator};
use crate::{AppState, handle_command, run_line, state, strip_ansi};
use hipercalc_core::calc_mode::CalcMode;
use hipercalc_core::trig::AngleMode;
use hipercalc_core::{bigfloat, calc_mode, display};
use std::collections::BTreeMap;

/// 一条输入行的求值结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineResult {
    /// 供界面展示的文本（**保证不含 ANSI 转义**，见模块头注释）。
    /// 出错时形如 `错误: xxx`（已按当前界面语言翻译）。
    pub text: String,
    /// 是否为错误（界面据此决定用错误色渲染）
    pub is_error: bool,
}

/// 整数值数制（结果怎么显示；与 `display::set_base` 同一套取值）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Base {
    Dec,
    Hex,
    Oct,
    Bin,
}

impl Base {
    /// 转成 `display::set_base` 接受的数值
    pub fn to_u32(self) -> u32 {
        match self {
            Base::Dec => 10,
            Base::Hex => 16,
            Base::Oct => 8,
            Base::Bin => 2,
        }
    }

    /// 界面上的短标签
    pub fn label(self) -> &'static str {
        match self {
            Base::Dec => "DEC",
            Base::Hex => "HEX",
            Base::Oct => "OCT",
            Base::Bin => "BIN",
        }
    }
}

/// 一个可长期存活的求值会话。GUI 持有它，替代 REPL 的 `AppState`。
pub struct Session {
    state: AppState,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    /// 新建会话：默认显示模式（MathIO）、弧度、空变量表、跟随系统语言。
    ///
    /// 刻意**不**调用 `state::load()`：GUI 与 CLI 各自持有配置，互不覆盖。
    pub fn new() -> Self {
        // 关掉 `colored` 的自动着色。理由见模块头：GUI 进程也可能附着终端。
        colored::control::set_override(false);
        // 关掉耗时行：GUI 只取结果行，混一行"用时：0.02 秒"进历史没有意义。
        //
        // ⚠️ 刻意**不**设 `calc_mode::set_quiet(true)`：
        //   所有 `/` 指令的反馈（`/let` 存下了什么、`/var` 列了哪些、`/mode` 切到哪）
        //   都走 `lprint!`，而 `quiet()` 为真时 `lprint!` 整体跳过 —— 用户敲 `/let A=7`
        //   会得到一行空文本，看起来像"没生效"。REPL 里 quiet 只用于 `/load` 回放脚本，
        //   不是"给库调用方消音"的开关。
        calc_mode::set_timing(false);
        Self {
            state: AppState {
                evaluator: Evaluator::new(),
                colors: Default::default(),
                lang_base: i18n::get(),
                timing_base: false,
            },
        }
    }

    /// 求值一行（表达式或 `/` 指令），返回展示文本。
    ///
    /// 分发规则与 REPL 的交互循环逐条对齐（**这是必需的**：`/` 指令的分发不在 `run_line` 里，
    /// 而是在 `run()` 的循环中，所以 `run_line("/let A=7")` 会把它当成表达式去解析、必然报错）：
    /// - 以 `/` 开头 → 走 `handle_command`，指令产生的反馈文本由 `lprint!` 直接打到 stdout，
    ///   **拿不到字符串**。所以这里靠"重定向 stdout"把这段输出抓回来（见 `capture_stdout`）。
    /// - 其余 → 走 `run_line`，返回值就是展示文本。
    ///
    /// 变量、`Ans`、模式都随求值更新，与 REPL 行为一致。
    pub fn eval(&mut self, line: &str) -> LineResult {
        let input = line.trim();
        // 定义域外的输入（空白行）直接回空，避免把空串喂给解析器
        if input.is_empty() {
            return LineResult {
                text: String::new(),
                is_error: false,
            };
        }
        // 静默开关是进程级的：用户可能在 GUI 里敲过 `/load`，或本进程另有一段 CLI 代码
        // 把它开着。求值前压回"不静默"，否则 `lprint!` 的内容全丢。
        calc_mode::set_quiet(false);

        if let Some(rest) = input.strip_prefix('/') {
            // `/exit`、`/q` 之类：GUI 没有"退出整个程序"的语义（用户只是敲了一行命令），
            // 这里当普通指令处理，返回空文本。
            if rest.is_empty() {
                return LineResult {
                    text: i18n::t("用法: /<指令名>，输入 /help 查看可用指令"),
                    is_error: true,
                };
            }
            let body = capture_stdout(|| {
                // 返回值（是否请求退出）GUI 不用：界面里 `/exit` 没有"关掉程序"的语义
                let _ = handle_command(input, &mut self.state);
            });
            let text = strip_ansi(&body).trim_end_matches('\n').to_string();
            return LineResult {
                // 指令自己负责提示；它没说话时给一句通用的，避免界面出现"空响应"
                is_error: false,
                text: if text.is_empty() {
                    i18n::t("（指令已执行）")
                } else {
                    text
                },
            };
        }

        let (raw, is_error) = run_line(input, &mut self.state);
        let text = strip_ansi(&raw);
        // 结果行末尾常有换行（`run_line` 里多处 `format!("…\n")`），GUI 自己排版，去掉
        let text = text.trim_end_matches('\n').to_string();
        LineResult { text, is_error }
    }

    /// 当前显示模式
    pub fn display_mode(&self) -> DisplayMode {
        self.state.evaluator.display_mode
    }

    /// 设置显示模式（MathIO = 数学式、LineIO = 线性式）
    pub fn set_display_mode(&mut self, mode: DisplayMode) {
        self.state.evaluator.display_mode = mode;
    }

    /// 当前角度模式
    pub fn angle_mode(&self) -> AngleMode {
        self.state.evaluator.angle_mode
    }

    /// 设置角度模式（弧度 / 角度）
    pub fn set_angle_mode(&mut self, mode: AngleMode) {
        self.state.evaluator.angle_mode = mode;
    }

    /// 当前计算模式（Fast = 快速、Deep = 死算）
    pub fn calc_mode(&self) -> CalcMode {
        if calc_mode::is_deep() {
            CalcMode::Deep
        } else {
            CalcMode::Fast
        }
    }

    /// 设置计算模式
    pub fn set_calc_mode(&mut self, mode: CalcMode) {
        calc_mode::set(mode);
    }

    /// 上次结果 `Ans`（供界面显示；`Number` 太底层，这里只给它的数学式文本）
    pub fn ans_text(&self) -> String {
        let e = &self.state.evaluator;
        match e.display_mode {
            DisplayMode::MathIO => display::format_mathio(&e.ans),
            DisplayMode::LineIO => display::format_lineio(&e.ans),
        }
    }

    /// 已存储的变量（`/let NAME = …`），按名字排序
    pub fn vars(&self) -> Vec<(String, String)> {
        self.state
            .evaluator
            .vars
            .iter()
            .map(|(k, v)| (k.clone(), display::format_decimal(v)))
            .collect()
    }

    /// 删除一个变量，返回是否真的删掉了
    pub fn del_var(&mut self, name: &str) -> bool {
        self.state.evaluator.vars.remove(name).is_some()
    }

    /// 当前界面语言
    pub fn lang(&self) -> Lang {
        i18n::get()
    }

    /// 切换界面语言（影响后续所有提示文案）
    pub fn set_lang(&mut self, lang: Lang) {
        i18n::set(lang);
        self.state.lang_base = lang;
    }

    /// 当前结果数制
    pub fn base(&self) -> Base {
        match display::base() {
            16 => Base::Hex,
            8 => Base::Oct,
            2 => Base::Bin,
            _ => Base::Dec,
        }
    }

    /// 设置结果数制
    pub fn set_base(&mut self, base: Base) {
        display::set_base(base.to_u32());
    }

    /// 工作精度（内部有效位数）
    pub fn precision(&self) -> usize {
        bigfloat::precision()
    }

    /// 设置工作精度（会收敛到 `[MIN_PRECISION, MAX_PRECISION]`）
    pub fn set_precision(&mut self, p: usize) {
        bigfloat::set_precision(p.clamp(crate::MIN_PRECISION, crate::MAX_PRECISION));
    }

    /// 显示的有效数字位数
    pub fn display_digits(&self) -> usize {
        bigfloat::display_digits()
    }

    /// 设置显示的有效数字位数
    pub fn set_display_digits(&mut self, n: usize) {
        bigfloat::set_display_digits(n);
    }

    /// 把当前会话写进 CLI 的状态文件（GUI 退出时调用，让命令行与 GUI 设置保持一致）
    pub fn persist(&self) -> std::io::Result<()> {
        state::save(
            self.state.evaluator.display_mode,
            self.state.evaluator.angle_mode,
            self.calc_mode(),
            self.state.lang_base,
            self.state.timing_base,
            self.precision(),
            self.display_digits(),
            bigfloat::sci_allowed(),
            bigfloat::group_enabled(),
            &self.state.evaluator.vars,
            &[],
        )
    }

    /// 变量表的只读视图（内部用；GUI 走 `vars()`）
    pub fn var_map(&self) -> &BTreeMap<String, hipercalc_core::number::Number> {
        &self.state.evaluator.vars
    }
}

/// 在"stdout 被临时重定向到管道"的状态下跑 `f`，把这段输出抓成字符串返回。
///
/// **为什么需要**：`/` 指令的反馈全部由 `lprint!` 宏直接 `println!` 到 stdout，
/// 函数签名里没有任何"返回文本"的出口。GUI 不在终端里，这些内容会消失。
/// 重定向 stdout 是唯一不改 `lib.rs` 大量打印逻辑就能拿到文本的办法。
///
/// **实现要点**（Windows 与 Unix 各一套）：
/// - Windows：`CreatePipe` + `SetStdHandle(STD_OUTPUT_HANDLE, 写端)`，
///   然后读端循环 `ReadFile` 收取；必须显式建**可继承**的管道句柄，否则子进程/新线程拿不到。
/// - Unix：`pipe(2)` + `dup2` 到 fd 1，读完 `restore`。
///
/// **风险与护栏**：
/// - 重定向期间 stdout 是进程级的，**不能并发**。GUI 是单线程求值（一次一个消息），
///   这里再用一把互斥锁兜住，避免将来有人在别的线程里调 `eval`。
/// - 无论 `f` 正常返回还是 panic，都要恢复原 stdout —— 用 `restore` 函数 + `catch_unwind`
///   保证不把进程的 stdout 永久留在管道上。
/// - 若重定向失败（权限/句柄异常），退化为"直接执行、返回空文本"，
///   绝不能因为抓不到输出就把指令本身也跳过。
fn capture_stdout<F: FnOnce()>(f: F) -> String {
    // 串行化：同一进程内不允许两段重定向区间重叠
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());

    match imp::redirect() {
        Some(mut redir) => {
            // catch_unwind：`f` 里 panic 时也必须恢复 stdout，否则整个进程的输出都没了。
            // 闭包是 FnOnce，而 catch_unwind 要 FnOnce —— 用 Option 把它"取出来用一次"
            let mut f = Some(f);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if let Some(f) = f.take() {
                    f();
                }
            }));
            let out = redir.collect_and_restore();
            if let Err(payload) = result {
                // 原样继续 panic，让上层看到真实原因；但 stdout 已经恢复
                std::panic::resume_unwind(payload);
            }
            out
        }
        None => {
            f();
            String::new()
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;

    type Handle = *mut c_void;
    const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const HANDLE_FLAG_INHERIT: u32 = 0x1;

    #[repr(C)]
    struct SecurityAttributes {
        length: u32,
        security_descriptor: *mut c_void,
        inherit_handle: i32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreatePipe(
            read: *mut Handle,
            write: *mut Handle,
            attr: *mut SecurityAttributes,
            size: u32,
        ) -> i32;
        fn SetStdHandle(which: u32, handle: Handle) -> i32;
        fn GetStdHandle(which: u32) -> Handle;
        fn SetHandleInformation(handle: Handle, mask: u32, flags: u32) -> i32;
        fn ReadFile(h: Handle, buf: *mut u8, n: u32, read: *mut u32, ovl: *mut c_void) -> i32;
        fn CloseHandle(h: Handle) -> i32;
        fn FlushFileBuffers(h: Handle) -> i32;
    }

    pub struct Redirect {
        saved: Handle,
        read: Handle,
        write: Handle,
    }

    /// 把 stdout 接到一个新管道上。返回 `None` 表示任一步失败（调用方退化执行）。
    pub fn redirect() -> Option<Redirect> {
        unsafe {
            let mut attr = SecurityAttributes {
                length: std::mem::size_of::<SecurityAttributes>() as u32,
                security_descriptor: std::ptr::null_mut(),
                inherit_handle: 1,
            };
            let (mut read, mut write) = (INVALID_HANDLE_VALUE, INVALID_HANDLE_VALUE);
            if CreatePipe(&mut read, &mut write, &mut attr, 0) == 0 {
                return None;
            }
            if read == INVALID_HANDLE_VALUE || write == INVALID_HANDLE_VALUE {
                return None;
            }
            // 读端给本进程用，不需要被子进程继承
            SetHandleInformation(read, HANDLE_FLAG_INHERIT, 0);
            let saved = GetStdHandle(STD_OUTPUT_HANDLE);
            if SetStdHandle(STD_OUTPUT_HANDLE, write) == 0 {
                CloseHandle(read);
                CloseHandle(write);
                return None;
            }
            Some(Redirect { saved, read, write })
        }
    }

    impl Redirect {
        /// 关闭写端（让读端读到 EOF）→ 收完所有输出 → 恢复原 stdout 并清理句柄。
        pub fn collect_and_restore(&mut self) -> String {
            unsafe {
                // ⚠️ 顺序很关键，写错过一次（表现为测试挂死 14 分钟）：
                //   1. 先**恢复 stdout**，再关写端。
                //      反过来的话，`CloseHandle(write)` 之后、`SetStdHandle(saved)` 之前，
                //      本进程的 STDOUT_HANDLE 仍指向那个已关闭的写端 —— 期间任何 `println!`
                //      都会失败/未定义。
                //   2. `FlushFileBuffers` 对**匿名管道**是同步等读端收走的，读端还没开始读时
                //      它会一直阻塞。所以要**先起读线程**再 flush。
                // 裸指针不是 `Send`（无法跨线程传递），但句柄只是个不透明整数 ——
                // 在**捕获进闭包之前**就转成 `usize`，闭包内再转回句柄。
                let read_addr = self.read as usize;
                let reader = std::thread::spawn(move || {
                    let read_handle = read_addr as Handle;
                    let mut out: Vec<u8> = Vec::new();
                    let mut buf = [0u8; 4096];
                    loop {
                        let mut n: u32 = 0;
                        let ok = ReadFile(
                            read_handle,
                            buf.as_mut_ptr(),
                            buf.len() as u32,
                            &mut n,
                            std::ptr::null_mut(),
                        );
                        if ok == 0 || n == 0 {
                            break;
                        }
                        out.extend_from_slice(&buf[..n as usize]);
                    }
                    CloseHandle(read_handle);
                    out
                });

                // 读线程已就位，现在 flush + 关写端，让读端读到 EOF
                FlushFileBuffers(self.write);
                CloseHandle(self.write);
                self.write = INVALID_HANDLE_VALUE;
                // 恢复 stdout（必须在关写端之后立刻做，见上面第 1 条）
                SetStdHandle(STD_OUTPUT_HANDLE, self.saved);
                self.read = INVALID_HANDLE_VALUE;

                let out = reader.join().unwrap_or_default();
                decode(&out)
            }
        }
    }

    /// 把管道里读到的字节解成字符串（UTF-8 优先，GBK 兜底，最后有损替换）
    fn decode(bytes: &[u8]) -> String {
        match std::str::from_utf8(bytes) {
            Ok(s) => s.to_string(),
            Err(_) => {
                // Windows 中文控制台默认 GBK：这里不引第三方编码库，
                // 用"按字节取可打印 ASCII、其余按 UTF-8 lossy"的务实策略，
                // 保证中文提示不整段消失（乱码好过空白）。
                String::from_utf8_lossy(bytes).into_owned()
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use std::os::fd::FromRawFd;

    pub struct Redirect {
        saved: i32,
        read: i32,
        write: i32,
    }

    /// 把 fd 1 接到新管道上
    pub fn redirect() -> Option<Redirect> {
        unsafe {
            let mut fds = [0i32; 2];
            if libc::pipe(fds.as_mut_ptr()) != 0 {
                return None;
            }
            let (read, write) = (fds[0], fds[1]);
            let saved = libc::dup(1);
            if saved < 0 {
                libc::close(read);
                libc::close(write);
                return None;
            }
            if libc::dup2(write, 1) < 0 {
                libc::close(read);
                libc::close(write);
                libc::close(saved);
                return None;
            }
            Some(Redirect { saved, read, write })
        }
    }

    impl Redirect {
        pub fn collect_and_restore(&mut self) -> String {
            use std::io::Read;
            unsafe {
                libc::close(self.write);
                self.write = -1;
                libc::dup2(self.saved, 1);
                libc::close(self.saved);

                let mut file = std::fs::File::from_raw_fd(self.read);
                let mut out = Vec::new();
                let _ = file.read_to_end(&mut out);
                self.read = -1;
                String::from_utf8_lossy(&out).into_owned()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hipercalc_core::settings;

    /// 测试隔离守卫。
    ///
    /// **为什么必须有**：`Session` 背后是一批**进程级全局开关**（角度模式、显示模式、数制、
    /// 精度、颜色开关…）。测试默认多线程、共享同一进程，任一条用例改了全局值，
    /// 就污染同进程里其它模块的测试。
    ///
    /// 这不是假设 —— 正是实际踩到的坑，而且**踩了两次**：
    ///
    /// 1. `session` 的用例把显示模式与数制改掉后，`solve_aux::tests` 那 5 条三角方程用例
    ///    （依赖默认的 MathIO + 弧度）全部变红，而它们在 `HEAD` 上是绿的。
    /// 2. 光加"用完还原"还不够 —— `solver_fit::tests::plain_fit_linear_quadratic_cubic`
    ///    偶发失败（期望 `y = x^2 + x + 1`，实际 `y = x^2 + x + 0x1`）：
    ///    本模块把数制设成 Hex 的那一瞬间，另一个线程正在读它。
    ///
    /// 所以守卫要做**两件事**：
    ///
    /// - 抢一把**跨模块的全局锁**（[`crate::lock_tests`]）—— 保证"我改全局量"期间
    ///   没有别的线程在读；
    /// - 进出时快照 / 还原全部全局设置 —— 保证用完不留痕。
    ///
    /// 只做后者（第一版的写法）挡不住竞态，只做前者则会把污染留给下一条用例。
    struct Isolated {
        settings: settings::Snapshot,
        session: Session,
        /// 持有到析构为止 —— 字段本身不用，但**不能省**：
        /// `Drop` 一跑锁就释放了，必须让它活到守卫结束
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl Isolated {
        fn new() -> Self {
            let lock = crate::lock_tests();
            let settings = settings::snapshot();
            Self {
                settings,
                session: Session::new(),
                _lock: lock,
            }
        }
    }

    impl Drop for Isolated {
        fn drop(&mut self) {
            // 把会话三模式推回默认（`Session` 自身没有"重置"入口，直接构造新的即可）
            let fresh = Session::new();
            self.session.set_display_mode(fresh.display_mode());
            self.session.set_angle_mode(fresh.angle_mode());
            self.session.set_calc_mode(fresh.calc_mode());
            // 全局设置整体还原
            settings::restore(&self.settings);
        }
    }

    /// 便捷宏：`with_session!(s => { ... })`，离开作用域自动还原全局状态。
    macro_rules! with_session {
        ($s:ident => $body:block) => {
            let mut guard = Isolated::new();
            let $s = &mut guard.session;
            $body
        };
    }

    #[test]
    fn eval_basic_value_has_no_ansi() {
        with_session!(s => {
            let r = s.eval("1+1");
            assert!(!r.is_error, "1+1 不该出错：{}", r.text);
            assert!(!r.text.contains('\u{1b}'), "结果串夹带了 ANSI 转义：{:?}", r.text);
            assert!(r.text.contains('2'), "结果里应有 2：{}", r.text);
        });
    }

    #[test]
    fn eval_result_is_single_line() {
        with_session!(s => {
            // `2^200` 在快速模式下会走科学计数法/截断路径，历史上结果行带换行
            for input in ["2^200", "sqrt(2)", "1/3", "sin(pi/6)"] {
                let r = s.eval(input);
                assert!(!r.text.ends_with('\n'), "{input} 的结果末尾有换行：{:?}", r.text);
            }
        });
    }

    #[test]
    fn eval_error_is_flagged_and_clean() {
        with_session!(s => {
            let r = s.eval("1+");
            assert!(r.is_error, "`1+` 应是错误：{}", r.text);
            assert!(!r.text.contains('\u{1b}'), "错误串夹带了 ANSI：{:?}", r.text);
        });
    }

    #[test]
    fn ans_and_vars_update() {
        with_session!(s => {
            s.eval("3*4");
            assert!(s.ans_text().contains("12"), "Ans 应变成 12：{}", s.ans_text());
            s.eval("/let A = 7");
            let vars = s.vars();
            assert!(vars.iter().any(|(k, _)| k == "A"), "变量 A 应存在：{vars:?}");
            assert!(s.del_var("A"), "删除刚建的变量应成功");
            assert!(!s.vars().iter().any(|(k, _)| k == "A"), "A 应已被删除");
        });
    }

    #[test]
    fn modes_roundtrip() {
        with_session!(s => {
            s.set_display_mode(DisplayMode::LineIO);
            assert_eq!(s.display_mode(), DisplayMode::LineIO);
            s.set_angle_mode(AngleMode::Degree);
            assert_eq!(s.angle_mode(), AngleMode::Degree);
            s.set_base(Base::Hex);
            assert_eq!(s.base(), Base::Hex);
            s.set_base(Base::Dec);
            // 精度会被夹到合法区间
            s.set_precision(1);
            assert_eq!(s.precision(), crate::MIN_PRECISION);
            s.set_precision(usize::MAX);
            assert_eq!(s.precision(), crate::MAX_PRECISION);
        });
    }

    #[test]
    fn blank_input_is_noop() {
        with_session!(s => {
            let r = s.eval("   ");
            assert!(r.text.is_empty() && !r.is_error);
        });
    }

    /// `/` 指令必须走 `handle_command` 而不是被当成表达式 ——
    /// 这是 `Session::eval` 最容易搞错的一处（`run_line` 不含指令分发）。
    #[test]
    fn slash_command_is_dispatched_not_parsed() {
        with_session!(s => {
            let r = s.eval("/let B = 2^10");
            assert!(!r.is_error, "/let 不该报错：{}", r.text);
            assert!(
                s.vars().iter().any(|(k, v)| k == "B" && v.contains("1024")),
                "B 应被存成 1024：{:?}",
                s.vars()
            );

            // `/base hex` 应真的改掉数制（证明指令副作用生效，而不只是"没报错"）。
            // 注意：输出是**小写且带前缀**的 `0xff`（见 core 的 `radix_string`），
            // 断言别写成大写 `FF` —— 那是错的第一版。
            s.eval("/base hex");
            assert_eq!(s.base(), Base::Hex, "数制应已切到十六进制");
            let r = s.eval("255");
            assert!(!r.is_error, "255 求值不该报错：{}", r.text);
            assert!(r.text.contains("0xff"), "十六进制下 255 应显示 0xff：{}", r.text);
        });
    }

    /// 未知指令必须给出可见反馈（不能是空串），否则界面看起来"点了没反应"
    #[test]
    fn unknown_command_gives_visible_feedback() {
        with_session!(s => {
            let r = s.eval("/nosuchcmd");
            assert!(!r.text.trim().is_empty(), "未知指令必须给出文字反馈");
            assert!(!r.text.contains('\u{1b}'), "反馈里夹带了 ANSI：{:?}", r.text);
        });
    }

    /// 指令执行前后的观测不能被破坏：stdout 重定向结束后必须还原干净。
    ///
    /// 这个测试是**回归护栏** —— 第一版 `collect_and_restore` 顺序写错，
    /// 导致 stdout 永久留在管道上，整个测试进程挂死 14 分钟才被 kill。
    #[test]
    fn stdout_restored_after_command() {
        with_session!(s => {
            s.eval("/var");
            // 还原后正常表达式仍走 run_line，能正常出结果
            let r = s.eval("6*7");
            assert!(r.text.contains("42"), "重定向后求值应正常：{}", r.text);
            // 再跑一次指令，确认第二次重定向也正常（句柄没有泄漏）
            let r = s.eval("/help");
            assert!(!r.text.trim().is_empty(), "/help 应有内容");
            // 第三次：模式指令 + 表达式混合，确保反复重定向不累积问题
            s.eval("/mode lineio");
            let r = s.eval("1/2");
            assert!(!r.text.is_empty(), "混合调用后求值应正常");
        });
    }
}
