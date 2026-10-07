//! 会话设置：把原先散在 `bigfloat` / `calc_mode` / `display` 三个模块里的 **10 个全局静变量**收在一处。
//!
//! # 为什么要收敛
//!
//! 这些开关**互相配合**（"死算模式 + 内部精度 + 显示位数 + 结果单位"其实是一组语义），
//! 散成 10 个原子量有三个实际问题：
//! 1. **看不出全貌** —— 想知道"一共有哪些设置"得翻三个文件；
//! 2. **读不到一致快照** —— 外部无法原子地"暂存→改动→还原"，这也是本项目测试里反复踩的坑
//!    （测试为了不被全局污染，只能用 `st.evaluator.display_mode = …` 绕开 `/mode`）；
//! 3. 新增设置没有明确归属。
//!
//! # 为什么不改成"一个锁"
//!
//! `is_deep()` / `precision()` 在**热路径**上（每次运算都会查），所以内部**仍然是原子量**
//! ——收敛的是**归属与可发现性**，不是把原子换成锁（那会拖慢每次运算）。
//! 各模块原有的 `set_*` / 读取函数**签名不变**，只是改为读写这里的字段 ⇒ 调用点一处不动。
//!
//! 注：`cancel.rs` 的 `COMPUTING` / `CANCEL` **不在这里** —— 它们是"正在算/已请求取消"的**瞬时状态**，
//! 不是用户设置；语义不同，刻意分开。

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

/// 内部计算精度（十进制位）
pub static PRECISION: AtomicUsize = AtomicUsize::new(80);
/// 显示的有效数字位数
pub static DISPLAY_DIGITS: AtomicUsize = AtomicUsize::new(20);
/// 结果是否允许科学计数法
pub static SCI_ALLOWED: AtomicBool = AtomicBool::new(true);
/// 整数是否按千分位分组
pub static GROUP_DIGITS: AtomicBool = AtomicBool::new(false);
/// 是否死算模式（取消规模上限）
pub static DEEP: AtomicBool = AtomicBool::new(false);
/// 是否显示耗时
pub static TIMING: AtomicBool = AtomicBool::new(true);
/// 是否静默（不输出提示性文字）
pub static QUIET: AtomicBool = AtomicBool::new(false);
/// 结果单位（标签 + 折成 SI 的系数）
pub static UNIT: std::sync::Mutex<Option<(String, num_rational::BigRational)>> =
    std::sync::Mutex::new(None);
/// 是否输出 LaTeX
pub static LATEX: AtomicBool = AtomicBool::new(false);
/// 结果数制（10/16/8/2）
pub static RESULT_BASE: AtomicU32 = AtomicU32::new(10);

/// 设置的**整体快照**：给测试用 —— 测试可以"存一份 → 随意改动 → 还原"，
/// 从此不必再靠"绕开全局开关"来避免互相污染。
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub precision: usize,
    pub display_digits: usize,
    pub sci_allowed: bool,
    pub group_digits: bool,
    pub deep: bool,
    pub timing: bool,
    pub quiet: bool,
    pub unit: Option<(String, num_rational::BigRational)>,
    pub latex: bool,
    pub base: u32,
}

/// 取当前全部设置
pub fn snapshot() -> Snapshot {
    Snapshot {
        precision: PRECISION.load(Ordering::Relaxed),
        display_digits: DISPLAY_DIGITS.load(Ordering::Relaxed),
        sci_allowed: SCI_ALLOWED.load(Ordering::Relaxed),
        group_digits: GROUP_DIGITS.load(Ordering::Relaxed),
        deep: DEEP.load(Ordering::Relaxed),
        timing: TIMING.load(Ordering::Relaxed),
        quiet: QUIET.load(Ordering::Relaxed),
        unit: UNIT.lock().ok().and_then(|g| g.clone()),
        latex: LATEX.load(Ordering::Relaxed),
        base: RESULT_BASE.load(Ordering::Relaxed),
    }
}

/// 还原一份设置
pub fn restore(s: &Snapshot) {
    PRECISION.store(s.precision, Ordering::Relaxed);
    DISPLAY_DIGITS.store(s.display_digits, Ordering::Relaxed);
    SCI_ALLOWED.store(s.sci_allowed, Ordering::Relaxed);
    GROUP_DIGITS.store(s.group_digits, Ordering::Relaxed);
    DEEP.store(s.deep, Ordering::Relaxed);
    TIMING.store(s.timing, Ordering::Relaxed);
    QUIET.store(s.quiet, Ordering::Relaxed);
    if let Ok(mut g) = UNIT.lock() {
        *g = s.unit.clone();
    }
    LATEX.store(s.latex, Ordering::Relaxed);
    RESULT_BASE.store(s.base, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **锁住 10 个设置的默认值**。
    ///
    /// 这条测试是在收敛过程中补上的：我把 `GROUP_DIGITS` 的默认值凭印象写成了 `true`，
    /// 而原值是 `false`（千分位默认关闭）—— 这类"初始化值写错"属于**静默行为变更**，
    /// 既有测试未必覆盖得到。逐项钉死，以后改动默认值必须显式来这里改。
    #[test]
    fn defaults_are_stable() {
        let d = snapshot();
        assert_eq!(d.precision, 80);
        assert_eq!(d.display_digits, 20);
        assert!(d.sci_allowed);
        assert!(!d.group_digits, "千分位默认关闭");
        assert!(!d.deep, "默认快速模式");
        assert!(d.timing);
        assert!(!d.quiet);
        assert_eq!(d.unit, None, "默认不套单位");
        assert!(!d.latex, "默认 MathIO 而非 LaTeX");
        assert_eq!(d.base, 10, "默认十进制");
    }

    /// 快照/还原：这是收敛带来的**新能力**（原先 10 个分散原子量做不到整体存取）
    #[test]
    fn snapshot_restore_roundtrip() {
        let saved = snapshot();
        PRECISION.store(123, Ordering::Relaxed);
        DEEP.store(true, Ordering::Relaxed);
        LATEX.store(true, Ordering::Relaxed);
        RESULT_BASE.store(16, Ordering::Relaxed);
        restore(&saved);
        assert_eq!(snapshot(), saved);
    }
}
