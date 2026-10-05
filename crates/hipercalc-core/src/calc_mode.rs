//! 计算模式（进程级开关）
//!
//! - `Fast`（默认）：保留规模保护——`exp`/幂/三角/阶乘/科学计数法指数等超出上限时直接返回提示，
//!   输出限 20 位有效数字（大数用科学计数法）；因式分解的候选枚举有预算，超预算时提示"结果可能不完整"。
//! - `Deep`（死算）：取消上述规模上限，完整精度输出（不截断、不用科学计数法）。
//!   极端输入可能长时间运行、产生超长输出甚至耗尽内存，由用户自行承担。
//!
//! 为什么用全局开关：这些护栏分散在 `bigfloat.rs` / `number.rs` / `parser.rs` / `solver_factor.rs` 的
//! 低层数值函数里，它们拿不到 `Evaluator` 上下文。REPL 是单线程执行运算，因此用原子布尔量同步即可；
//! 计时线程只读取时间，不访问该状态。
//!
//! 注意：`Deep` 只放开**规模**上限，不放开牛顿迭代的发散保护
//! （`NEWTON_ABS_LIMIT`），否则 `2^x=8` 这类方程会回到卡死状态。

use std::sync::atomic::{AtomicBool, Ordering};

/// 计算模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalcMode {
    /// 快速模式：超限直接提示，输出限 20 位有效数字
    Fast,
    /// 死算模式：不设规模上限，完整精度输出
    Deep,
}

/// 当前是否处于死算模式（默认 Fast）
static DEEP: AtomicBool = AtomicBool::new(false);

/// 是否显示耗时（默认开）。关掉后不再打印"用时：N秒"行，便于脚本/断言
static TIMING: AtomicBool = AtomicBool::new(true);

/// 静默输出开关（`/load` 回放脚本时把命令自身的提示压掉，只在末尾给一行汇总）
static QUIET: AtomicBool = AtomicBool::new(false);

/// 当前是否处于静默输出状态（`lprint!` 等输出宏会据此跳过打印）
pub fn quiet() -> bool {
    QUIET.load(Ordering::Relaxed)
}

/// 设置静默输出
pub fn set_quiet(on: bool) {
    QUIET.store(on, Ordering::Relaxed);
}

/// 是否显示耗时行
pub fn timing_enabled() -> bool {
    TIMING.load(Ordering::Relaxed)
}

/// 设置是否显示耗时行（`/timing on|off`）
pub fn set_timing(on: bool) {
    TIMING.store(on, Ordering::Relaxed);
}

/// 设置计算模式
pub fn set(mode: CalcMode) {
    DEEP.store(mode == CalcMode::Deep, Ordering::Relaxed);
}

/// 是否死算模式（低层护栏的开关判据）
pub fn is_deep() -> bool {
    DEEP.load(Ordering::Relaxed)
}

/// 模式名（用于状态显示）
pub fn name() -> &'static str {
    if is_deep() { "Deep" } else { "Fast" }
}
