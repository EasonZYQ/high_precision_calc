//! 计算中断（Ctrl+C）。
//!
//! # 为什么需要这个模块
//!
//! rustyline 的按键处理只在 `readline` 期间生效；**计算过程中根本没有读键盘**，
//! 所以"Ctrl+C 中断当前运算"不能靠键位绑定实现，得走系统的控制台信号：
//!
//! ```text
//! 空闲（等输入）  控制台处于 rustyline 的 raw 模式 ⇒ Ctrl+C 是"按键" ⇒ rustyline 默认 Cmd::Interrupt
//!                 ⇒ ReadlineError::Interrupted ⇒ 主循环退出程序 ✓
//! 计算中          没在读键盘 ⇒ 我们的控制台处理器拿到 CTRL_C_EVENT：
//!                   · COMPUTING = true  ⇒ 置 CANCEL 并返回"已处理"（不杀进程），计算在检查点退出 ✓
//!                   · COMPUTING = false ⇒ 返回"未处理" ⇒ 走系统默认（终止进程）✓
//! ```
//!
//! 这样两种状态下的行为都符合直觉：**忙时中断、闲时退出**。
//!
//! # 检查点放在哪
//!
//! 放在"每次调用求值器入口"这个粒度上（`evaluate` / `evaluate_with_var` / `evaluate_with_vars`）
//! 以及几个自带长循环的地方（DK 求根、数值求和、数值积分的每一轮）。
//! 一次 relaxed 原子读取的代价可以忽略，而长循环每次迭代至少求值一次 ⇒ 中断足够及时。

use std::sync::atomic::{AtomicBool, Ordering};

/// 当前是否正在计算（决定 Ctrl+C 是"中断"还是"退出"）
static COMPUTING: AtomicBool = AtomicBool::new(false);
/// 用户是否要求中断当前计算
static CANCEL: AtomicBool = AtomicBool::new(false);

/// 中断时返回的错误文案（走正常错误通道，用户能看到一行"错误: …"）
pub const ERROR_CANCELLED: &str = "计算已中断";

/// 是否正在计算
pub fn is_computing() -> bool {
    COMPUTING.load(Ordering::Relaxed)
}

/// 中断判定的**纯逻辑**（两个输入都显式传入，便于单测且不碰全局）。
///
/// 单测**只测这个函数**：早期版本的测试直接改全局 `CANCEL`，并行跑时会污染
/// 其它测试（CI 上 `definite_numeric_fallback` 就是这样挂的）——
/// 全局可变状态的测试必须绕开，而不是靠"记得还原"。
fn check_with(computing: bool, cancel: bool) -> Result<(), String> {
    if computing && cancel {
        Err(ERROR_CANCELLED.to_string())
    } else {
        Ok(())
    }
}

/// 计算过程中的检查点：被中断时返回 `Err(ERROR_CANCELLED)`
pub fn check() -> Result<(), String> {
    check_with(is_computing(), CANCEL.load(Ordering::Relaxed))
}

/// 标记"正在计算"的 RAII 包装：进入时清掉上次遗留的中断标志，
/// 无论正常返回还是提前 `?` 返回错误，离开时都会复位 `COMPUTING`。
pub struct Scope;

impl Scope {
    pub fn new() -> Self {
        CANCEL.store(false, Ordering::Relaxed);
        COMPUTING.store(true, Ordering::Relaxed);
        Self
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        COMPUTING.store(false, Ordering::Relaxed);
    }
}

/// 安装控制台处理器（仅 Windows 有效；其它平台为空实现 —— CI 要在三平台编译）
#[cfg(windows)]
pub fn install_handler() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // BOOL WINAPI HandlerRoutine(DWORD ctrl_type)
        unsafe extern "system" fn handler(ctrl_type: u32) -> i32 {
            const CTRL_C_EVENT: u32 = 0;
            const CTRL_BREAK_EVENT: u32 = 1;
            if ctrl_type == CTRL_C_EVENT || ctrl_type == CTRL_BREAK_EVENT {
                if is_computing() {
                    CANCEL.store(true, Ordering::Relaxed);
                    return 1; // 已处理：不终止进程，交给检查点退出计算
                }
                return 0; // 空闲：走默认行为（终止进程）⇒ 用户看到的就是"退出"
            }
            0
        }
        unsafe extern "system" {
            fn SetConsoleCtrlHandler(
                handler: Option<unsafe extern "system" fn(u32) -> i32>,
                add: i32,
            ) -> i32;
        }
        // SAFETY: 传入的是本模块内 'static 的函数指针；进程生命周期内一直有效。
        unsafe {
            SetConsoleCtrlHandler(Some(handler), 1);
        }
    });
}

/// 非 Windows：rustyline 的 raw 模式已经处理了空闲时的 Ctrl+C；
/// 计算中的中断暂不支持（留待有需求时按各平台的终端信号补齐）。
#[cfg(not(windows))]
pub fn install_handler() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_is_pure_and_only_fires_while_computing() {
        // 空闲时即使标志被置上也必须放行：否则空闲的 Ctrl+C 会把程序变成中途退出
        assert!(check_with(false, false).is_ok());
        assert!(check_with(false, true).is_ok(), "空闲态不该被中断");
        assert!(check_with(true, false).is_ok());
        assert_eq!(check_with(true, true).unwrap_err(), ERROR_CANCELLED);
    }

    #[test]
    fn scope_marks_computing_and_resets() {
        assert!(!is_computing());
        {
            let _s = Scope::new();
            assert!(is_computing());
        }
        assert!(
            !is_computing(),
            "离开作用域必须复位，否则空闲时 Ctrl+C 会不退出"
        );
    }
}
