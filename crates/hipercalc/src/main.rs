mod bigfloat;
mod bigint_ext;
mod calc_mode;
mod complex;
mod display;
mod equation;
mod i18n;
mod number;
mod parser;
mod primefac;
mod calculus;
mod cancel;
mod language;
mod solver_factor;
mod solver_linear;
mod solver_nonlinear;
mod solver_fit;
mod solver_poly;
mod solver_triangle;
mod solve_aux;
mod state;
mod trig;

use colored::*;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::Signed;
use number::Number;
use parser::{parse_and_eval, DisplayMode, EvalResult, Evaluator};
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::{Hint, Hinter};
use rustyline::validate::Validator;
use rustyline::history::DefaultHistory;
use rustyline::line_buffer::LineBuffer;
use rustyline::{
    Cmd, ConditionalEventHandler, Context, Editor, Event, EventContext, EventHandler, Helper,
    KeyEvent, RepeatCount, Result as RlResult,
};
use rustyline::Changeset;
use std::cell::Cell;
use std::io::{IsTerminal, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// 全部指令（Tab 补全共用；新增指令时同时改这里与 `handle_command`）
const COMMANDS: &[&str] = &[
    "/help", "/clear", "/mode", "/lang", "/language", "/timing", "/set", "/let", "/var", "/del",
    "/reset", "/save", "/load", "/exit", "/quit", "/q",
];

/// 指令的内联用法提示（键为指令全名；文案会经 `i18n::t` 翻译）
const COMMAND_HINTS: &[(&str, &str)] = &[
    ("/mode", "mathio|lineio|deg|rad|fast|deep|prec|digits|sci|group"),
    ("/lang", "zh-CN|zh-TW|en（无参数进入选择菜单）"),
    ("/timing", "on|off"),
    ("/set", "<类别> <颜色>"),
    ("/let", "NAME = <表达式>（变量名须全大写）"),
    ("/del", "NAME 或 all"),
    ("/reset", "[all]（all = 连模式与颜色一起恢复默认）"),
    ("/save", "<文件路径>"),
    ("/load", "<文件路径>"),
];

/// 函数元数据：**元数 + 参数签名**，供 Tab 补全提示与"括号内参数提示"使用。
///
/// 这是**唯一**的函数元数据表；函数名单一真源仍是 `parser::FUNCTIONS`，
/// 由单测 `functions_meta_covers_parser_functions` 双向抽检，防止两处漂移。
///
/// - `min` / `max`：允许的参数个数区间（用于"参数不足/过多"提示）；`triangle` 是命名参数特例，
///   不参与元数校验（`max` 取 `usize::MAX` 即永不报"过多"）。
/// - `sig`：参数签名文案；纯 ASCII 的签名在词条表里查不到就原样输出（也是允许的）。
struct FnMeta {
    name: &'static str,
    min: usize,
    max: usize,
    sig: &'static str,
}

const FUNCTIONS_META: &[FnMeta] = &[
    // 单参
    FnMeta { name: "sqr", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "sqrt", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "sin", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "cos", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "tan", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "cot", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "sec", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "csc", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "arcsin", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "arccos", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "arctan", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "arccot", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "arcsec", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "arccsc", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "abs", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "sd", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "factor", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "fac", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "ln", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "exp", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "log10", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "log2", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "sinh", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "cosh", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "tanh", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "coth", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "sech", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "csch", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "arcsinh", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "arccosh", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "arctanh", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "cbrt", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "re", min: 1, max: 1, sig: "(z)" },
    FnMeta { name: "im", min: 1, max: 1, sig: "(z)" },
    FnMeta { name: "conj", min: 1, max: 1, sig: "(z)" },
    FnMeta { name: "arg", min: 1, max: 1, sig: "(z)" },
    FnMeta { name: "isprime", min: 1, max: 1, sig: "(n)" },
    FnMeta { name: "nextprime", min: 1, max: 1, sig: "(n)" },
    FnMeta { name: "primefac", min: 1, max: 1, sig: "(n)" },
    FnMeta { name: "floor", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "ceil", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "round", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "frac", min: 1, max: 1, sig: "(x)" },
    FnMeta { name: "sign", min: 1, max: 1, sig: "(x)" },
    // 双参
    FnMeta { name: "log", min: 2, max: 2, sig: "(base, x)" },
    FnMeta { name: "nroot", min: 2, max: 2, sig: "(x, n)" },
    FnMeta { name: "mod", min: 2, max: 2, sig: "(a, b)" },
    FnMeta { name: "idiv", min: 2, max: 2, sig: "(a, b)" },
    FnMeta { name: "nCr", min: 2, max: 2, sig: "(n, r)" },
    FnMeta { name: "nPr", min: 2, max: 2, sig: "(n, r)" },
    FnMeta { name: "gcd", min: 2, max: 2, sig: "(a, b)" },
    FnMeta { name: "lcm", min: 2, max: 2, sig: "(a, b)" },
    // 高等数学
    FnMeta { name: "diff", min: 2, max: 2, sig: "(f, x)" },
    FnMeta { name: "lim", min: 3, max: 3, sig: "(f, x, a)" },
    FnMeta { name: "int", min: 2, max: 4, sig: "(f, x) 或 (f, x, a, b)" },
    FnMeta { name: "taylor", min: 4, max: 4, sig: "(f, x, a, n)" },
    FnMeta { name: "sum", min: 4, max: 4, sig: "(f, k, a, b)" },
    FnMeta { name: "prod", min: 4, max: 4, sig: "(f, k, a, b)" },
    // 命名参数特例：不做元数校验（a/b/c/A/B/C 任意组合，可逗号可空白）
    FnMeta { name: "triangle", min: 0, max: usize::MAX, sig: "(a, b, c, A, B, C)" },
];

/// 全角 → 半角映射：中文输入法下最容易打出的全角标点（`。` 是最高频痛点）。
/// 只做符号与数字，**不做全角字母**（会干扰变量名的单字母/全大写规则）。
const FULLWIDTH_MAP: &[(char, &str)] = &[
    ('。', "."), ('．', "."), ('，', ","), ('；', ";"), ('：', ":"),
    ('（', "("), ('）', ")"), ('［', "["), ('］', "]"),
    ('＝', "="), ('＋', "+"), ('－', "-"), ('×', "*"), ('÷', "/"),
    ('０', "0"), ('１', "1"), ('２', "2"), ('３', "3"), ('４', "4"),
    ('５', "5"), ('６', "6"), ('７', "7"), ('８', "8"), ('９', "9"),
];

/// 在 `FUNCTIONS_META` 里查函数元数据
fn fn_meta(name: &str) -> Option<&'static FnMeta> {
    FUNCTIONS_META.iter().find(|m| m.name == name)
}

/// 光标处（或其左邻）的括号 → `(自身下标, 配对下标)`；没括号或找不到配对返回 None。
fn match_bracket_pair(line: &str, pos: usize) -> Option<(usize, usize)> {
    let chars: Vec<char> = line.chars().collect();
    let idx = if pos < chars.len() && matches!(chars[pos], '(' | ')') {
        pos
    } else if pos > 0 && matches!(chars[pos - 1], '(' | ')') {
        pos - 1
    } else {
        return None;
    };
    let forward = chars[idx] == '(';
    let (open, close) = if forward { ('(', ')') } else { (')', '(') };
    let mut depth = 0i32;
    if forward {
        for (j, c) in chars.iter().enumerate().skip(idx) {
            if *c == open {
                depth += 1;
            } else if *c == close {
                depth -= 1;
                if depth == 0 {
                    return Some((idx, j));
                }
            }
        }
    } else {
        for j in (0..=idx).rev() {
            if chars[j] == open {
                depth += 1;
            } else if chars[j] == close {
                depth -= 1;
                if depth == 0 {
                    return Some((idx, j));
                }
            }
        }
    }
    None
}

/// 需要标红的括号下标：
/// - **多余的 `)` 一律标红**（永远是明显不合法）；
/// - **未闭合的 `(` 只在光标不在其右侧时标红** —— 正在输入参数（`sin(1+` 光标在末尾）时不标，
///   避免打字全程满屏红；光标移开或回车前才提示。
fn unmatched_brackets(line: &str, pos: usize) -> Vec<usize> {
    let chars: Vec<char> = line.chars().collect();
    let mut stack: Vec<usize> = Vec::new();
    let mut bad: Vec<usize> = Vec::new();
    for (i, c) in chars.iter().enumerate() {
        match c {
            '(' => stack.push(i),
            ')' => {
                if stack.pop().is_none() {
                    bad.push(i);
                }
            }
            _ => {}
        }
    }
    for o in stack {
        if pos <= o {
            bad.push(o);
        }
    }
    bad.sort_unstable();
    bad
}

/// 定位"包裹住光标的函数调用"：返回 `(函数名, 开括号下标, 已输入的一级参数个数)`。
///
/// 用于"光标在函数括号内时提示参数"。一级参数个数 = 一级逗号数 + 1（括号内为空则 0）；
/// 统计时忽略嵌套括号与 `|…|` 内的逗号。
fn enclosing_function_call(line: &str, pos: usize) -> Option<(String, usize, usize)> {
    let chars: Vec<char> = line.chars().collect();
    let pos = pos.min(chars.len());
    // 1) 向左找最近的"未闭合 '('"
    let mut depth = 0i32;
    let mut open = None;
    let mut i = pos;
    while i > 0 {
        i -= 1;
        match chars[i] {
            ')' => depth += 1,
            '(' => {
                if depth == 0 {
                    open = Some(i);
                    break;
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    let open = open?;
    // 2) 取 '(' 左侧紧邻标识符作为函数名
    let mut j = open;
    while j > 0 && (chars[j - 1].is_ascii_alphanumeric() || chars[j - 1] == '_') {
        j -= 1;
    }
    if j == open {
        return None; // 裸括号分组，不是函数调用
    }
    let name: String = chars[j..open].iter().collect();
    // 3) 统计 '(' 到光标之间的一级逗号数
    let mut depth = 0i32;
    let mut in_abs = false;
    let mut commas = 0usize;
    let mut has_content = false;
    for c in chars[open + 1..pos].iter() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            '|' => in_abs = !in_abs,
            ',' if depth == 0 && !in_abs => commas += 1,
            c if !c.is_whitespace() => has_content = true,
            _ => {}
        }
    }
    let argc = if !has_content && commas == 0 { 0 } else { commas + 1 };
    Some((name, open, argc))
}

/// 函数白名单（高亮用）：直接引用 `parser::FUNCTIONS`，避免多处数组不同步
const VALID_FUNCTIONS: &[&str] = parser::FUNCTIONS;
const CONSTANTS: &[&str] = &["pi", "π", "e", "tau", "phi", "φ", "i", "inf", "∞"];
/// 运算符（输入行与结果行共用）：后三个只出现在**结果**串里（`≈` 前缀、科学计数法的 `×`、
/// 通式里的 `k·π`），补进来是为了让结果行也能把运算符一并点亮。
const OPS: &[char] = &['+', '-', '*', '/', '^', '!', '=', '≈', '×', '·'];
const BRACKETS: &[char] = &['(', ')', '|'];

/// /set 可设置的类别（英文类别名, 中文标注）
const COLOR_CATEGORIES: &[(&str, &str)] = &[
    ("functions", "函数"),
    ("operators", "运算符"),
    ("commands", "指令"),
    ("brackets", "括号"),
    ("constants", "常量"),
    ("numbers", "数字"),
    ("prompt", "提示符"),
    ("result", "结果"),
    ("error", "错误"),
];

/// /set 可选颜色（英文色名, 中文标注, 颜色值）
const COLOR_OPTIONS: &[(&str, &str, Color)] = &[
    ("black", "黑", Color::Black),
    ("red", "红", Color::Red),
    ("green", "绿", Color::Green),
    ("yellow", "黄", Color::Yellow),
    ("blue", "蓝", Color::Blue),
    ("magenta", "洋红", Color::Magenta),
    ("cyan", "青", Color::Cyan),
    ("white", "白", Color::White),
    ("bright_black", "亮黑", Color::BrightBlack),
    ("bright_red", "亮红", Color::BrightRed),
    ("bright_green", "亮绿", Color::BrightGreen),
    ("bright_yellow", "亮黄", Color::BrightYellow),
    ("bright_blue", "亮蓝", Color::BrightBlue),
    ("bright_magenta", "亮洋红", Color::BrightMagenta),
    ("bright_cyan", "亮青", Color::BrightCyan),
    ("bright_white", "亮白", Color::BrightWhite),
];

/// 颜色名 → 颜色（/set 解析与状态文件加载共用）
fn parse_color(name: &str) -> Option<Color> {
    COLOR_OPTIONS
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, _, c)| *c)
}

/// 颜色 → 颜色名（写状态文件与列出当前颜色用）
fn color_name(color: Color) -> &'static str {
    COLOR_OPTIONS
        .iter()
        .find(|(_, _, c)| *c == color)
        .map(|(n, _, _)| *n)
        .unwrap_or("white")
}

/// 所有用户可见输出统一走这里：按当前语言翻译后打印（见 `i18n` 模块）。
/// **不要在 main.rs 里直接用 `println!`**，否则新文案不会随语言切换。
macro_rules! lprint {
    () => {{
        if !crate::calc_mode::quiet() {
            println!()
        }
    }};
    ($($arg:tt)*) => {{
        if !crate::calc_mode::quiet() {
            println!("{}", crate::i18n::t(&format!($($arg)*)))
        }
    }};
}

/// 同上，输出到 stderr
macro_rules! leprint {
    ($($arg:tt)*) => {{
        if !crate::calc_mode::quiet() {
            eprintln!("{}", crate::i18n::t(&format!($($arg)*)))
        }
    }};
}

/// 不换行输出（供动态计时刷新用）
macro_rules! lprint_inline {
    ($($arg:tt)*) => {{
        if !crate::calc_mode::quiet() {
            print!("{}", crate::i18n::t(&format!($($arg)*)))
        }
    }};
}

/// 动态耗时行的最大宽度（清行时用等宽空格覆盖）
const TIMING_LINE_WIDTH: usize = 24;

/// 运算计时器：
/// - **运算进行中**：在算式下一行用 `\r` 动态刷新 `用时：N秒`（整秒；仅交互式终端）；
/// - **运算结束**：先清掉动态行，随后打印的结果正好覆盖这一行；再在结果下一行打印最终耗时
///   （整秒；不足 1 秒显示 `用时：<1秒`）。
///
/// 实现要点：动态刷新跑在独立线程里，通过通道 + `recv_timeout` 实现"可立即唤醒的定时器"，
/// 结束时不额外等待（关闭通道即让线程立刻退出）。管道/重定向场景下 stdout 不是终端，
/// 直接跳过动态刷新（避免把 `\r` 混进输出），最终耗时行照常打印。
struct Timing {
    start: Instant,
    stop_tx: Option<mpsc::Sender<()>>,
    handle: Option<std::thread::JoinHandle<()>>,
    dynamic: bool,
    /// 是否已停止并清行（`stop_dynamic` 幂等，Drop 里不会重复清行）
    stopped: bool,
}

impl Timing {
    fn begin() -> Self {
        // `/timing off`（或命令行 -q）时不做任何计时，也不起刷新线程
        let dynamic = calc_mode::timing_enabled() && std::io::stdout().is_terminal();
        let start = Instant::now();
        if !dynamic {
            return Timing {
                start,
                stop_tx: None,
                handle: None,
                dynamic,
                stopped: false,
            };
        }
        let (tx, rx) = mpsc::channel::<()>();
        let handle = std::thread::spawn(move || {
            let mut shown: u64 = u64::MAX;
            loop {
                // 先刷新（首次立即出现，之后每秒变化一次），再等信号/超时
                let secs = start.elapsed().as_secs();
                if secs != shown {
                    shown = secs;
                    lprint_inline!("\r用时：{}秒", secs);
                    let _ = std::io::stdout().flush();
                }
                match rx.recv_timeout(Duration::from_millis(100)) {
                    // 收到停止信号或通道关闭 ⇒ 退出
                    Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
        });
        Timing {
            start,
            stop_tx: Some(tx),
            handle: Some(handle),
            dynamic,
            stopped: false,
        }
    }

    /// 停止动态刷新并清掉动态行（让后续打印的结果覆盖该行）；可重复调用
    fn stop_dynamic(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        if let Some(tx) = self.stop_tx.take() {
            drop(tx); // 关闭通道 ⇒ 刷新线程的 recv_timeout 立即返回并退出
        }
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        if self.dynamic {
            print!("\r{:width$}\r", "", width = TIMING_LINE_WIDTH);
            let _ = std::io::stdout().flush();
        }
    }

    /// 最终耗时文本：整秒；不足 1 秒显示 `用时：<1秒`；关闭计时显示时返回空串
    fn elapsed_text(&self) -> String {
        if !calc_mode::timing_enabled() {
            return String::new();
        }
        let secs = self.start.elapsed().as_secs();
        if secs < 1 {
            "用时：<1秒".to_string()
        } else {
            format!("用时：{}秒", secs)
        }
    }
}

impl Drop for Timing {
    fn drop(&mut self) {
        self.stop_dynamic();
    }
}

#[derive(Clone)]
struct ColorConfig {
    function: Color,
    operator: Color,
    command: Color,
    bracket: Color,
    constant: Color,
    number: Color,
    prompt: Color,
    result: Color,
    error: Color,
}

impl Default for ColorConfig {
    fn default() -> Self {
        ColorConfig {
            function: Color::Green,
            operator: Color::Yellow,
            command: Color::Cyan,
            bracket: Color::Magenta,
            constant: Color::Blue,
            number: Color::White,
            prompt: Color::BrightBlue,
            result: Color::BrightWhite,
            error: Color::Red,
        }
    }
}

impl ColorConfig {
    /// 按类别名设置颜色；类别未知返回 false（/set 与状态文件加载共用）
    fn set_category(&mut self, category: &str, color: Color) -> bool {
        match category {
            "functions" => self.function = color,
            "operators" => self.operator = color,
            "commands" => self.command = color,
            "brackets" => self.bracket = color,
            "constants" => self.constant = color,
            "numbers" => self.number = color,
            "prompt" => self.prompt = color,
            "result" => self.result = color,
            "error" => self.error = color,
            _ => return false,
        }
        true
    }

    /// 按类别名取当前颜色（/set 列出当前设置用；类别未知回退白色）
    fn get_category(&self, category: &str) -> Color {
        match category {
            "functions" => self.function,
            "operators" => self.operator,
            "commands" => self.command,
            "brackets" => self.bracket,
            "constants" => self.constant,
            "numbers" => self.number,
            "prompt" => self.prompt,
            "result" => self.result,
            "error" => self.error,
            _ => Color::White,
        }
    }

    /// 全部类别 → 颜色名（写状态文件用）
    fn to_pairs(&self) -> Vec<(&'static str, &'static str)> {
        COLOR_CATEGORIES
            .iter()
            .map(|(cat, _)| (*cat, color_name(self.get_category(cat))))
            .collect()
    }
}

struct AppState {
    evaluator: Evaluator,
    colors: ColorConfig,
    /// 状态文件里应保存的界面语言（**不含** 命令行 `--lang` 的临时覆盖：
    /// 覆盖只作用于本次会话，不该写进用户配置）
    lang_base: i18n::Lang,
    /// 状态文件里应保存的耗时显示开关（**不含** 命令行 `-q`/`--no-timing` 的临时覆盖：
    /// 它只作用于本次会话，不该把用户的 `/timing on` 配置改成 off）
    timing_base: bool,
}

/// Tab 补全候选（纯函数，便于单元测试）：
/// - token 以 `/` 开头 → 只补指令；
/// - 否则补多字母函数名、常数与存储变量名；
/// - **单字母 token 一律不给候选**：多字母拆分场景（`xy` 表示 x·y）不会被补成函数名，
///   从根上避免补全破坏隐式乘法。
fn completion_candidates(token: &str, vars: &[String]) -> Vec<String> {
    if token.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<String> = Vec::new();
    if token.starts_with('/') {
        out.extend(
            COMMANDS
                .iter()
                .filter(|c| c.starts_with(token))
                .map(|c| (*c).to_string()),
        );
    } else if token.chars().count() > 1 {
        out.extend(
            parser::FUNCTIONS
                .iter()
                .filter(|f| f.len() > 1 && !matches!(**f, "fact" | "abs") && f.starts_with(token))
                .map(|f| (*f).to_string()),
        );
        out.extend(
            CONSTANTS
                .iter()
                .filter(|c| c.starts_with(token))
                .map(|c| (*c).to_string()),
        );
        out.extend(vars.iter().filter(|v| v.starts_with(token)).cloned());
    }
    out.sort();
    out.dedup();
    out
}

/// 内联提示文本（纯函数，便于单元测试）：
/// - 行首 `/xxx`（还没输入空格）→ 该指令的用法；
/// - 光标左侧正好是一个函数名 → 参数签名（如 `log` 后面提示 `(base, x)`）。
fn inline_hint(line: &str) -> Option<String> {
    let t = line.trim_start();
    if t.starts_with('/') && !t.chars().any(char::is_whitespace) {
        return COMMAND_HINTS
            .iter()
            .find(|(c, _)| *c == t)
            .map(|(_, h)| format!("  {}", i18n::t(h)));
    }
    let chars: Vec<char> = line.chars().collect();
    let mut start = chars.len();
    while start > 0 && (chars[start - 1].is_ascii_alphanumeric() || chars[start - 1] == '_') {
        start -= 1;
    }
    if start == chars.len() {
        return None;
    }
    let ident: String = chars[start..].iter().collect();
    // 签名经 `i18n::t`：`int` 的签名含中文，英/繁界面必须翻译
    // （旧实现直接返回原文，英/繁模式下会残留中文）
    fn_meta(&ident).map(|m| i18n::t(m.sig))
}

/// 补全要替换的区间与**光标落点**（纯函数，便于单测）。
///
/// - 区间固定为 `start..orig_pos`：`orig_pos` 是 `complete()` 时的光标位置。
///   **不能**用当前 `line.pos()` —— 我们随后会把光标挪进括号，循环补全时读它就会越替换越短。
/// - 落点：候选以 `)` 结尾（函数补成 `name()`）时落在**右括号之前** ⇒ 光标在括号内；
///   其余（指令/常数/变量）落在替换文本末尾。
fn completion_edit(
    start: usize,
    orig_pos: usize,
    elected: &str,
) -> (std::ops::Range<usize>, usize) {
    let len = elected.chars().count();
    let pos = if elected.ends_with(')') && len > 0 {
        start + len - 1
    } else {
        start + len
    };
    (start..orig_pos.max(start), pos)
}

/// 把签名 `(f, x, a, b)` 拆成参数名列表（去掉括号与空白）
fn sig_params(sig: &str) -> Vec<&str> {
    sig.trim_matches(|c| c == '(' || c == ')')
        .split(',')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect()
}

/// 已输入 `argc` 个参数后，右侧提示应当只显示**剩下还没写的参数**，末尾补 `)`。
/// 例：`log` 的签名是 `(base, x)`，刚敲完 `log(`（argc=0）提示 `base, x)`；
/// 敲完第一个参数（argc=1）提示只剩 `x)`（即"每输入一个参数就去掉该参数的提示"）。
fn remaining_args_hint(sig: &str, argc: usize) -> String {
    let params = sig_params(sig);
    if argc >= params.len() {
        return ")".to_string();
    }
    format!("{})", params[argc..].join(", "))
}

/// 本帧参数的提示种类（决定右侧提示的颜色）。
/// 之所以用 `Cell` 而不是把颜色塞进 `display()`：`display()` 参与终端宽度计算，**不能含 ANSI**。
#[derive(Clone, Copy, PartialEq, Eq)]
enum HintKind {
    /// 参数个数正常；或只是指令/签名的普通提示
    Normal,
    /// 参数个数不足
    Incomplete,
    /// 参数个数过多（标红）
    TooMany,
}

/// 右侧内联提示：**只显示、不补全**。
///
/// `Hint::completion()` 的默认实现返回 `Some(self)` —— 那会让"按右箭头"把提示文本直接插进输入行
/// （旧实现用的是裸 `String`，就有这个毛病）。显式返回 `None` 后右箭头只会 beep。
struct ArgHint {
    text: String,
}

impl Hint for ArgHint {
    fn display(&self) -> &str {
        &self.text
    }
    fn completion(&self) -> Option<&str> {
        None
    }
}

#[derive(Clone)]
struct CalcHelper {
    colors: ColorConfig,
    /// 存储变量名（`/let`），供 Tab 补全；随颜色一起在增删变量后刷新
    vars: Vec<String>,
    /// 本帧提示种类（见 `HintKind`）
    hint_kind: Cell<HintKind>,
    /// `complete()` 时记录的光标位置（字符下标）。
    /// `update()` 必须用它来界定"原区间" —— 因为那里我们已把光标挪进括号，
    /// 若读 `line.pos()` 会越补越短。
    orig_pos: Cell<usize>,
}

impl Completer for CalcHelper {
    type Candidate = Pair;

    fn complete(&self, line: &str, pos: usize, _ctx: &Context<'_>) -> RlResult<(usize, Vec<Pair>)> {
        // 取光标左侧的 token（字母数字下划线，以及行首指令的 `/`）。
        // 按**字符**扫描而不是字节：行内可能出现中文（变量之外的说明性文字），字节下标会与
        // rustyline 的字符下标错位。
        let chars: Vec<char> = line.chars().collect();
        let pos = pos.min(chars.len());
        let mut start = pos;
        while start > 0 {
            let c = chars[start - 1];
            if c.is_ascii_alphanumeric() || c == '_' || c == '/' {
                start -= 1;
            } else {
                break;
            }
        }
        self.orig_pos.set(pos);
        let token: String = chars[start..pos].iter().collect();
        let cands = completion_candidates(&token, &self.vars)
            .into_iter()
            .map(|name| match fn_meta(&name) {
                // 函数：补成 `name()`，参数留给用户在括号内输入
                Some(m) => Pair {
                    display: format!("{}{}", name, m.sig),
                    replacement: format!("{}()", name),
                },
                // 指令 / 常数 / 变量：原样补全
                None => Pair {
                    display: name.clone(),
                    replacement: name,
                },
            })
            .collect();
        Ok((start, cands))
    }

    /// **实现"Tab 补成 `sin()` 且光标落在括号内"**。
    ///
    /// rustyline 的默认实现用 `line.replace(start..line.pos(), elected)`，光标会停在 `)` 之后；
    /// 这里插入后再把光标左移一格。注意 `line.pos()` 已被我们改过，所以原区间必须用
    /// `complete()` 记下的 `orig_pos` 来界定（循环补全每次都会回到同一区间重放）。
    fn update(&self, line: &mut LineBuffer, start: usize, elected: &str, cl: &mut Changeset) {
        let orig = self.orig_pos.get().clamp(start, line.len());
        let (range, pos) = completion_edit(start, orig, elected);
        line.replace(range, elected, cl);
        line.set_pos(pos);
    }
}

impl Hinter for CalcHelper {
    type Hint = ArgHint;

    fn hint(&self, line: &str, pos: usize, _ctx: &Context<'_>) -> Option<ArgHint> {
        // A. 光标落在某个函数调用的括号内 ⇒ 显示参数提示（不再要求光标在行尾）
        // 只在**光标位于行尾**时显示：rustyline 的 hint 只能追加在整行末尾，
        // 光标停在行中间时它会跑到最右边、看起来像行尾的垃圾（用户反馈过）。
        // 行尾正是正常打字的位置，此时提示就贴在手边。
        if pos == line.len() {
            if let Some((name, _open, argc)) = enclosing_function_call(line, pos) {
                if let Some(m) = fn_meta(&name) {
                    let (kind, text) = if argc > m.max {
                        (
                            HintKind::TooMany,
                            i18n::fmt("参数过多：最多 {0} 个", &[&m.max.to_string()]),
                        )
                    } else if argc < m.min {
                        // 渐进提示：只显示还没写的参数
                        (HintKind::Incomplete, remaining_args_hint(m.sig, argc))
                    } else {
                        (HintKind::Normal, remaining_args_hint(m.sig, argc))
                    };
                    self.hint_kind.set(kind);
                    return Some(ArgHint {
                        text: format!("  {}", text),
                    });
                }
            }
        }
        self.hint_kind.set(HintKind::Normal);
        // B. 退回原有的"行尾"提示（指令用法 / 函数签名）
        if pos == line.len() {
            inline_hint(line).map(|text| ArgHint { text })
        } else {
            None
        }
    }
}

impl Validator for CalcHelper {
    fn validate(
        &self,
        _ctx: &mut rustyline::validate::ValidationContext,
    ) -> RlResult<rustyline::validate::ValidationResult> {
        // 刻意恒为合法：**不拦回车**。括号问题靠实时染色 + 参数提示引导，
        // 回车后仍由解析器给正式报错（文案三语已有）。
        Ok(rustyline::validate::ValidationResult::Valid(None))
    }
}

impl Highlighter for CalcHelper {
    fn highlight<'l>(&self, line: &'l str, pos: usize) -> std::borrow::Cow<'l, str> {
        let pair = match_bracket_pair(line, pos);
        let bad = unmatched_brackets(line, pos);
        colorize_impl(line, &self.colors, false, None, pair, &bad).into()
    }

    fn highlight_char(&self, _line: &str, _pos: usize, _forced: bool) -> bool {
        // 恒 true：光标移动也要重绘，配对高亮才能跟着光标走
        true
    }

    fn highlight_hint<'h>(&self, hint: &'h str) -> std::borrow::Cow<'h, str> {
        match self.hint_kind.get() {
            HintKind::TooMany => hint.color(self.colors.error).bold().to_string().into(),
            HintKind::Incomplete => hint.color(self.colors.operator).to_string().into(),
            HintKind::Normal => hint.dimmed().to_string().into(),
        }
    }
}

impl Helper for CalcHelper {}

/// 全角字符 → 半角（中文输入法下 `。`（ 等极易打出）
struct FullwidthHandler {
    to: &'static str,
}

impl ConditionalEventHandler for FullwidthHandler {
    fn handle(
        &self,
        _evt: &Event,
        _n: RepeatCount,
        _positive: bool,
        _ctx: &EventContext,
    ) -> Option<Cmd> {
        Some(Cmd::Insert(1, self.to.to_string()))
    }
}

/// 注册自定义键位：Ctrl+C 清行/连按退出 + 全角转半角。
/// （Ctrl+L 清屏、Ctrl+R 反向历史搜索是 rustyline 的 Emacs 默认绑定，无需注册。）
fn install_key_bindings(rl: &mut Editor<CalcHelper, DefaultHistory>) {
    // **不给 Ctrl+C 绑定自定义动作**：空闲时应走 rustyline 默认的 `Cmd::Interrupt`
    // ⇒ `ReadlineError::Interrupted` ⇒ 主循环退出程序（"没运算时 Ctrl+C 退出"）。
    // 计算中的中断走 `cancel` 模块的控制台处理器（那里没有键盘读取）。
    for (full, half) in FULLWIDTH_MAP {
        rl.bind_sequence(
            KeyEvent::from(*full),
            EventHandler::Conditional(Box::new(FullwidthHandler { to: half })),
        );
    }
}

/// `/xxx` 是否该当作指令点亮。
///
/// `command_anywhere = true`（/help 正文）时必须是**已知指令** —— 否则正文里的 `x/y/z`
/// 会把 `/y`、`/z` 点亮（英文帮助里就出现过这种误判）。输入行是 false：
/// 用户正在敲指令、还没敲完也该有指令色。
fn command_token_is_valid(tok: &str, command_anywhere: bool) -> bool {
    !command_anywhere || COMMANDS.contains(&tok)
}

/// 按类别给一段文本着色（REPL 输入行、/help 列表与各指令提示共用）：
/// 函数（functions）、指令（commands）、常量（constants）、数字（numbers）、
/// 运算符（operators）、括号（brackets）各自使用对应颜色，其余文字保持原样。
/// `command_anywhere = true` 时文本中任意位置的 `/xxx` 都按指令着色（/help 列表场景），
/// 为 false 时只认行首指令（输入行场景）。
fn colorize_text(line: &str, colors: &ColorConfig, command_anywhere: bool) -> String {
    colorize_impl(line, colors, command_anywhere, None, None, &[])
}

/// 结果行着色：与输入行**共用同一套** token 着色（函数/运算符/括号/常量/数字各按类别上色），
/// 未被识别的部分（变量名、中文标签、逗号等）用 `result` 颜色打底。
/// 于是结果既保留了"这是结果"的整体色感，又能一眼看出表达式结构。
/// （旧实现把整行刷成单一的 `result` 颜色，看起来和没高亮一样。）
fn colorize_result(line: &str, colors: &ColorConfig) -> String {
    colorize_impl(line, colors, false, Some(colors.result), None, &[])
}

/// `colorize_text` / `colorize_result` 的公共实现。
/// `base` 为 `None` 时未识别文本原样输出（输入行场景，行为与颜色改造前逐字节一致）；
/// 为 `Some(c)` 时用颜色 `c` 打底（结果行场景）。
/// 底色**不加粗**：否则纯白加粗的标签会把同行的数字（number 用普通白）衬得过暗。
fn colorize_impl(
    line: &str,
    colors: &ColorConfig,
    command_anywhere: bool,
    base: Option<Color>,
    pair: Option<(usize, usize)>,
    bad: &[usize],
) -> String {
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut result = String::new();
    // 累积"未识别文本"，遇到已着色的 token 时按底色一次性刷出（缓冲可减少 ANSI 转义序列数量）
    let mut plain = String::new();
    macro_rules! flush_plain {
        () => {
            if !plain.is_empty() {
                match base {
                    Some(c) => result.push_str(&plain.color(c).to_string()),
                    None => result.push_str(&plain),
                }
                plain.clear();
            }
        };
    }
    let mut i = 0;
    let mut seen_non_space = false; // 行首判定（command_anywhere = false 时用）

    while i < len {
        // 指令：`/` + 至少一个字母（排除 1/2、1/x 这类除法）
        // 指令：`/` + 至少一个字母（排除 1/2、1/x 这类除法）
        //
        // `command_anywhere = true`（/help 正文着色）时**必须再要求是已知指令**：
        // 否则正文里的 `x/y/z` 会把 `/y`、`/z` 当成指令点亮（英文帮助里就出现过）——
        // 帮助正文是散文，不该用"像指令"这种弱判据。
        // 输入行（false）保留宽松判据：用户正在敲指令时即使还没敲完也该有指令色。
        let is_command = chars[i] == '/'
            && i + 1 < len
            && chars[i + 1].is_ascii_alphabetic()
            && (command_anywhere || !seen_non_space);
        if is_command {
            let start = i;
            i += 1;
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let cmd: String = chars[start..i].iter().collect();
            if !command_token_is_valid(&cmd, command_anywhere) {
                // 不是已知指令 ⇒ 当作普通文本（连同 `/` 一起原样输出）
                plain.push_str(&cmd);
                i = start + cmd.chars().count();
                continue;
            }
            flush_plain!();
            result.push_str(&cmd.color(colors.command).bold().to_string());
            seen_non_space = true;
            continue;
        }

        if !chars[i].is_whitespace() {
            seen_non_space = true;
        }

        // 括号：不合法的标红 > 光标配对高亮（同色加粗）> 常规色
        if BRACKETS.contains(&chars[i]) {
            flush_plain!();
            if bad.contains(&i) {
                result.push_str(&style_char(chars[i], colors.error).to_string());
            } else if matches!(pair, Some((a, b)) if a == i || b == i) {
                // 加粗要经 ColoredString（ 本身没有 bold），故这里直接构造
                result.push_str(&chars[i].to_string().color(colors.bracket).bold().to_string());
            } else {
                result.push_str(&style_char(chars[i], colors.bracket).to_string());
            }
            i += 1;
            continue;
        }

        // 运算符
        if OPS.contains(&chars[i]) {
            flush_plain!();
            result.push_str(&style_char(chars[i], colors.operator).bold().to_string());
            i += 1;
            continue;
        }

        // 标识符（字母开头；中文说明文字也会被整体扫描，未识别即原样输出）
        if chars[i].is_alphabetic() || chars[i] == '_' {
            let start = i;
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();

            // 仅完整匹配才着色
            if VALID_FUNCTIONS.contains(&word.as_str()) {
                flush_plain!();
                result.push_str(&word.color(colors.function).bold().to_string());
            } else if CONSTANTS.contains(&word.as_str()) {
                flush_plain!();
                result.push_str(&word.color(colors.constant).bold().to_string());
            } else if word == "ans" {
                flush_plain!();
                result.push_str(&word.color(colors.number).to_string());
            } else {
                plain.push_str(&word);
            }
            continue;
        }

        // 数字（含科学计数法 1e3、2.5e-2；'.' 只有后跟数字才算数字开头）
        if chars[i].is_ascii_digit()
            || (chars[i] == '.' && i + 1 < len && chars[i + 1].is_ascii_digit())
        {
            let start = i;
            while i < len && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            // 指数部分：e/E [+-] digits（无数字则回退，如 2e 的 e 是常数）
            if i < len && (chars[i] == 'e' || chars[i] == 'E') {
                let save = i;
                let mut j = i + 1;
                if j < len && (chars[j] == '+' || chars[j] == '-') {
                    j += 1;
                }
                if j < len && chars[j].is_ascii_digit() {
                    while j < len && chars[j].is_ascii_digit() {
                        j += 1;
                    }
                    i = j;
                } else {
                    i = save;
                }
            }
            let num: String = chars[start..i].iter().collect();
            flush_plain!();
            result.push_str(&num.color(colors.number).to_string());
            continue;
        }

        // 其他字符
        plain.push(chars[i]);
        i += 1;
    }

    flush_plain!();
    result
}

/// 为单个字符着色
fn style_char(ch: char, color: Color) -> colored::ColoredString {
    ch.to_string().color(color)
}

/// 启动方式：默认交互式 REPL；其余三种由显式参数启用。
/// 注意：管道/重定向**不会**自动切换（保持既有"管道喂输入仍是 REPL"的验证习惯）。
#[derive(Debug, PartialEq)]
enum Cli {
    Interactive,
    /// `-e/--eval <表达式>`：执行一条后退出
    Eval(String),
    /// `-f/--file <路径>`：逐行执行脚本
    Script(std::path::PathBuf),
    /// `--stdin`：从标准输入流式执行
    Stdin,
}

/// 命令行选项
#[derive(Debug, PartialEq)]
struct Options {
    cli: Cli,
    /// `--lang <代码>`：强制界面语言（优先于状态文件，便于脚本化核对三语输出）
    lang: Option<i18n::Lang>,
    /// `-q/--no-timing`：不输出耗时行
    no_timing: bool,
}

/// 解析命令行参数（纯函数，便于单元测试）
fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut cli = Cli::Interactive;
    let mut lang = None;
    let mut no_timing = false;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "-e" | "--eval" => {
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| format!("参数 {0} 缺少取值", a))?;
                cli = Cli::Eval(v.clone());
                i += 2;
            }
            "-f" | "--file" => {
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| format!("参数 {0} 缺少取值", a))?;
                cli = Cli::Script(std::path::PathBuf::from(v));
                i += 2;
            }
            "--stdin" => {
                cli = Cli::Stdin;
                i += 1;
            }
            "--lang" => {
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| format!("参数 {0} 缺少取值", a))?;
                let l = i18n::Lang::parse(v)
                    .ok_or_else(|| format!("无法识别的语言代码: {0}", v))?;
                lang = Some(l);
                i += 2;
            }
            "-q" | "--no-timing" => {
                no_timing = true;
                i += 1;
            }
            _ => return Err(format!("未知参数: {0}", a)),
        }
    }
    Ok(Options { cli, lang, no_timing })
}

/// 非交互执行：`-e` 只跑一条且只打印裸结果行；`-f`/`--stdin` 逐行执行。
/// 返回进程退出码：0 全部成功、1 有求值错误、2 文件/读取错误。
fn run_non_interactive(opts: &Options, state: &mut AppState) -> i32 {
    match &opts.cli {
        Cli::Eval(expr) => {
            let (out, is_err) = run_line(expr, state);
            lprint!("{}", out);
            if is_err { 1 } else { 0 }
        }
        Cli::Script(path) => {
            let text = match std::fs::read_to_string(path) {
                Ok(t) => t,
                Err(e) => {
                    println!("{}", i18n::t(&format!("读取文件失败: {0}", io_reason(&e))));
                    return 2;
                }
            };
            if run_script_lines(text.lines(), state, false) { 1 } else { 0 }
        }
        Cli::Stdin => {
            let mut text = String::new();
            {
                let stdin = std::io::stdin();
                let _ = std::io::Read::read_to_string(&mut stdin.lock(), &mut text);
            }
            if run_script_lines(text.lines(), state, false) { 1 } else { 0 }
        }
        Cli::Interactive => 0,
    }
}

/// 逐行执行脚本/标准输入：空行与 `#` 注释跳过；`/` 开头走指令（`/exit` 结束）；其余求值。
/// 返回是否出现过错误（错误行照常打印，供脚本排查）。
fn run_script_lines<'a, I: Iterator<Item = &'a str>>(
    lines: I,
    state: &mut AppState,
    quiet: bool,
) -> bool {
    let mut had_err = false;
    for line in lines {
        let input = line.trim();
        if input.is_empty() || input.starts_with('#') {
            continue;
        }
        if input.starts_with('/') {
            if handle_command(input, state) {
                break; // /exit 或 /quit
            }
            continue;
        }
        let (out, is_err) = run_line(input, state);
        if !quiet {
            lprint!("{}", out);
        }
        had_err |= is_err;
    }
    had_err
}

fn main() {
    // 命令行参数：不带参数即交互式 REPL；带 -e/-f/--stdin 才走非交互
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opts = match parse_args(&args) {
        Ok(o) => o,
        Err(e) => {
            // 参数错误时先确定界面语言，保证提示与后续输出一致
            match state::load().and_then(|(_, _, _, saved_lang)| saved_lang) {
                Some(l) => i18n::set(l),
                None => i18n::set(i18n::detect_system()),
            }
            println!("{}", i18n::t(&e));
            println!(
                "{}",
                i18n::t("用法: hipercalc [-e <表达式>] [-f <文件>] [--stdin] [--lang <代码>] [-q]")
            );
            std::process::exit(2);
        }
    };

    let mut evaluator = Evaluator::new();
    let mut colors = ColorConfig::default();

    // 加载上次会话状态（显示/角度/计算模式 + `/let` 变量 + `/set` 颜色）；无状态文件则保持默认
    let restored = state::load();
    let mut restored_vars = 0usize;
    let mut restore_ok = false;
    // 用户配置里的耗时开关（默认开）；命令行 `-q` 只改运行期值，不改它
    let mut timing_base = true;
    if let Some((m, vars, saved_colors, saved_lang)) = restored {
        restore_ok = true;
        evaluator.display_mode = m.display;
        evaluator.angle_mode = m.angle;
        calc_mode::set(m.calc);
        calc_mode::set_timing(m.timing);
        timing_base = m.timing;
        // 精度与显示开关：必须在任何运算之前设定（常数缓存按精度分桶，越早设定越省重复计算）
        bigfloat::set_precision(m.prec);
        bigfloat::set_display_digits(m.digits);
        bigfloat::set_sci_allowed(m.sci);
        bigfloat::set_group_enabled(m.group);
        // 界面语言：状态文件有记录就用它；没有（首次启动/旧版本文件）则按系统语言判定
        match saved_lang {
            Some(l) => i18n::set(l),
            None => i18n::set(i18n::detect_system()),
        }
        restored_vars = vars.len();
        for (name, v) in vars {
            evaluator.vars.insert(name, v);
        }
        // 颜色：类别与颜色名都合法才应用（非法项跳过，保持默认）
        for (cat, name) in saved_colors {
            if let Some(c) = parse_color(&name) {
                colors.set_category(&cat, c);
            }
        }
    }

    if !restore_ok {
        // 首次启动（无状态文件）：语言按系统语言判定，其余保持默认
        i18n::set(i18n::detect_system());
    }
    // 状态文件/系统语言确定的"基础语言"：命令行 --lang 只覆盖显示，不改它（避免写坏用户配置）
    let lang_base = i18n::get();
    // 命令行覆盖：--lang 优先于状态文件；-q 本会话关闭耗时行（**不落盘**）
    if let Some(l) = opts.lang {
        i18n::set(l);
    }
    if opts.no_timing {
        calc_mode::set_timing(false);
    }

    let mut state = AppState {
        evaluator,
        colors: colors.clone(),
        lang_base,
        timing_base,
    };

    // 非交互入口：显式 -e/-f/--stdin 时执行完即退出，不创建 readline 编辑器
    if !matches!(opts.cli, Cli::Interactive) {
        std::process::exit(run_non_interactive(&opts, &mut state));
    }

    let helper = CalcHelper {
        hint_kind: Cell::new(HintKind::Normal),
        orig_pos: Cell::new(0),
        colors: colors.clone(),
        vars: state.evaluator.vars.keys().cloned().collect(),
    };
    cancel::install_handler();
    let mut rl = Editor::new().expect("创建 readline 编辑器失败");
    rl.set_helper(Some(helper));
    install_key_bindings(&mut rl);

    // 加载历史（失败则忽略，首次运行无历史文件）
    if let Some(path) = history_path() {
        let _ = rl.load_history(&path);
    }

    lprint!(
        "{}",
        "超高精度命令行计算器 (HiPerCalc) v1.1"
            .color(state.colors.prompt)
            .bold()
    );
    lprint!(
        "{}",
        format!("当前模式: {}", mode_info(&state)).color(state.colors.prompt)
    );
    if restored_vars > 0 {
        lprint!(
            "{}",
            format!(
                "已恢复上次会话设置，含 {} 个存储变量（/var 查看，/del all 清空）",
                restored_vars
            )
            .dimmed()
        );
    }
    // 该消息的插值带颜色，输出边界的"纯文本段匹配"会切段，故在构造期翻译
    lprint!(
        "{}",
        i18n::fmt(
            "输入 {0} 查看帮助，输入表达式进行计算，{1} 退出",
            &[
                &"/help".color(state.colors.command).bold().to_string(),
                &"Ctrl+C".dimmed().to_string(),
            ]
        )
    );
    lprint!();

    loop {
        // 使用纯文本 prompt 避免 ANSI 转义码导致光标偏移
        let read = rl.readline("> ");

        match read {
            Ok(line) => {
                let input = line.trim().to_string();
                if input.is_empty() {
                    continue;
                }

                // 记录历史（含指令）
                let _ = rl.add_history_entry(input.as_str());

                if input.starts_with('/') {
                    if handle_command(&input, &mut state) {
                        break;
                    }
                    // 指令可能改变了颜色或变量，同步到高亮器与补全器
                    {
                        let h = rl.helper_mut().unwrap();
                        h.colors = state.colors.clone();
                        h.vars = state.evaluator.vars.keys().cloned().collect();
                    }
                    continue;
                }

                // 表达式计算前同步颜色与变量名
                {
                    let h = rl.helper_mut().unwrap();
                    h.colors = state.colors.clone();
                    h.vars = state.evaluator.vars.keys().cloned().collect();
                }
                handle_input_result(&input, &mut state);
            }
            Err(ReadlineError::Interrupted) => {
                lprint!();
                break;
            }
            Err(ReadlineError::Eof) => {
                lprint!();
                break;
            }
            Err(err) => {
                leprint!("{}: {:?}", "读取错误".color(state.colors.error).bold(), err);
                break;
            }
        }
    }

    // 退出前保存历史与会话状态（模式 + 变量 + 颜色）
    if let Some(path) = history_path() {
        let _ = rl.save_history(&path);
    }
    persist_state(&state);
}

fn handle_command(input: &str, state: &mut AppState) -> bool {
    let parts: Vec<&str> = input.split_whitespace().collect();
    if parts.is_empty() {
        return false;
    }

    match parts[0].to_lowercase().as_str() {
        "/help" => print_help(state),
        "/clear" => clear_screen(),
        "/mode" => {
            if parts.len() < 2 {
                // 用法提醒与当前模式（/mode、/set 的用法提示风格一致）
                lprint!(
                    "{}: {}",
                    "用法".color(state.colors.prompt),
                    colorize_text(&mode_usage(), &state.colors, true)
                );
                lprint!(
                    "{}",
                    format!("当前模式: {}", mode_info(state)).color(state.colors.prompt)
                );
            } else {
                handle_mode(&parts, state);
                // 模式变更立即落盘，重启后保持
                persist_state(state);
            }
        }
        "/lang" | "/language" => {
            handle_lang(parts.get(1).copied(), state);
            // 语言立即落盘（重启后保持）；切换只影响后续输出
            persist_state(state);
        }
        "/reset" => {
            handle_reset(parts.get(1).map(|s| s.to_lowercase()).as_deref(), state);
            persist_state(state);
        }
        "/save" => handle_save(&parts, state),
        "/load" => handle_load(&parts, state),
        "/timing" => {
            match parts.get(1).map(|s| s.to_lowercase()).as_deref() {
                Some("on") => {
                    calc_mode::set_timing(true);
                    state.timing_base = true;
                    lprint!("{}", "已开启耗时显示".green());
                }
                Some("off") => {
                    calc_mode::set_timing(false);
                    state.timing_base = false;
                    lprint!("{}", "已关闭耗时显示".dimmed());
                }
                _ => {
                    lprint!(
                        "{}: {}",
                        "用法".color(state.colors.prompt),
                        colorize_text("/timing <on|off>", &state.colors, true)
                    );
                }
            }
            // 立即落盘（重启后保持）
            persist_state(state);
        }
        "/set" => handle_set(parts, state),
        "/let" => handle_let(input, state),
        "/var" => handle_vars(state),
        "/del" => handle_del(parts, state),
        "/exit" | "/quit" | "/q" => return true,
        _ => {
            lprint!(
                "{}",
                i18n::fmt(
                    "{0}: {1}。输入 {2} 查看可用指令。",
                    &[
                        &"未知指令".color(state.colors.error).bold().to_string(),
                        &parts[0].color(state.colors.command).to_string(),
                        &"/help".color(state.colors.command).bold().to_string(),
                    ]
                )
            );
        }
    }
    false
}

/// 类别/颜色名显示：中文下写"英文(中文)"，其它语言直接显示英文键名本身
/// （`error`、`green` 这类键名就是英文，翻译反而会变成 `Error` 这种不一致的写法）
fn option_name(key: &str, zh: &str) -> String {
    if i18n::get() == i18n::Lang::ZhCn {
        format!("{}({})", key, zh)
    } else {
        key.to_string()
    }
}

/// 选项显示名：中文下写"英文(中文)"，其它语言只写译名。
/// 避免出现 `functions(functions)` 这种重复（英文模式下中文标注本就多余）。
fn label_with_hint(key: &str, zh: &str) -> String {
    if i18n::get() == i18n::Lang::ZhCn {
        format!("{}({})", key, zh)
    } else {
        i18n::t(zh)
    }
}

/// /mode 的用法文本（选项按指令类别着色、带含义标注）
fn mode_usage() -> String {
    let mut s = String::from("/mode ");
    let items = [
        ("mathio", "数学显示"),
        ("lineio", "线性显示"),
        ("deg", "角度制"),
        ("rad", "弧度制"),
        ("fast", "快速"),
        ("deep", "死算"),
    ];
    for (i, (name, zh)) in items.iter().enumerate() {
        if i > 0 {
            s.push_str(" | ");
        }
        s.push_str(&label_with_hint(name, zh));
    }
    // 需要取值的子命令（纯 ASCII，无需翻译）
    s.push_str(" | prec <N> | digits <N> | sci <on|off> | group <on|off>");
    s
}

/// `/reset`：清空变量与 ans；`/reset all` 连模式、颜色、精度一起恢复默认（**语言不重置**）
fn handle_reset(arg: Option<&str>, state: &mut AppState) {
    if arg == Some("all") {
        state.evaluator.display_mode = DisplayMode::LineIO;
        state.evaluator.angle_mode = trig::AngleMode::Radian;
        calc_mode::set(calc_mode::CalcMode::Fast);
        calc_mode::set_timing(true);
        state.timing_base = true;
        bigfloat::set_precision(bigfloat::DEFAULT_PRECISION);
        bigfloat::set_display_digits(bigfloat::DEFAULT_DISPLAY_DIGITS);
        bigfloat::set_sci_allowed(true);
        bigfloat::set_group_enabled(false);
        state.colors = ColorConfig::default();
        state.evaluator.vars.clear();
        state.evaluator.ans = Number::from_int(0);
        lprint!(
            "{}",
            "已恢复默认设置（模式、颜色、精度），语言保持不变".green()
        );
        return;
    }
    let n = state.evaluator.vars.len();
    state.evaluator.vars.clear();
    state.evaluator.ans = Number::from_int(0);
    lprint!(
        "{}",
        format!("已重置会话（清空 {0} 个变量与 ans）", n).green()
    );
}

/// `/save <路径>`：把当前会话写成可回放脚本（模式 + 精度 + 颜色 + 变量）
fn handle_save(parts: &[&str], state: &AppState) {
    let Some(path) = parts.get(1) else {
        lprint!(
            "{}: {}",
            "用法".color(state.colors.prompt),
            colorize_text("/save <文件路径>", &state.colors, true)
        );
        return;
    };
    let mut out = String::new();
    out.push_str("# hipercalc session\n");
    out.push_str(&format!(
        "/mode {}\n",
        match state.evaluator.display_mode {
            DisplayMode::MathIO => "mathio",
            DisplayMode::LineIO => "lineio",
        }
    ));
    out.push_str(&format!(
        "/mode {}\n",
        match state.evaluator.angle_mode {
            trig::AngleMode::Degree => "deg",
            trig::AngleMode::Radian => "rad",
        }
    ));
    out.push_str(&format!(
        "/mode {}\n",
        match state::current_calc_mode() {
            calc_mode::CalcMode::Deep => "deep",
            calc_mode::CalcMode::Fast => "fast",
        }
    ));
    out.push_str(&format!("/mode prec {}\n", bigfloat::precision()));
    out.push_str(&format!("/mode digits {}\n", bigfloat::display_digits()));
    out.push_str(&format!(
        "/mode sci {}\n",
        if bigfloat::sci_allowed() { "on" } else { "off" }
    ));
    out.push_str(&format!(
        "/mode group {}\n",
        if bigfloat::group_enabled() { "on" } else { "off" }
    ));
    out.push_str(&format!(
        "/timing {}\n",
        if calc_mode::timing_enabled() { "on" } else { "off" }
    ));
    for (cat, name) in state.colors.to_pairs() {
        out.push_str(&format!("/set {} {}\n", cat, name));
    }
    for (name, v) in &state.evaluator.vars {
        out.push_str(&format!(
            "/let {} = {}\n",
            name,
            state::encode_var_for_script(v)
        ));
    }
    match std::fs::write(path, out) {
        Ok(_) => lprint!(
            "{}",
            format!(
                "已保存会话到 {0}（{1} 个变量）",
                path,
                state.evaluator.vars.len()
            )
            .green()
        ),
        Err(e) => lprint!(
            "{}: {}",
            "错误".color(state.colors.error).bold(),
            format!("保存文件失败: {0}", io_reason(&e))
        ),
    }
}

/// `/load <路径>`：逐行执行脚本（静默执行，只在末尾给一行汇总）
fn handle_load(parts: &[&str], state: &mut AppState) {
    let Some(path) = parts.get(1) else {
        lprint!(
            "{}: {}",
            "用法".color(state.colors.prompt),
            colorize_text("/load <文件路径>", &state.colors, true)
        );
        return;
    };
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let count = text
                .lines()
                .filter(|l| {
                    let t = l.trim();
                    !t.is_empty() && !t.starts_with('#')
                })
                .count();
            calc_mode::set_quiet(true);
            let had_err = run_script_lines(text.lines(), state, true);
            calc_mode::set_quiet(false);
            if had_err {
                lprint!(
                    "{}",
                    format!("已从 {0} 加载 {1} 行（其中部分行出错，已跳过）", path, count).yellow()
                );
            } else {
                lprint!(
                    "{}",
                    format!("已从 {0} 加载 {1} 行", path, count).green()
                );
            }
        }
        Err(e) => lprint!(
            "{}: {}",
            "错误".color(state.colors.error).bold(),
            format!("读取文件失败: {0}", io_reason(&e))
        ),
    }
}

fn handle_set(parts: Vec<&str>, state: &mut AppState) {
    // 仅输入 /set：先提醒用法（类别带中文标注），再列出当前各类别的颜色
    if parts.len() < 3 {
        print_set_usage(state);
        return;
    }

    let category = parts[1].to_lowercase();
    let color_name_in = parts[2].to_lowercase();

    let new_color = match parse_color(&color_name_in) {
        Some(c) => c,
        None => {
            lprint!("{}: {}", "未知颜色".color(state.colors.error).bold(), color_name_in);
            lprint!("{}", colorize_text("  可用颜色见 /set 的用法提示", &state.colors, true));
            return;
        }
    };

    if !state.colors.set_category(&category, new_color) {
        lprint!("{}: {}", "未知类别".color(state.colors.error).bold(), category);
        lprint!("{}", colorize_text("  可用类别见 /set 的用法提示", &state.colors, true));
        return;
    }

    // 颜色设置立即落盘（状态文件），重启后保持
    persist_state(state);

    // 类别与颜色名带标注（中文）或译名（其它语言）
    let cat_label = COLOR_CATEGORIES
        .iter()
        .find(|(c, _)| *c == category)
        .map(|(_, zh)| *zh)
        .unwrap_or("");
    let color_label = COLOR_OPTIONS
        .iter()
        .find(|(c, _, _)| *c == color_name_in)
        .map(|(_, zh, _)| *zh)
        .unwrap_or("");
    lprint!(
        "颜色已设置为 {0}（已保存，重启后保持）",
        format!(
            "{} = {}",
            label_with_hint(&category, cat_label),
            label_with_hint(&color_name_in, color_label)
        )
    );
}

/// 仅输入 /set 时的提示：用法（类别/颜色均带中文标注）+ 当前各类别颜色
fn print_set_usage(state: &AppState) {
    lprint!(
        "{}: {}",
        "用法".color(state.colors.prompt),
        colorize_text("/set <类别> <颜色>", &state.colors, true)
    );
    let cats: Vec<String> = COLOR_CATEGORIES
        .iter()
        .map(|(k, zh)| option_name(k, zh))
        .collect();
    lprint!("{}", colorize_text(&format!("类别: {}", cats.join(" ")), &state.colors, true));
    // 颜色名保持英文（可选值），逐项附标注（中文）/译名（其它语言）
    let color_lines: Vec<String> = COLOR_OPTIONS
        .iter()
        .map(|(name, zh, _)| option_name(name, zh))
        .collect();
    lprint!("颜色: {}", color_lines.join("  "));
    lprint!("当前颜色（按各自颜色显示）:");
    for (cat, zh) in COLOR_CATEGORIES {
        let c = state.colors.get_category(cat);
        lprint!("  {}: {}", option_name(cat, zh), color_name(c).color(c).bold());
    }
}

fn handle_mode(parts: &[&str], state: &mut AppState) {
    let sub = parts[1].to_lowercase();
    let val = parts.get(2).map(|s| s.to_lowercase());
    match sub.as_str() {
        "mathio" => {
            state.evaluator.display_mode = DisplayMode::MathIO;
            lprint!("{}", "已切换到数学显示模式 (MathIO)".green());
        }
        "lineio" => {
            state.evaluator.display_mode = DisplayMode::LineIO;
            lprint!("{}", "已切换到线性显示模式 (LineIO)".green());
        }
        "deg" => {
            state.evaluator.angle_mode = trig::AngleMode::Degree;
            lprint!("{}", "已切换到角度模式 (Degree)".green());
        }
        "rad" => {
            state.evaluator.angle_mode = trig::AngleMode::Radian;
            lprint!("{}", "已切换到弧度模式 (Radian)".green());
        }
        "fast" => {
            calc_mode::set(calc_mode::CalcMode::Fast);
            lprint!(
                "{}",
                "已切换到快速模式 (Fast)：规模超限时直接返回提示，输出 20 位有效数字（大数用科学计数法）"
                    .green()
            );
        }
        "deep" => {
            calc_mode::set(calc_mode::CalcMode::Deep);
            lprint!(
                "{}",
                "已切换到死算模式 (Deep)：不设规模上限、完整精度输出（不走科学计数法）；极端输入可能长时间无响应或产生超长输出"
                    .yellow()
            );
        }
        // /mode prec N：工作精度（小数位数）
        "prec" => match val.as_deref().and_then(|v| v.parse::<usize>().ok()) {
            Some(n) if (20..=2000).contains(&n) => {
                bigfloat::set_precision(n);
                lprint!(
                    "{}",
                    format!(
                        "已设置工作精度为 {0} 位小数（显示 {1} 位有效数字）",
                        bigfloat::precision(),
                        bigfloat::display_digits()
                    )
                    .green()
                );
                if n > bigfloat::display_digits() {
                    lprint!(
                        "{}",
                        format!("提示: 显示位数（{}）少于工作精度，可用 /mode digits 提高", bigfloat::display_digits())
                            .dimmed()
                    );
                }
            }
            _ => lprint!(
                "{}: {}",
                "用法".color(state.colors.prompt),
                colorize_text("/mode prec <20..2000>", &state.colors, true)
            ),
        },
        // /mode digits N：显示有效位数
        "digits" => match val.as_deref().and_then(|v| v.parse::<usize>().ok()) {
            Some(n) if n >= 1 && n <= bigfloat::precision() => {
                bigfloat::set_display_digits(n);
                lprint!(
                    "{}",
                    format!("已设置显示 {0} 位有效数字", bigfloat::display_digits()).green()
                );
            }
            _ => lprint!(
                "{}: {}",
                "用法".color(state.colors.prompt),
                colorize_text(
                    &format!("/mode digits <1..{}>（不超过工作精度）", bigfloat::precision()),
                    &state.colors,
                    true
                )
            ),
        },
        // /mode sci on|off：是否用科学计数法显示大数
        "sci" => match val.as_deref() {
            Some("on") => {
                bigfloat::set_sci_allowed(true);
                lprint!("{}", "已开启科学计数法显示".green());
            }
            Some("off") => {
                bigfloat::set_sci_allowed(false);
                lprint!("{}", "已关闭科学计数法显示（大数完整写出）".green());
            }
            _ => lprint!(
                "{}: {}",
                "用法".color(state.colors.prompt),
                colorize_text("/mode sci <on|off>", &state.colors, true)
            ),
        },
        // /mode group on|off：整数部分千分位
        "group" => match val.as_deref() {
            Some("on") => {
                bigfloat::set_group_enabled(true);
                lprint!("{}", "已开启千分位分隔".green());
            }
            Some("off") => {
                bigfloat::set_group_enabled(false);
                lprint!("{}", "已关闭千分位分隔".green());
            }
            _ => lprint!(
                "{}: {}",
                "用法".color(state.colors.prompt),
                colorize_text("/mode group <on|off>", &state.colors, true)
            ),
        },
        _ => {
            lprint!(
                "{}",
                i18n::fmt(
                    "{0}: {1}。可选: {2}",
                    &[
                        &"未知模式".color(state.colors.error).bold().to_string(),
                        &sub.color(state.colors.command).to_string(),
                        &colorize_text(
                            "mathio, lineio, deg, rad, fast, deep, prec, digits, sci, group",
                            &state.colors,
                            true
                        )
                        .to_string(),
                    ]
                )
            );
        }
    }
}


/// 生成一行的结果文本（供 REPL 与非交互入口共用）；返回 (文本, 是否错误)。
/// 不做任何打印、不涉及计时——计时与输出由调用方负责。
/// 生成一行的结果文本（供 REPL 与非交互入口共用）；返回 (文本, 是否错误)。
/// 不做打印、不涉及计时——计时与输出由调用方负责。
fn run_line(input: &str, state: &mut AppState) -> (String, bool) {
    let parsed = parse_and_eval(input, &mut state.evaluator);
    match parsed {
        Ok(EvalResult::Value(result)) => {
            // 先把结果移进 ans（大结果动辄几十万位，多克隆一次就是几十 MB 拷贝），
            // 再借用 ans 格式化
            state.evaluator.ans = result;
            (format_result_line(&state.evaluator.ans, state), false)
        }
        Ok(EvalResult::SdValue(result)) => {
            // sd() 表达式的特殊处理：与显示模式**互换**的显示形式
            // （MathIO 强制小数、LineIO 尝试符号）。
            // 前缀也按互换后的模式判定——被转换成小数时能否标 `=` 取决于
            // 该有理数能否完整写成有限小数；被转换成符号时精确值给 `=`。
            // 旧实现无条件标 `≈`，导致 sd(1/2) 这类精确值（`= 0.5`）被标错。
            state.evaluator.ans = result;
            let result = &state.evaluator.ans;
            let (output, prefix) = match state.evaluator.display_mode {
                DisplayMode::MathIO => (
                    display::format_decimal(result),
                    solve_aux::result_prefix(result, DisplayMode::LineIO),
                ),
                DisplayMode::LineIO => (
                    display::format_mathio(result),
                    solve_aux::result_prefix(result, DisplayMode::MathIO),
                ),
            };
            (
                colorize_result(&format!("{} {}", prefix, output), &state.colors),
                false,
            )
        }
        Ok(EvalResult::Factor(inner)) => (handle_factor(&inner, state), false),
        Ok(EvalResult::Equation(left, right)) => (handle_equation(&left, &right, state), false),
        Ok(EvalResult::System(equations)) => (handle_system(&equations, state), false),
        Ok(EvalResult::Fit(fit)) => (handle_fit(&fit, state), false),
        Ok(EvalResult::Triangle(tri)) => (handle_triangle(&tri, state), false),
        Ok(EvalResult::PrimeFac(n)) => (handle_primefac(&n, state), false),
        Ok(EvalResult::Symbolic(expr)) => (handle_symbolic(&expr, state), false),
        Err(e) => (
            format!("{}: {}", "错误".color(state.colors.error).bold(), e),
            true,
        ),
    }
}

fn handle_input_result(input: &str, state: &mut AppState) {
    // 标记"正在计算"：期间 Ctrl+C 会置中断标志（控制台处理器），求值器的检查点随即退出；
    // 离开作用域（含提前 return/错误路径）自动复位，空闲时才走"Ctrl+C 退出程序"。
    let _cancel_scope = cancel::Scope::new();
    // 计时：动态刷新覆盖"解析 + 结果生成"整段——耗时大多发生在结果
    // 格式化阶段（方程通式识别、根收集、因式分解等），而非 parse_and_eval 本身。
    let mut timing = Timing::begin();
    let (out, _is_err) = run_line(input, state);
    // 先清掉动态行，让结果覆盖它；再在结果下一行输出最终耗时（关闭计时时不打印）
    timing.stop_dynamic();
    lprint!("{}", out);
    let t = timing.elapsed_text();
    if !t.is_empty() {
        // 必须走 `lprint!`：`println!` 会绕过 i18n，英/繁模式下这一行会留在中文
        lprint!("{}", t.dimmed());
    }
}

/// 普通结果的格式化并着色（= / ≈ 前缀由显示模式与精确性决定）
fn format_result_line(result: &Number, state: &AppState) -> String {
    let output = match state.evaluator.display_mode {
        DisplayMode::MathIO => display::format_mathio(result),
        DisplayMode::LineIO => display::format_lineio(result),
    };
    let prefix = solve_aux::result_prefix(result, state.evaluator.display_mode);
    colorize_result(&format!("{} {}", prefix, output), &state.colors)
}

/// 处理 fac/factor(表达式)。
///
/// 参数**恰好是非零整数**时走素因数分解（`fac(12)` → `12 = 2^2 * 3`），与 `primefac(12)` 等价；
/// 其余情况（多项式、小数、0、含自由变量）保持原来的多项式分解语义不变——
/// 所以这里只是在前面加一层"非零整数则做素因数分解"的分流。
fn handle_factor(expr: &parser::Expr, state: &mut AppState) -> String {
    if let Some(n) = eval_nonzero_integer(expr, &mut state.evaluator) {
        return handle_primefac(&n, state);
    }
    let mode = state.evaluator.display_mode;
    match solver_factor::factor_expr(&state.evaluator, expr, mode) {
        Ok(output) => colorize_result(&format!("= {}", output), &state.colors),
        Err(e) => format!("{}: {}", "错误".color(state.colors.error).bold(), e),
    }
}

/// 把表达式求值成"非零整数"；求值失败（含自由变量的 `x^2-4`）或结果不是非零整数时返回 `None`
fn eval_nonzero_integer(expr: &parser::Expr, evaluator: &mut Evaluator) -> Option<Number> {
    let v = evaluator.evaluate(expr).ok()?;
    if v.is_zero() {
        return None;
    }
    let r = v.as_rational()?;
    if r.is_integer() {
        Some(Number::from_bigint(r.to_integer()))
    } else {
        None
    }
}

/// 把 `io::Error` 归纳成**可翻译**的一句话。
///
/// **不要**直接用 `Display` / `Debug`：`Display` 带操作系统区域设置——中文 Windows 上恒为中文，
/// 于是英文界面里会突然冒出一句中文（`failed to read file: 系统找不到指定的文件。 (os error 2)`）；
/// `Debug` 还会把 `Os { code: 2, kind: NotFound, .. }` 一大串原样打给用户。
/// 返回的是**简体原文**，由输出边界统一翻译（`println!` 那条要自己套 `i18n::t`）。
fn io_reason(err: &std::io::Error) -> &'static str {
    use std::io::ErrorKind::*;
    match err.kind() {
        NotFound => "文件不存在",
        PermissionDenied => "没有访问权限",
        InvalidData => "文件内容不是有效的文本",
        _ => "无法访问该文件",
    }
}

/// 处理 primefac(非零整数)：输出 `12 = 2^2 * 3` / `-12 = -2^2 * 3`
fn handle_primefac(n: &Number, state: &AppState) -> String {
    let Some(r) = n.as_rational() else {
        return format!(
            "{}: primefac 的参数必须是非零整数",
            "错误".color(state.colors.error).bold()
        );
    };
    match primefac::format_prime_factorization(&r.to_integer()) {
        Ok(text) => colorize_result(&text, &state.colors),
        Err(e) => format!("{}: {}", "错误".color(state.colors.error).bold(), e),
    }
}

/// 处理高等数学的**符号结果**（如 `diff(x^2, x)` → `= 2*x`）。
///
/// 前缀判定与数值结果同源：系数里出现 `Approx`（如 `taylor(sin(x), x, 0.1, 3)`）就是近似值，
/// 给 `≈`；全部精确给 `=`。符号结果**不写入 `ans`**（`ans` 只能装 `Number`）。
fn handle_symbolic(expr: &parser::Expr, state: &AppState) -> String {
    let mode = state.evaluator.display_mode;
    let body = calculus::render_expr(expr, mode);
    let prefix = calculus::expr_prefix(expr, mode);
    colorize_result(&format!("{} {}", prefix, body), &state.colors)
}

/// 处理"三角形求解"输入：边/角/高 → 全部量（多解时按 `解 N:` 分段）
fn handle_triangle(tri: &solver_triangle::TriangleInput, state: &mut AppState) -> String {
    let mode = state.evaluator.display_mode;
    let angle_mode = state.evaluator.angle_mode;
    match solver_triangle::solve_triangle(tri, &state.evaluator) {
        Ok(sols) => {
            let mut lines: Vec<String> = Vec::new();
            for (i, sol) in sols.iter().enumerate() {
                if sols.len() > 1 {
                    lines.push(format!("解 {}:", i + 1));
                }
                lines.extend(solver_triangle::format_triangle_solution(
                    sol, mode, angle_mode,
                ));
            }
            lines
                .into_iter()
                .map(|l| colorize_result(&l, &state.colors))
                .collect::<Vec<_>>()
                .join("\n")
        }
        Err(e) => format!("{}: {}", "错误".color(state.colors.error).bold(), e),
    }
}

/// 提取方程/方程组的未知数：`/let` 存储的全大写变量算已知常数，必须过滤掉
/// （`handle_equation` 与 `handle_system` 共用，避免两处判定规则不一致）
fn unknown_variables(expr: &parser::Expr, evaluator: &Evaluator) -> Vec<char> {
    equation::EquationInfo::extract(expr)
        .variables
        .into_iter()
        .filter(|v| !evaluator.vars.contains_key(&v.to_string()))
        .collect()
}

/// 处理"多项式拟合"输入：坐标（+ 可选解析式）→ 函数解析式（可能多行）
fn handle_fit(fit: &solver_fit::FitInput, state: &mut AppState) -> String {
    let mode = state.evaluator.display_mode;
    match solver_fit::solve_polynomial_fit(&fit.points, fit.template.as_ref(), &state.evaluator) {
        Ok(sol) => {
            let mut lines: Vec<String> = Vec::new();
            // 参数行（唯一解一行；欠定时每个主元参数一行 + 自由参数提示）
            for line in solver_fit::format_fit_params(&sol, mode) {
                lines.push(line);
            }
            // 一般式：恒为 `y = …`
            lines.push(solver_fit::format_fit_solution(&sol, mode));
            // 二次函数追加顶点式（系数全为常数、实际二次、首项非 0 且 m≠0 时才有）
            if sol.all_constant()
                && let Some(v) = solver_fit::format_vertex_form(&sol.coeffs, sol.var, mode)
            {
                lines.push(format!("y = {}", v));
            }
            lines
                .into_iter()
                .map(|l| colorize_result(&l, &state.colors))
                .collect::<Vec<_>>()
                .join("\n")
        }
        Err(e) => format!("{}: {}", "错误".color(state.colors.error).bold(), e),
    }
}

fn handle_equation(left: &parser::Expr, right: &parser::Expr, state: &mut AppState) -> String {
    // 化简: left - right = 0
    let eq_expr = parser::Expr::Binary(
        Box::new(left.clone()),
        parser::BinOp::Sub,
        Box::new(right.clone()),
    );

    // 提取变量（已存储的大写变量视为常数，不参与未知数判定）
    let variables = unknown_variables(&eq_expr, &state.evaluator);
    if variables.is_empty() {
        return format!("{}", "错误: 方程中没有变量".color(state.colors.error).bold());
    }
    if variables.len() > 1 {
        let vs: String = variables
            .iter()
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        return format!(
            "{}: 方程包含多个变量（{}），当前单一方程仅支持单变量；线性方程组请用逗号分隔输入",
            "错误".color(state.colors.error).bold(),
            vs
        );
    }

    let var = variables[0];

    // 尝试多项式求解
    if equation::is_polynomial(&eq_expr) {
        if let Some(coeffs) = solver_poly::extract_polynomial(&state.evaluator, &eq_expr, var) {
            // 恒等式：所有系数为 0（如 x-x=0、5=5）
            if coeffs.iter().all(|c| c.is_zero()) {
                return colorize_result("恒等式：对变量的任意取值均成立", &state.colors);
            }
            if coeffs.len() == 2 {
                // bx + c = 0 一次方程
                let b = &coeffs[1];
                let c = &coeffs[0];
                let solutions = solver_poly::solve_quadratic(&Number::from_int(0), b, c);
                return format_solutions(&solutions, var, state);
            } else if coeffs.len() == 3 {
                // ax^2 + bx + c = 0 二次方程
                let a = &coeffs[2];
                let b = &coeffs[1];
                let c = &coeffs[0];
                let solutions = solver_poly::solve_quadratic(a, b, c);
                return format_solutions(&solutions, var, state);
            } else {
                // 高次方程——分解后求全部根（含复数）
                match solver_poly::solve_poly_full(&coeffs) {
                    Ok(sols) if !sols.is_empty() => {
                        return format_solutions(&sols, var, state);
                    }
                    // 规模护栏明确拒绝（次数过高）：直接报错，不回退牛顿单根——
                    // 高次多项式的牛顿求值代价极高（x^5000 每次求值都要算 5000 次幂），
                    // 回退会让界面长时间无响应且只给出一个根
                    Err(msg) if msg.starts_with(solver_poly::DEGREE_GUARD_PREFIX) => {
                        return format!("{}: {}", "错误".color(state.colors.error).bold(), msg);
                    }
                    _ => {
                        if let Some(root) = newton_with_guesses(&state.evaluator, &eq_expr, var) {
                            let sol = solver_poly::PolySolution::Real(Number::Approx(root));
                            return colorize_result(
                                &format!("{} 的一个解: {} = {}",
                                    variables.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(","),
                                    var,
                                    solver_poly::format_solution(&sol, state.evaluator.display_mode)
                                ),
                                &state.colors,
                            );
                        } else {
                            return format!("{}", "未能找到实数根".color(state.colors.error).bold());
                        }
                    }
                }
            }
        }
    }

    // 非多项式方程：收集所有实根；若构成周期序列则输出通式，否则列出全部根
    let roots = solve_aux::collect_all_roots(&state.evaluator, &eq_expr, var);
    if roots.is_empty() {
        format!("{}", "未能找到实数根".color(state.colors.error).bold())
    } else if let Some(spec) =
        solve_aux::format_periodic_roots(&roots, var, state.evaluator.angle_mode)
    {
        colorize_result(&spec, &state.colors)
    } else {
        // 近似根：与简单分数/整数足够接近时回填精确值并按显示模式输出
        // （MathIO `1 / 2` 带 `=`；LineIO `0.5` 带 `=`、循环小数则 `≈`）
        let mode = state.evaluator.display_mode;
        let parts: Vec<String> = roots
            .iter()
            .map(|r| {
                let approx = r.to_approx();
                match solve_aux::float_to_exact_rational(&approx) {
                    Some((p, q)) => {
                        let exact = Number::from_rational(BigRational::new(
                            BigInt::from(p),
                            BigInt::from(q),
                        ));
                        let out = match mode {
                            DisplayMode::MathIO => display::format_mathio(&exact),
                            DisplayMode::LineIO => display::format_lineio(&exact),
                        };
                        format!(
                            "{} {} {}",
                            var,
                            solve_aux::result_prefix(&exact, mode),
                            out
                        )
                    }
                    None => format!(
                        "{} ≈ {}",
                        var,
                        display::format_lineio(r)
                    ),
                }
            })
            .collect();
        colorize_result(&parts.join(", "), &state.colors)
    }
}

/// 用牛顿法尝试多组初始猜测（由近及远，含 pi/2 的整数倍以覆盖三角函数零点）
fn newton_with_guesses(
    evaluator: &parser::Evaluator,
    expr: &parser::Expr,
    var: char,
) -> Option<crate::bigfloat::BigFloat> {
    let pi = crate::bigfloat::BigFloat::pi(crate::bigfloat::precision());
    let half_pi = crate::bigfloat::BigFloat::div(
        &pi,
        &crate::bigfloat::BigFloat::from_u64(2),
        crate::bigfloat::precision(),
    );

    // 0 与由近及远的整数点
    if let Some(root) = solver_poly::newton_solve(
        evaluator,
        expr,
        var,
        crate::bigfloat::BigFloat::from_u64(0),
        200,
    ) {
        return Some(root);
    }
    for i in 1i64..=10 {
        for sig in [1i64, -1i64] {
            let guess = crate::bigfloat::BigFloat::from_i64(sig * i);
            if let Some(root) = solver_poly::newton_solve(evaluator, expr, var, guess, 200) {
                return Some(root);
            }
        }
    }
    // 分数初值（±1/2、±3/2，覆盖如 1/x=2 的小根）
    let half = crate::bigfloat::BigFloat::div(
        &crate::bigfloat::BigFloat::from_u64(1),
        &crate::bigfloat::BigFloat::from_u64(2),
        crate::bigfloat::precision(),
    );
    for s in [1i64, -1, 3, -3] {
        let guess = crate::bigfloat::BigFloat::mul(
            &crate::bigfloat::BigFloat::from_i64(s),
            &half,
            crate::bigfloat::precision(),
        );
        if let Some(root) = solver_poly::newton_solve(evaluator, expr, var, guess, 200) {
            return Some(root);
        }
    }
    // pi/2 的整数倍
    for k in 1i64..=6 {
        for sig in [1i64, -1i64] {
            let guess = crate::bigfloat::BigFloat::mul(
                &crate::bigfloat::BigFloat::from_i64(sig * k),
                &half_pi,
                crate::bigfloat::precision(),
            );
            if let Some(root) = solver_poly::newton_solve(evaluator, expr, var, guess, 200) {
                return Some(root);
            }
        }
    }
    None
}

/// /let：存储变量（变量名必须全大写字母）
fn handle_let(input: &str, state: &mut AppState) {
    // 去掉 '/' 与命令词本身（大小写不敏感），只保留参数部分：
    // 不能"跳到第一个大写字母"——那会让 `/let 1A=5` 被静默当成 `A=5` 存下来。
    let after_slash = input.trim().trim_start_matches('/');
    let cmd_len = after_slash.chars().take_while(|c| c.is_ascii_alphabetic()).count();
    let body = after_slash[cmd_len..].trim();
    let Some((name, expr)) = body.split_once('=') else {
        lprint!(
            "{}: {}",
            "用法".color(state.colors.prompt),
            colorize_text("/let <大写变量名> = <表达式>", &state.colors, true)
        );
        return;
    };
    let name = name.trim();
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
        lprint!("{}: 变量名必须为全大写字母（如 X、AB）", "错误".color(state.colors.error).bold());
        return;
    }
    match parse_and_eval(expr.trim(), &mut state.evaluator) {
        Ok(EvalResult::Value(v)) | Ok(EvalResult::SdValue(v)) => {
            let value_str = fmt_value(&v, state.evaluator.display_mode);
            state.evaluator.vars.insert(name.to_string(), v);
            persist_state(state);
            lprint!(
                "{}",
                colorize_result(&format!("{} {}", name, value_str), &state.colors)
            );
        }
        Ok(_) => {
            lprint!("{}: /let 只能存储数值表达式的值", "错误".color(state.colors.error).bold());
        }
        Err(e) => {
            lprint!("{}: {}", "错误".color(state.colors.error).bold(), e);
        }
    }
}

/// /var：列出所有存储变量（变量名用常量色、值用结果色）
fn handle_vars(state: &AppState) {
    if state.evaluator.vars.is_empty() {
        lprint!("{}", "暂无存储变量".dimmed());
        return;
    }
    for (name, v) in &state.evaluator.vars {
        let value_str = fmt_value(v, state.evaluator.display_mode);
        lprint!(
            "{} {}",
            name.color(state.colors.constant).bold(),
            colorize_result(&value_str, &state.colors)
        );
    }
}

/// /del：删除单个存储变量；`/del all` 清空全部变量
fn handle_del(parts: Vec<&str>, state: &mut AppState) {
    if parts.len() < 2 {
        lprint!(
            "{}: {}",
            "用法".color(state.colors.prompt),
            colorize_text("/del <大写变量名> 或 /del all", &state.colors, true)
        );
        return;
    }
    // 严格匹配小写 all 才清空：名为 ALL 的变量仍可用 /del ALL 单独删除
    if parts[1] == "all" {
        let n = state.evaluator.vars.len();
        if n == 0 {
            lprint!("{}: 暂无存储变量", "提示".color(state.colors.prompt));
            return;
        }
        state.evaluator.vars.clear();
        persist_state(state);
        lprint!(
            "{}",
            format!("已删除全部 {} 个变量", n).color(state.colors.result).bold()
        );
        return;
    }
    let name = parts[1];
    if !name.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
        lprint!("{}: 变量名必须为全大写字母（如 X、AB），或用 /del all 清空", "错误".color(state.colors.error).bold());
        return;
    }
    if state.evaluator.vars.remove(name).is_some() {
        persist_state(state);
        lprint!("{}", format!("已删除变量 {}", name).color(state.colors.result).bold());
    } else {
        lprint!("{}: 变量 {} 不存在", "提示".color(state.colors.prompt), name);
    }
}

/// 落盘用的耗时开关：**必须**取 `timing_base`，而非 `calc_mode::timing_enabled()`。
/// 后者会被命令行 `-q/--no-timing` 临时改成 `false`；若直接落盘，一次 `-q` 运行就会把
/// 用户配置里的 `timing=on` 永久改写成 `off`（表现为"计时功能突然不显示了"）。
/// 与 `lang_base` 同理：命令行覆盖只作用于本次会话。抽成函数是为了让单元测试能守住这个不变量。
fn timing_for_persist(state: &AppState) -> bool {
    state.timing_base
}

/// 保存会话状态（显示/角度/计算模式 + 变量 + /set 颜色）；失败时只提示一次，不打断使用
fn persist_state(state: &AppState) {
    use std::sync::Once;
    static WARNED: Once = Once::new();
    let r = state::save(
        state.evaluator.display_mode,
        state.evaluator.angle_mode,
        state::current_calc_mode(),
        state.lang_base,
        timing_for_persist(state),
        bigfloat::precision(),
        bigfloat::display_digits(),
        bigfloat::sci_allowed(),
        bigfloat::group_enabled(),
        &state.evaluator.vars,
        &state.colors.to_pairs(),
    );
    if let Err(e) = r {
        WARNED.call_once(|| {
            leprint!(
                "{}: 会话状态保存失败（{}），本次的设置与变量不会在重启后保留",
                "提示".color(state.colors.prompt),
                e
            );
        });
    }
}

/// 按显示模式与精确性给数值加前缀与格式化（"= 7" / "≈ 3.14…"）
fn fmt_value(v: &Number, mode: parser::DisplayMode) -> String {
    let out = match mode {
        parser::DisplayMode::MathIO => display::format_mathio(v),
        parser::DisplayMode::LineIO => display::format_lineio(v),
    };
    let p = solve_aux::result_prefix(v, mode);
    format!("{} {}", p, out)
}

/// 历史文件路径（%USERPROFILE% 或 $HOME 下的 .hipercalc_history）
fn history_path() -> Option<std::path::PathBuf> {
    let dir = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()?;
    Some(std::path::PathBuf::from(dir).join(".hipercalc_history"))
}

fn handle_system(equations: &[(Box<parser::Expr>, Box<parser::Expr>)], state: &mut AppState) -> String {
    if equations.is_empty() {
        return String::new();
    }

    // 第一步：收集所有变量、移项后的方程，并判断是否为纯线性方程组
    let mut all_vars: Vec<char> = Vec::new();
    let mut eq_exprs: Vec<parser::Expr> = Vec::new();
    let mut all_linear = true;
    let mut per_eq: Vec<(Vec<(char, Number)>, Number)> = Vec::new(); // 线性路径各方程 (变量系数, 常数) 原始对

    for (left, right) in equations {
        let expr = parser::Expr::Binary(
            Box::new(left.as_ref().clone()),
            parser::BinOp::Sub,
            Box::new(right.as_ref().clone()),
        );
        let evars = unknown_variables(&expr, &state.evaluator);
        for v in &evars {
            if !all_vars.contains(v) {
                all_vars.push(*v);
            }
        }
        if equation::is_linear(&expr) {
            match extract_linear_from_expr(&state.evaluator, &expr) {
                Some((coeffs, cnst, _)) => per_eq.push((coeffs, cnst)),
                None => all_linear = false,
            }
        } else {
            all_linear = false;
        }
        eq_exprs.push(expr);
    }

    if all_vars.is_empty() {
        return String::new();
    }
    all_vars.sort();

    if all_linear {
        // 精确高斯消元（按排序后的变量顺序构建矩阵，修复列错位问题）
        let mut rows: Vec<Vec<Number>> = Vec::new();
        let mut constants: Vec<Number> = Vec::new();
        for (coeffs, cnst) in per_eq {
            let mut row = vec![Number::from_int(0); all_vars.len()];
            for (v, c) in &coeffs {
                if let Some(pos) = all_vars.iter().position(|x| x == v) {
                    row[pos] = c.clone();
                }
            }
            rows.push(row);
            constants.push(cnst);
        }

        let mut out: Vec<String> = Vec::new();
        if rows.len() != all_vars.len() {
            out.push(format!(
                "{}: 方程数量({})与变量数量({})不匹配",
                "提示".yellow(),
                rows.len(),
                all_vars.len()
            ));
        }

        let mut matrix: Vec<Vec<Number>> = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            let mut aug_row = row.clone();
            aug_row.push(constants[i].clone());
            matrix.push(aug_row);
        }

        if let Some(solution) = solver_linear::gaussian_elimination(&mut matrix, &all_vars) {
            if solution.infinite {
                out.push(format!(
                    "{}",
                    "方程组有无穷多解".color(state.colors.error).bold()
                ));
            } else {
                let output = solver_linear::format_linear_solution(
                    &solution,
                    state.evaluator.display_mode,
                );
                out.push(colorize_result(&output, &state.colors));
            }
        } else {
            out.push(format!("{}", "方程组无解".color(state.colors.error).bold()));
        }
        return out.join("\n");
    }

    // 非线性方程组：数值多维牛顿
    if eq_exprs.len() != all_vars.len() {
        return format!(
            "{}: 非线性方程组暂支持方程数 = 变量数（当前 {}/{})",
            "提示".yellow(),
            eq_exprs.len(),
            all_vars.len()
        );
    }
    if all_vars.len() > 3 {
        return format!(
            "{}: 非线性方程组暂支持不超过 3 个变量",
            "提示".yellow()
        );
    }

    let sols = solver_nonlinear::solve_system(&state.evaluator, &eq_exprs, &all_vars);
    if sols.is_empty() {
        return format!("{}", "未找到实数解".color(state.colors.error).bold());
    }
    let eps = crate::bigfloat::BigFloat::div(
        &crate::bigfloat::BigFloat::from_u64(1),
        &BigInt::from(10).pow(14).into(),
        crate::bigfloat::precision(),
    );
    let mut lines: Vec<String> = Vec::new();
    for sol in sols {
        let parts: Vec<String> = all_vars
            .iter()
            .zip(sol.iter())
            .map(|(v, b)| {
                // 近零分量显示为 0
                let shown = if b.value.abs() <= eps.value.abs() {
                    crate::bigfloat::BigFloat::from_u64(0)
                } else {
                    b.clone()
                };
                format!(
                    "{} ≈ {}",
                    v,
                    shown.to_significant_string(crate::bigfloat::display_digits())
                )
            })
            .collect();
        lines.push(colorize_result(&parts.join(", "), &state.colors));
    }
    lines.join("\n")
}

/// 从线性表达式提取系数: 返回 (变量系数列表, 常数项, 变量列表)
fn extract_linear_from_expr(
    evaluator: &Evaluator,
    expr: &parser::Expr,
) -> Option<(Vec<(char, Number)>, Number, Vec<char>)> {
    let mut var_coeffs: Vec<(char, Number)> = Vec::new();
    let mut const_term = Number::from_int(0);
    let mut vars_seen: Vec<char> = Vec::new();

    extract_linear_rec(evaluator, expr, &mut var_coeffs, &mut const_term, &mut vars_seen)?;

    // 常数项移到右边: (表达式 = 0) => 变量项 = -常数
    Some((var_coeffs, const_term.neg(), vars_seen))
}

fn extract_linear_rec(
    evaluator: &Evaluator,
    expr: &parser::Expr,
    coeffs: &mut Vec<(char, Number)>,
    const_term: &mut Number,
    vars: &mut Vec<char>,
) -> Option<()> {
    match expr {
        parser::Expr::Number(n) => {
            *const_term = const_term.add(n);
            Some(())
        }
        parser::Expr::Variable(name) => {
            // 已存储变量（/let）与 ans 按数值代入常数项，与 handle_equation 的规则一致。
            // 旧实现把它们当未知数（或被静默丢弃），导致 `x+A=5, y=1`（A=4）解成 x=5。
            if name == "ans" {
                *const_term = const_term.add(&evaluator.ans);
                return Some(());
            }
            if let Some(v) = evaluator.vars.get(name) {
                *const_term = const_term.add(v);
                return Some(());
            }
            if name.len() == 1 {
                let ch = name.chars().next().unwrap();
                if !vars.contains(&ch) { vars.push(ch); }
                add_coeff(coeffs, ch, Number::from_int(1));
                Some(())
            } else {
                None
            }
        }
        parser::Expr::Binary(left, op, right) => {
            match op {
                parser::BinOp::Add => {
                    extract_linear_rec(evaluator, left, coeffs, const_term, vars)?;
                    extract_linear_rec(evaluator, right, coeffs, const_term, vars)?;
                    Some(())
                }
                parser::BinOp::Sub => {
                    extract_linear_rec(evaluator, left, coeffs, const_term, vars)?;
                    let mut right_coeffs = Vec::new();
                    let mut right_const = Number::from_int(0);
                    let mut right_vars = Vec::new();
                    extract_linear_rec(evaluator, right, &mut right_coeffs, &mut right_const, &mut right_vars)?;
                    for (v, c) in right_coeffs {
                        for ch in &right_vars { if !vars.contains(ch) { vars.push(*ch); } }
                        add_coeff(coeffs, v, c.neg());
                    }
                    *const_term = const_term.sub(&right_const);
                    Some(())
                }
                parser::BinOp::Mul => {
                    let l_num = extract_const(left);
                    let r_num = extract_const(right);
                    let l_var = extract_var_name(left);
                    let r_var = extract_var_name(right);

                    // 数字在左、变量在右: 2*x
                    if let (Some(n), Some(v)) = (&l_num, &r_var) {
                        if !vars.contains(v) { vars.push(*v); }
                        add_coeff(coeffs, *v, n.clone());
                        Some(())
                    // 变量在左、数字在右: x*2
                    } else if let (Some(v), Some(n)) = (&l_var, &r_num) {
                        if !vars.contains(v) { vars.push(*v); }
                        add_coeff(coeffs, *v, n.clone());
                        Some(())
                    // 常数×常数（如 2*3）：乘起来并入常数项
                    } else if let (Some(n1), Some(n2)) = (&l_num, &r_num) {
                        *const_term = const_term.add(&n1.mul(n2));
                        Some(())
                    } else {
                        None
                    }
                }
                // 除以非零常数（如 x/2）：把左侧线性项系数整体除以常数
                parser::BinOp::Div => {
                    let den = extract_const(right)?;
                    if den.is_zero() {
                        return None;
                    }
                    let mut inner_coeffs = Vec::new();
                    let mut inner_const = Number::from_int(0);
                    let mut inner_vars = Vec::new();
                    extract_linear_rec(evaluator, left, &mut inner_coeffs, &mut inner_const, &mut inner_vars)?;
                    for (v, c) in inner_coeffs {
                        for ch in &inner_vars { if !vars.contains(ch) { vars.push(*ch); } }
                        add_coeff(coeffs, v, c.div(&den));
                    }
                    *const_term = const_term.add(&inner_const.div(&den));
                    Some(())
                }
            }
        }
        parser::Expr::Unary(op, e) => {
            match op {
                parser::UnaryOp::Pos => extract_linear_rec(evaluator, e, coeffs, const_term, vars),
                parser::UnaryOp::Neg => {
                    let mut inner_coeffs = Vec::new();
                    let mut inner_const = Number::from_int(0);
                    let mut inner_vars = Vec::new();
                    extract_linear_rec(evaluator, e, &mut inner_coeffs, &mut inner_const, &mut inner_vars)?;
                    for (v, c) in inner_coeffs {
                        for ch in &inner_vars { if !vars.contains(ch) { vars.push(*ch); } }
                        add_coeff(coeffs, v, c.neg());
                    }
                    *const_term = const_term.sub(&inner_const);
                    Some(())
                }
            }
        }
        _ => None,
    }
}

fn add_coeff(coeffs: &mut Vec<(char, Number)>, var: char, coeff: Number) {
    for (v, c) in coeffs.iter_mut() {
        if *v == var {
            *c = c.add(&coeff);
            return;
        }
    }
    coeffs.push((var, coeff));
}

fn extract_const(expr: &parser::Expr) -> Option<Number> {
    match expr {
        parser::Expr::Number(n) => Some(n.clone()),
        parser::Expr::Unary(op, e) => {
            let n = extract_const(e)?;
            match op {
                parser::UnaryOp::Pos => Some(n),
                parser::UnaryOp::Neg => Some(n.neg()),
            }
        }
        _ => None,
    }
}

fn extract_var_name(expr: &parser::Expr) -> Option<char> {
    match expr {
        parser::Expr::Variable(name) if name.len() == 1 => Some(name.chars().next().unwrap()),
        _ => None,
    }
}

fn format_solutions(solutions: &[solver_poly::PolySolution], var: char, state: &AppState) -> String {
    if solutions.is_empty() {
        return colorize_result("无实数解", &state.colors);
    }
    let mode = state.evaluator.display_mode;
    let parts: Vec<String> = solutions.iter().map(|s| {
        // 前缀按显示模式判定：MathIO 下精确根为 `=`；LineIO 下只有能完整写出
        // 的有限小数/整数才是 `=`（如 sqrt(2) 的小数展开应标 `≈`）
        let sign = solution_prefix(s, mode);
        format!("{} {} {}", var, sign, solver_poly::format_solution(s, mode))
    }).collect();
    // 整体着色（而不是逐个根着色后再拼接）：这样分隔用的 `, ` 也会落在 result 底色上
    colorize_result(&parts.join(", "), &state.colors)
}

/// 根的精确度前缀：所有分量按显示模式都"能精确呈现"才是 `=`，否则 `≈`
fn solution_prefix(s: &solver_poly::PolySolution, mode: DisplayMode) -> &'static str {
    match s {
        solver_poly::PolySolution::Real(n) => solve_aux::result_prefix(n, mode),
        solver_poly::PolySolution::Complex(re, im) => {
            if solve_aux::result_prefix(re, mode) == "="
                && solve_aux::result_prefix(im, mode) == "="
            {
                "="
            } else {
                "≈"
            }
        }
    }
}

fn mode_info(state: &AppState) -> String {
    let disp = match state.evaluator.display_mode {
        DisplayMode::MathIO => "MathIO",
        DisplayMode::LineIO => "LineIO",
    };
    let angle = match state.evaluator.angle_mode {
        trig::AngleMode::Radian => "Radian",
        trig::AngleMode::Degree => "Degree",
    };
    // 末尾附加当前界面语言（母语写法），便于一眼看出当前语言设置；
    // 精度/显示位数/显示开关只在非默认时追加，避免默认横幅变长
    let mut s = format!(
        "{}/{}/{}/{}",
        disp,
        angle,
        calc_mode::name(),
        i18n::get().native_name()
    );
    if bigfloat::precision() != bigfloat::DEFAULT_PRECISION
        || bigfloat::display_digits() != bigfloat::DEFAULT_DISPLAY_DIGITS
    {
        s.push_str(&format!(
            "/{}/{}",
            bigfloat::precision(),
            bigfloat::display_digits()
        ));
    }
    if !bigfloat::sci_allowed() {
        s.push_str("/sci-off");
    }
    if bigfloat::group_enabled() {
        s.push_str("/group");
    }
    s
}

fn clear_screen() {
    if cfg!(target_os = "windows") {
        let _ = std::process::Command::new("cmd")
            .args(["/c", "cls"])
            .status();
    } else {
        let _ = std::process::Command::new("clear").status();
    }
}

/// /help 正文（无边框简版）。正文按类别着色：
/// 函数（functions）、指令（commands）、常量（constants）、数字（numbers）、
/// 运算符（operators）、括号（brackets）各用对应颜色；【…】小节标题用 result 颜色加粗。
/// `/lang` `/language`：切换界面语言。
/// - 带参数（`/lang en`、`/lang 2`、`/lang 繁體中文`）→ 直接切换；
/// - 不带参数 → 输出三行语言列表，交互终端下用 ↑/↓ 移动箭头、回车确认；
///   管道/重定向时退化为"输入序号或语言代码后回车"（空行 = 确认当前选择）。
fn handle_lang(arg: Option<&str>, state: &mut AppState) {
    if let Some(a) = arg {
        match i18n::Lang::parse(a) {
            Some(l) => set_language(l, state),
            None => lprint!(
                "{}: 无法识别的语言，可选: 1 简体中文 / 2 繁體中文 / 3 English",
                "错误".color(state.colors.error).bold()
            ),
        }
        return;
    }

    let current = i18n::get();
    let chosen = match lang_menu_raw(current, &state.colors) {
        // 交互终端：↑/↓ 菜单
        Some(sel) => sel,
        // 其它情况：打印菜单 + 读一行
        None => {
            print_lang_menu(current, state);
            let mut line = String::new();
            if std::io::stdin().read_line(&mut line).is_err() {
                return;
            }
            let line = line.trim();
            if line.is_empty() {
                current
            } else {
                match i18n::Lang::parse(line) {
                    Some(l) => l,
                    None => {
                        lprint!(
                            "{}: 无法识别的语言，可选: 1 简体中文 / 2 繁體中文 / 3 English",
                            "错误".color(state.colors.error).bold()
                        );
                        return;
                    }
                }
            }
        }
    };
    set_language(chosen, state);
}

/// 应用语言：立即生效于后续输出（并提示仅影响后续输出）
fn set_language(lang: i18n::Lang, state: &mut AppState) {
    i18n::set(lang);
    // 显式选择要写进配置（与命令行 --lang 的临时覆盖区分开）
    state.lang_base = lang;
    lprint!(
        "{}",
        format!("已切换语言：{}（仅对后续输出生效）", lang.native_name())
            .color(state.colors.result)
            .bold()
    );
}

/// 打印三行语言列表，当前选择前方用箭头标出
fn print_lang_menu(current: i18n::Lang, state: &AppState) {
    lprint!("{}", "语言 / Language:".color(state.colors.prompt).bold());
    for l in i18n::Lang::ALL {
        // 箭头用运算符颜色高亮（与 /set operators 同色）；名字本身按当前选择用 result 色
        let arrow = if l == current { "❯" } else { " " };
        let name = if l == current {
            l.native_name().color(state.colors.result).bold().to_string()
        } else {
            l.native_name().to_string()
        };
        lprint!("  {} {}", arrow.color(state.colors.operator).bold(), name);
    }
    lprint!(
        "{}",
        "↑/↓ 选择，回车确认（也可直接输入序号或 /lang <代码>）".dimmed()
    );
}

/// 循环移动选择（越界回到另一端）
fn step_lang(cur: i18n::Lang, delta: i32) -> i18n::Lang {
    let all = i18n::Lang::ALL;
    let idx = all.iter().position(|l| *l == cur).unwrap_or(0) as i32;
    let n = all.len() as i32;
    all[((idx + delta).rem_euclid(n)) as usize]
}

/// 交互终端下的语言菜单：↑/↓ 移动、回车确认、Esc 取消。
/// 返回 None 表示无法使用原始按键（管道、非 Windows、控制台模式设置失败）⇒ 调用方走文本路径。
fn lang_menu_raw(current: i18n::Lang, colors: &ColorConfig) -> Option<i18n::Lang> {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return None;
    }
    let prev_mode = rawkey::enter_raw()?;
    let mut sel = current;
    let mut first = true;
    let result = loop {
        print_lang_menu_raw(sel, colors, first);
        first = false;
        match rawkey::read_key() {
            Some(rawkey::Key::Up) => sel = step_lang(sel, -1),
            Some(rawkey::Key::Down) => sel = step_lang(sel, 1),
            Some(rawkey::Key::Enter) => break Some(sel),
            Some(rawkey::Key::Esc) => break None,
            Some(rawkey::Key::Other) | None => {}
        }
    };
    rawkey::leave_raw(prev_mode);
    result
}

/// 语言菜单的行数：表头 + 3 个语言 + 提示行。
/// 重画时按这个高度上移；数量与实际打印行数不一致就会一次比一次多出一份菜单。
const LANG_MENU_LINES: usize = 5;

/// 原始按键模式下的菜单：整体原地重画（每行都清行后重写，且都以换行结束）。
/// 语言名一律用各自的母语写法（不翻译）；箭头用运算符颜色高亮（与 /set operators 同色）。
fn print_lang_menu_raw(sel: i18n::Lang, colors: &ColorConfig, first: bool) {
    let mut out = String::new();
    if !first {
        // 上移整个菜单高度，回到表头所在行再重写
        out.push_str(&format!("\x1b[{}A", LANG_MENU_LINES));
    }
    out.push_str(&format!("\r\x1b[2K{}\n", i18n::t("语言 / Language:")));
    for l in i18n::Lang::ALL {
        let arrow = if l == sel { "❯" } else { " " };
        out.push_str(&format!(
            "\r\x1b[2K  {} {}\n",
            arrow.color(colors.operator).bold(),
            l.native_name()
        ));
    }
    out.push_str(&format!("\r\x1b[2K{}\n", i18n::t("↑/↓ 选择，回车确认")));
    print!("{}", out);
    use std::io::Write;
    let _ = std::io::stdout().flush();
}

/// 原始按键读取（仅 Windows 控制台；其它平台/环境返回 None 走文本路径）。
/// 这里直接声明所需 Win32 API，避免为此引入 windows-sys/console 等依赖。
#[cfg(windows)]
mod rawkey {
    pub enum Key {
        Up,
        Down,
        Enter,
        Esc,
        Other,
    }

    type Handle = *mut core::ffi::c_void;
    const STD_INPUT_HANDLE: u32 = -10i32 as u32;
    const ENABLE_PROCESSED_INPUT: u32 = 0x0001;
    const ENABLE_LINE_INPUT: u32 = 0x0002;
    const ENABLE_ECHO_INPUT: u32 = 0x0004;
    const KEY_EVENT: u16 = 0x0001;
    const VK_UP: u16 = 0x26;
    const VK_DOWN: u16 = 0x28;
    const VK_RETURN: u16 = 0x0D;
    const VK_ESCAPE: u16 = 0x1B;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct KeyEventRecord {
        key_down: i32,
        repeat: u16,
        virtual_key: u16,
        scan: u16,
        ch: u16,
        ctrl: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    union EventUnion {
        key: KeyEventRecord,
        raw: [u32; 4],
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct InputRecord {
        kind: u16,
        _pad: u16,
        ev: EventUnion,
    }

    unsafe extern "system" {
        fn GetStdHandle(n: u32) -> Handle;
        fn GetConsoleMode(h: Handle, mode: *mut u32) -> i32;
        fn SetConsoleMode(h: Handle, mode: u32) -> i32;
        fn ReadConsoleInputW(h: Handle, buf: *mut InputRecord, len: u32, read: *mut u32) -> i32;
    }

    /// 关闭行输入/回显/按键预处理，返回原模式（失败返回 None）
    pub fn enter_raw() -> Option<u32> {
        // INPUT_RECORD = EventType(2) + 对齐(2) + KEY_EVENT_RECORD(16) = 20 字节
        debug_assert_eq!(std::mem::size_of::<InputRecord>(), 20);
        unsafe {
            let h = GetStdHandle(STD_INPUT_HANDLE);
            let mut mode: u32 = 0;
            if GetConsoleMode(h, &mut mode) == 0 {
                return None;
            }
            let raw = mode & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT);
            if SetConsoleMode(h, raw) == 0 {
                return None;
            }
            Some(mode)
        }
    }

    /// 还原控制台模式
    pub fn leave_raw(prev: u32) {
        unsafe {
            let h = GetStdHandle(STD_INPUT_HANDLE);
            let _ = SetConsoleMode(h, prev);
        }
    }

    /// 读一个按键（忽略非按键事件与按键抬起）
    pub fn read_key() -> Option<Key> {
        unsafe {
            let h = GetStdHandle(STD_INPUT_HANDLE);
            let mut rec: InputRecord = std::mem::zeroed();
            loop {
                let mut read: u32 = 0;
                if ReadConsoleInputW(h, &mut rec, 1, &mut read) == 0 || read == 0 {
                    return None;
                }
                if rec.kind != KEY_EVENT {
                    continue;
                }
                let k = rec.ev.key;
                if k.key_down == 0 || k.repeat == 0 {
                    continue;
                }
                return Some(match k.virtual_key {
                    VK_UP => Key::Up,
                    VK_DOWN => Key::Down,
                    VK_RETURN => Key::Enter,
                    VK_ESCAPE => Key::Esc,
                    _ => Key::Other,
                });
            }
        }
    }
}

/// 非 Windows：无原始按键支持，一律走文本输入路径。
///
/// **`Key` 的变体必须与 Windows 版保持完全一致**——`lang_menu_raw` 是两份实现共享的，
/// 里面要匹配 `Key::Up/Down/Enter/Esc`。早先这里只声明了 `Other`，于是所有非 Windows 平台
/// 都在 `lang_menu_raw` 处报 `error[E0599]: no variant ... named Up`，
/// CI 上 Linux/macOS 五个目标全挂、只有 Windows 通过（Windows 分支的变体是全的）。
#[cfg(not(windows))]
mod rawkey {
    /// 与 Windows 版同名的变体。本平台 `read_key` 恒返回 `None`，因此这些变体永远不会被构造，
    /// 但类型必须存在，否则共享的 `lang_menu_raw` 无法编译。
    #[allow(dead_code)]
    pub enum Key {
        Up,
        Down,
        Enter,
        Esc,
        Other,
    }
    pub fn enter_raw() -> Option<u32> {
        None
    }
    pub fn leave_raw(_prev: u32) {}
    pub fn read_key() -> Option<Key> {
        None
    }
}

/// 繁體中文帮助（结构与 HELP_TEXT 一致）
const HELP_TEXT_TW: &str = "\
HiPerCalc 超高精度命令列計算器（直接輸入算式計算；/exit 離開，Ctrl+C 清行、連按兩次離開）

【基本運算】
  + - * / ^ !        加 減 乘 除、幂（右結合）、階乘（優先於幂）
  ( ) | |            括號、絕對值（2|x|、|x-3|）
  ans                上一次結果；隱式乘法 3x、2(x+1)；科學記號 1e3、2.5e-2

【函數】
  sqr sqrt abs       開方、絕對值
  ln exp             自然對數、自然指數
  log(b,x) log10 log2                任意底、常用、以 2 為底對數
  floor ceil round frac sign         取整、小數部分、符號
  sin cos tan cot sec csc            三角函數（受角度模式影響）
  arcsin arccos arctan arccot arcsec arccsc
  sinh cosh tanh     雙曲函數；特殊角與反三角特殊值回傳精確值
  gcd(a,b) lcm(a,b)  最大公因數 / 最小公倍數（須為非零整數，結果恆非負）

【常數與精確度】
  pi π e             內部 80 位小數，顯示 20 位有效數字

【方程 / 方程組】
  x^2-4=0            單方程：多項式精確/數值求全部根（含複數），三角方程給 k·π 通式
  x+y=5, 2x-y=1      逗號分隔方程組（線性高斯消去、非線性多維牛頓）

【因式分解 / 質因數分解 / 顯示轉換 / 三角形】（須為最外層函數）
  fac(x^2-4)         因式分解（factor 等價）；整數入參做質因數分解；MathIO 實數域、LineIO 有理數域
  primefac(12)       質因數分解：12 = 2^2 * 3（負數寫成 -12 = -2^2 * 3）
  sd(1/3)            顯示轉換：MathIO 轉小數、LineIO 轉符號
  triangle(a=3 b=4 c=5)  三角形求解：邊/角/高 → 全部量（可逗號分隔；只能最外層）

【高等數學】（可參與運算：diff(x^2,x)+1 合法；精確值給 =，數值回退給 ≈）
  diff(f, x)         求導：diff(x^2, x) → 2*x、diff(sin(x), x) → cos(x)（變數須為單個字母）
  int(f, x)          不定積分：int(x^2, x) → 1 / 3 * x^3（省略積分常數）
  int(f, x, a, b)    定積分：int(x^2, x, 0, 1) → 1 / 3；無初等原函數時自動數值積分（上下限可寫 inf）
  lim(f, x, a)       極限：lim(sin(x)/x, x, 0) → 1、lim(1/x, x, inf) → 0（a 可為 inf）
  taylor(f, x, a, n) 泰勒展開（a=0 即麥克勞林）：taylor(sin(x), x, 0, 5) → x - 1 / 6 * x^3 + 1 / 120 * x^5（n 為最高次數，不含餘項）
  sum(f, k, a, b)    求和：sum(k, k, 1, 100) → 5050、sum(k, k, 1, 1000000) → 500000500000（先閉式後逐項）
  prod(f, k, a, b)   求積：prod(k, k, 1, 10) → 3628800
  ── 鍵盤與輸入 ──
  Tab                補全函數/指令/常數/變數；函數補成 name() 並把游標放進括號
  Ctrl+C             計算中：中斷當前運算；空閒：離開程式
  Ctrl+L             清屏          Ctrl+R  反向搜尋歷史
  括號               游標處的括號與其配對括號加粗；不合法的括號標紅（不擋 Enter）
  輸入法             全角（）、。，等自動轉成半角

【變數儲存】
  /let A = 5         儲存變數（全大寫名），在算式、方程、fac 中自動取值
  /var               檢視全部；/del A 刪除單一；/del all 清除
  變數、模式、顏色保存在使用者家目錄的 .hipercalc_state，刪除即恢復預設

【模式】
  /mode mathio|lineio   數學顯示（符號優先）、線性顯示（一律小數）
  /mode deg|rad         角度制、弧度制
  /mode fast|deep       快速（規模保護）、死算（完整精度）；/mode 檢視目前
  /lang zh-CN|zh-TW|en  介面語言（簡體/繁體/英文），也可直接輸入 /lang 選擇

【指令】
  /help /clear /mode /lang /timing /set /let /var /del /reset /save /load /exit /quit

【顏色設定】
  /set <類別> <顏色>   僅輸入 /set 檢視用法與目前的各類顏色
  類別: functions(函數) operators(運算子) commands(指令) brackets(括號)
        constants(常數) numbers(數字) prompt(提示字元) result(結果) error(錯誤)

";

/// English help (same layout as HELP_TEXT)
const HELP_TEXT_EN: &str = "\
HiPerCalc - ultra-precision CLI calculator (enter an expression to compute; /exit to quit, Ctrl+C clears the line, twice to exit)

[Basics]
  + - * / ^ !        add, sub, mul, div, power (right-assoc), factorial (binds tighter than ^)
  ( ) | |            parentheses, absolute value (2|x|, |x-3|)
  ans                previous result; implicit multiplication 3x, 2(x+1); scientific 1e3, 2.5e-2

[Functions]
  sqr sqrt abs       square root, absolute value
  ln exp             natural log / exponential
  log(b,x) log10 log2                arbitrary base, base-10, base-2
  floor ceil round frac sign         floor, ceil, round, fraction, sign
  sin cos tan cot sec csc            trigonometric (follows the angle mode)
  arcsin arccos arctan arccot arcsec arccsc
  sinh cosh tanh     hyperbolic; special angles and inverse values stay exact
  gcd(a,b) lcm(a,b)  greatest common divisor / least common multiple (non-zero integers; never negative)

[Constants & precision]
  pi π e             80 decimal digits internally, 20 significant digits on screen

[Equations / systems]
  x^2-4=0            single equation: exact/numeric roots (incl. complex); trig equations get a k·π family
  x+y=5, 2x-y=1      comma-separated system (Gaussian elimination, multi-dim Newton)

[Factoring / prime factorization / display conversion / triangle] (must be the outermost function)
  fac(x^2-4)         factor (same as factor); integer arguments get prime factorization; MathIO over the reals, LineIO over the rationals
  primefac(12)       prime factorization: 12 = 2^2 * 3 (negatives: -12 = -2^2 * 3)
  sd(1/3)            display conversion: MathIO -> decimal, LineIO -> symbolic
  triangle(a=3 b=4 c=5)  triangle solver: sides/angles/heights -> everything (commas OK; outermost only)

[Calculus] (usable inside expressions, e.g. diff(x^2,x)+1; exact gives =, numeric fallback gives ~)
  diff(f, x)         derivative: diff(x^2, x) -> 2*x, diff(sin(x), x) -> cos(x) (single-letter variable)
  int(f, x)          antiderivative: int(x^2, x) -> 1 / 3 * x^3 (integration constant omitted)
  int(f, x, a, b)    definite integral: int(x^2, x, 0, 1) -> 1 / 3; numeric fallback when needed (bounds may be inf)
  lim(f, x, a)       limit: lim(sin(x)/x, x, 0) -> 1, lim(1/x, x, inf) -> 0 (a may be inf)
  taylor(f, x, a, n) Taylor series (a=0 gives Maclaurin): taylor(sin(x), x, 0, 5) -> x - 1/6*x^3 + 1/120*x^5 (n = highest degree, no remainder)
  sum(f, k, a, b)    summation: sum(k, k, 1, 100) -> 5050, sum(k, k, 1, 1000000) -> 500000500000 (closed form first)
  prod(f, k, a, b)   product: prod(k, k, 1, 10) -> 3628800
  -- Keyboard & input --
  Tab                complete functions/commands/constants/variables; a function becomes name() with the cursor inside
  Ctrl+C             during a computation: interrupt it; when idle: quit
  Ctrl+L             clear screen   Ctrl+R  reverse history search
  Brackets           the brackets at the cursor and their match are bolded; invalid ones turn red (Enter is never blocked)
  IME                full-width ( ) , . etc. are converted to half-width automatically

[Variables]
  /let A = 5         store a variable (UPPERCASE name), usable in expressions, equations and fac
  /var               list all; /del A deletes one; /del all clears everything
  Variables, modes and colors live in .hipercalc_state under your home directory; delete it to reset

[Modes]
  /mode mathio|lineio   symbolic-first display, decimal-only display
  /mode deg|rad         degree / radian
  /mode fast|deep       Fast (size guards), Deep (brute force, full precision); /mode shows current
  /lang zh-CN|zh-TW|en  interface language, or just type /lang to pick from a menu

[Commands]
  /help /clear /mode /lang /timing /set /let /var /del /reset /save /load /exit /quit

[Colors]
  /set <category> <color>   type /set alone to see usage and the current colors
  Categories: functions operators commands brackets
              constants numbers prompt result error

";

/// 按当前语言返回帮助文本
fn help_text() -> &'static str {
    match i18n::get() {
        i18n::Lang::ZhCn => HELP_TEXT,
        i18n::Lang::ZhTw => HELP_TEXT_TW,
        i18n::Lang::En => HELP_TEXT_EN,
    }
}

const HELP_TEXT: &str = "\
HiPerCalc 超高精度命令行计算器（输入表达式直接计算；/exit 退出，Ctrl+C 清行、连按两次退出）

【基本运算】
  + - * / ^ !        加 减 乘 除、幂（右结合）、阶乘（优先级高于幂）
  ( ) | |            圆括号、绝对值（2|x|、|x-3|）
  ans                上一次结果；隐式乘法 3x、2(x+1)；科学计数法 1e3、2.5e-2

【函数】
  sqr sqrt abs       开方、绝对值
  ln exp             自然对数、自然指数
  log(b,x) log10 log2                任意底、常用、以 2 为底对数
  floor ceil round frac sign         取整、小数部分、符号
  sin cos tan cot sec csc            三角函数（受角度模式影响）
  arcsin arccos arctan arccot arcsec arccsc
  sinh cosh tanh     双曲函数；特殊角与反三角特殊值返回精确值
  gcd(a,b) lcm(a,b)  最大公因数 / 最小公倍数（须为非零整数，结果恒非负）

【常数与精度】
  pi π e             内部 80 位小数，显示 20 位有效数字

【方程 / 方程组】
  x^2-4=0            单方程：多项式精确/数值求全部根（含复数），三角方程给 k·π 通式
  x+y=5, 2x-y=1      逗号分隔方程组（线性高斯消元、非线性多维牛顿）

【因式分解 / 素因数分解 / 显示转换 / 三角形】（须为最外层函数）
  fac(x^2-4)         因式分解（factor 等价）；整数入参做素因数分解；MathIO 实数域、LineIO 有理数域
  primefac(12)       素因数分解：12 = 2^2 * 3（负数写成 -12 = -2^2 * 3）
  sd(1/3)            显示转换：MathIO 转小数、LineIO 转符号
  triangle(a=3 b=4 c=5)  三角形求解：边/角/高 → 全部量（可逗号分隔；只能最外层）

【高等数学】（可参与运算：diff(x^2,x)+1 合法；精确值给 =，数值回退给 ≈）
  diff(f, x)         求导：diff(x^2, x) → 2*x、diff(sin(x), x) → cos(x)（变量须为单个字母）
  int(f, x)          不定积分：int(x^2, x) → 1 / 3 * x^3（省略积分常数）
  int(f, x, a, b)    定积分：int(x^2, x, 0, 1) → 1 / 3；无初等原函数时自动数值积分（上限可写 inf）
  lim(f, x, a)       极限：lim(sin(x)/x, x, 0) → 1、lim(1/x, x, inf) → 0（a 可为 inf）
  taylor(f, x, a, n) 泰勒展开（a=0 即麦克劳林）：taylor(sin(x), x, 0, 5) → x - 1 / 6 * x^3 + 1 / 120 * x^5（n 是最高次数，不含余项）
  sum(f, k, a, b)    求和：sum(k, k, 1, 100) → 5050、sum(k, k, 1, 1000000) → 500000500000（先闭式后逐项）
  prod(f, k, a, b)   求积：prod(k, k, 1, 10) → 3628800
  ── 键盘与输入 ──
  Tab                补全函数/指令/常数/变量；函数补成 name() 并把光标放进括号
  Ctrl+C             计算中：中断当前运算；空闲：退出程序
  Ctrl+L             清屏          Ctrl+R  反向搜索历史
  括号               光标处的括号与其配对括号加粗；不合法的括号标红（不挡回车）
  输入法             全角（）、。，等自动转成半角

【变量存储】
  /let A = 5         存储变量（全大写名），在表达式、方程、fac 中自动取值
  /var               查看全部；/del A 删除单个；/del all 清空
  变量、模式、颜色保存在用户主目录的 .hipercalc_state，删除即恢复默认

【模式】
  /mode mathio|lineio   数学显示（符号优先）、线性显示（一律小数）
  /mode deg|rad         角度制、弧度制
  /mode fast|deep       快速（规模保护）、死算（完整精度）；/mode 查看当前
  /lang zh-CN|zh-TW|en  界面语言（简/繁/英），也可只输入 /lang 进入选择菜单

【指令】
  /help /clear /mode /lang /timing /set /let /var /del /reset /save /load /exit /quit

【颜色设置】
  /set <类别> <颜色>   仅输入 /set 查看用法与当前的各类颜色
  类别: functions(函数) operators(运算符) commands(指令) brackets(括号)
        constants(常量) numbers(数字) prompt(提示符) result(结果) error(错误)
";

/// 打印帮助：无边框、按类别着色，末尾附当前模式
fn print_help(state: &AppState) {
    for line in help_text().lines() {
        if line.starts_with('【') {
            lprint!("{}", line.color(state.colors.result).bold());
        } else {
            lprint!("{}", colorize_text(line, &state.colors, true));
        }
    }
    lprint!(
        "{}",
        format!("当前模式: {}", mode_info(state)).color(state.colors.prompt)
    );
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_args_default_is_interactive() {
        let o = parse_args(&args(&[])).unwrap();
        assert_eq!(o.cli, Cli::Interactive);
        assert_eq!(o.lang, None);
        assert!(!o.no_timing);
    }

    #[test]
    fn parse_args_eval_and_flags() {
        let o = parse_args(&args(&["-e", "1+2*3"])).unwrap();
        assert_eq!(o.cli, Cli::Eval("1+2*3".to_string()));
        let o = parse_args(&args(&["--eval", "x^2-4=0", "-q"])).unwrap();
        assert_eq!(o.cli, Cli::Eval("x^2-4=0".to_string()));
        assert!(o.no_timing);
        let o = parse_args(&args(&["--lang", "en", "-e", "1"])).unwrap();
        assert_eq!(o.lang, Some(i18n::Lang::En));
        let o = parse_args(&args(&["--lang", "zh-TW"])).unwrap();
        assert_eq!(o.lang, Some(i18n::Lang::ZhTw));
        let o = parse_args(&args(&["--stdin"])).unwrap();
        assert_eq!(o.cli, Cli::Stdin);
        let o = parse_args(&args(&["-f", "s.hc"])).unwrap();
        assert_eq!(o.cli, Cli::Script(std::path::PathBuf::from("s.hc")));
    }

    #[test]
    fn cli_q_does_not_leak_into_saved_timing() {
        // 配置里 timing=on，用户本次运行带了命令行 -q
        let st = AppState {
            evaluator: Evaluator::new(),
            colors: ColorConfig::default(),
            lang_base: i18n::Lang::ZhCn,
            timing_base: true,
        };
        calc_mode::set_timing(false); // -q 的运行期效果
        assert!(!calc_mode::timing_enabled(), "-q 本会话应确实关闭耗时行");
        // 旧实现直接落盘 `timing_enabled()` ⇒ false 被写进配置，之后每次启动都不显示耗时
        assert!(
            timing_for_persist(&st),
            "-q 只作用于本次会话，不得改写配置里的 timing"
        );
        // 只有用户显式 /timing off 才会更新落盘值
        let mut off = st;
        off.timing_base = false;
        assert!(!timing_for_persist(&off));
        calc_mode::set_timing(true); // 复原全局开关，避免影响其它测试
    }

    /// 构造一个只用于解方程的会话状态
    fn eq_state(angle: trig::AngleMode) -> AppState {
        let mut st = AppState {
            evaluator: Evaluator::new(),
            colors: ColorConfig::default(),
            lang_base: i18n::Lang::ZhCn,
            timing_base: true,
        };
        st.evaluator.angle_mode = angle;
        st
    }

    /// 解一条方程并返回输出（断言不报错）
    fn solve_eq(input: &str, angle: trig::AngleMode) -> String {
        let mut st = eq_state(angle);
        let (out, is_err) = run_line(input, &mut st);
        assert!(!is_err, "{input} 解算报错: {out}");
        out
    }

    #[test]
    fn degree_trig_equations_give_periodic_formulas() {
        // 度模式下三角方程必须给出周期通式（此前只给单个解）
        for (input, want) in [
            ("sin(x)=0", "x = k·180，k 为整数"),
            ("cos(x)=0", "x = 90 + k·180，k 为整数"),
            ("tan(x)=1", "x = 45 + k·180，k 为整数"),
            ("sin(x)=0.5", "x = 30 + k·360  或  150 + k·360，k 为整数"),
            ("2*sin(x)-1=0", "x = 30 + k·360  或  150 + k·360，k 为整数"),
            ("cos(x)=0.5", "x = 60 + k·360  或  300 + k·360，k 为整数"),
            ("sin(x)+cos(x)=0", "x = 135 + k·180，k 为整数"),
            ("sin(x)+cos(x)=1", "x = k·360  或  90 + k·360，k 为整数"),
        ] {
            let out = solve_eq(input, trig::AngleMode::Degree);
            assert!(out.contains(want), "{input} → {out}；期望含 `{want}`");
        }
    }

    #[test]
    fn radian_trig_equations_zero_regression() {
        for (input, want) in [
            ("sin(x)=0", "x = k·π，k 为整数"),
            ("cos(x)=0", "x = π/2 + k·π，k 为整数"),
            ("tan(x)=1", "x = π/4 + k·π，k 为整数"),
            ("sin(x)=0.5", "x = π/6 + k·2π  或  5π/6 + k·2π，k 为整数"),
            ("cos(x)=0.5", "x = π/3 + k·2π  或  5π/3 + k·2π，k 为整数"),
            ("sin(x)+cos(x)=0", "x = 3π/4 + k·π，k 为整数"),
            ("sin(x)+cos(x)=1", "x = k·2π  或  π/2 + k·2π，k 为整数"),
        ] {
            let out = solve_eq(input, trig::AngleMode::Radian);
            assert!(out.contains(want), "{input} → {out}；期望含 `{want}`");
        }
    }

    #[test]
    fn non_periodic_equations_are_left_alone() {
        // 不能把不呈周期结构的方程硬判成通式
        let out = solve_eq("sin(x)=x/10", trig::AngleMode::Radian);
        assert!(!out.contains("k·"), "sin(x)=x/10 不应给出通式: {out}");
        // 非三角方程不受角度模式影响
        for angle in [trig::AngleMode::Radian, trig::AngleMode::Degree] {
            assert!(solve_eq("1/x=2", angle).contains("0.5"));
            assert!(solve_eq("2^x=8", angle).contains("x = 3"));
            assert!(solve_eq("x^2-4=0", angle).contains("x = 2"));
            assert!(solve_eq("ln(x)=1", angle).contains("2.71828182845904"));
        }
    }

    #[test]
    fn primefac_and_gcd_lcm_end_to_end() {
        let mut st = eq_state(trig::AngleMode::Radian);
        for (input, want) in [
            // primefac：负数负号顶在最前、重复因数用指数
            ("primefac(12)", "12 = 2^2 * 3"),
            ("primefac(-12)", "-12 = -2^2 * 3"),
            ("primefac(1)", "1 = 1"),
            ("primefac(-1)", "-1 = -1"),
            ("primefac(7)", "7 = 7"),
            ("primefac(360)", "360 = 2^3 * 3^2 * 5"),
            ("primefac(2+3)", "5 = 5"), // 括号内是可以求值的表达式
            // fac / factor 的整数入参走同一条路
            ("fac(12)", "12 = 2^2 * 3"),
            ("factor(-12)", "-12 = -2^2 * 3"),
            // fac 的多项式路径**不受影响**
            ("fac(x^2-4)", "= (x - 2) * (x + 2)"),
            ("fac(0)", "= 0"),
            ("fac(2.5)", "= 2.5"),
            // gcd / lcm：不要求最外层，结果恒非负
            ("gcd(12,18)", "= 6"),
            ("lcm(4,6)", "= 12"),
            ("gcd(-4,-6)", "= 2"),
            ("lcm(-4,-6)", "= 12"),
            ("2*gcd(12,18)", "= 12"),
            ("lcm(3,5)+1", "= 16"),
        ] {
            let (out, is_err) = run_line(input, &mut st);
            assert!(!is_err, "{input} 报错: {out}");
            assert!(out.contains(want), "{input} → {out}；期望含 `{want}`");
        }
        // 报错路径
        for bad in [
            "primefac(0)",
            "primefac(2.5)",
            "primefac(1/2)",
            "2*primefac(12)",
            "primefac(12)+1",
            "gcd(0,6)",
            "lcm(6,0)",
            "gcd(1.5,2)",
        ] {
            let (out, is_err) = run_line(bad, &mut st);
            assert!(is_err, "{bad} 应当报错，实得: {out}");
            assert!(out.contains("错误"), "{bad} → {out}");
        }
    }

    #[test]
    fn calculus_diff_end_to_end() {
        let mut st = eq_state(trig::AngleMode::Radian);
        for (input, want) in [
            ("diff(x^2,x)", "= 2*x"),
            ("diff(x^2,x)+1", "= 2*x + 1"), // 结果可参与运算
            ("2*diff(x^3,x)", "= 6*x^2"),
            ("diff(sin(x),x)", "= cos(x)"),
            ("diff(1/x,x)", "= -1 / x^2"),
            ("diff(x^x,x)", "= x^x * (ln(x) + 1)"),
            ("diff(sin(2*x),x)", "= 2*cos(2*x)"),
            ("diff(y,x)", "= 0"),
            ("diff(diff(x^3,x),x)", "= 6*x"),
        ] {
            let (out, is_err) = run_line(input, &mut st);
            assert!(!is_err, "{input} 报错: {out}");
            assert!(out.contains(want), "{input} → {out}；期望含 `{want}`");
        }
        // 求导结果能进方程：`diff(x^2,x)=2` ⇒ 2*x = 2 ⇒ x = 1
        let (out, is_err) = run_line("diff(x^2,x)=2", &mut st);
        assert!(!is_err, "diff 进方程报错: {out}");
        assert!(out.contains("x = 1"), "{out}");

        // 报错路径
        for bad in ["diff(x^2)", "diff(x^2,x+1)", "diff(floor(x),x)"] {
            let (out, is_err) = run_line(bad, &mut st);
            assert!(is_err, "{bad} 应当报错，实得: {out}");
            assert!(out.contains("错误"), "{bad} → {out}");
        }
        // 职责边界：fac/sd 内部用高数函数要给"不能出现在此处"，而不是别的原因
        for bad in ["fac(diff(x^3,x))", "sd(diff(x^2,x))"] {
            let (out, is_err) = run_line(bad, &mut st);
            assert!(is_err, "{bad} 应当报错，实得: {out}");
            assert!(out.contains("不能出现在此处"), "{bad} → {out}");
        }
        // 零行为变化：不含高数函数的输入照旧
        let (out, is_err) = run_line("x+1", &mut st);
        assert!(is_err && out.contains("未定义变量"), "x+1 → {out}");
        let (out, is_err) = run_line("1+2*3", &mut st);
        assert!(!is_err && out.contains("= 7"), "1+2*3 → {out}");
    }

    #[test]
    fn calculus_integrate_end_to_end() {
        let mut st = eq_state(trig::AngleMode::Radian);
        st.evaluator.display_mode = parser::DisplayMode::MathIO;
        for (input, want) in [
            ("int(x^2,x)", "1 / 3 * x^3"),
            ("int(1/x,x)", "ln(abs(x))"),
            ("int(sin(x),x)", "-cos(x)"),
            ("int(x^2,x,0,1)", "1 / 3"),
            ("int(x,x,0,2)", "2"),
            ("int(1/(1+x^2),x)", "arctan(x)"),
        ] {
            let (out, is_err) = run_line(input, &mut st);
            assert!(!is_err, "{input} 报错: {out}");
            assert!(out.contains(want), "{input} → {out}；期望含 `{want}`");
        }
        // 报错路径
        for (bad, want) in [
            ("int(tan(x),x)", "未覆盖"),
            ("int(exp(-x^2),x)", "初等原函数"),
            ("int(x^2)", "需要"),
            ("int(x^2,x,x,1)", "常数"),
        ] {
            let (out, is_err) = run_line(bad, &mut st);
            assert!(is_err, "{bad} 应当报错，实得: {out}");
            assert!(out.contains(want), "{bad} → {out}；期望含 `{want}`");
        }
        // 无穷限：原函数在 ∞ 处收敛到 0（低于显示精度 ⇒ 按精确 0 处理），
        // 所以 ∫₀^∞ e^{-x}dx 的值就是 1（前缀可能是 ≈：另一端的 exp(0) 本身是近似值）
        st.evaluator.display_mode = parser::DisplayMode::MathIO;
        let (out, _) = run_line("int(exp(-x),x,0,inf)", &mut st);
        assert!(out.trim_end().ends_with('1'), "{out}");
        assert!(!out.contains("0.9999"), "不应出现 0.999… 的截断: {out}");
    }

    #[test]
    fn calculus_limit_end_to_end() {
        let mut st = eq_state(trig::AngleMode::Radian);
        st.evaluator.display_mode = parser::DisplayMode::MathIO;
        for (input, want) in [
            ("lim(x^2,x,2)", "4"),
            ("lim((x^2-1)/(x-1),x,1)", "2"),
            ("lim(sin(x)/x,x,0)", "1"),
            ("lim((exp(x)-1)/x,x,0)", "1"),
            ("lim(1/x,x,inf)", "0"),
            ("lim((2*x^2+1)/(x^2-3),x,inf)", "2"),
            ("lim((x+1)/(x^2+1),x,inf)", "0"),
            ("lim(x^3/(x^2+1),x,inf)", "inf"),
        ] {
            let (out, is_err) = run_line(input, &mut st);
            assert!(!is_err, "{input} 报错: {out}");
            assert!(out.contains(want), "{input} → {out}；期望含 `{want}`");
        }
        for (bad, want) in [
            ("lim(sin(x),x,inf)", "无法判定"),
            ("lim(abs(x)/x,x,0)", "不相等"),
            ("lim(floor(x),x,0)", "不相等"),
            ("lim(x^2,x,y)", "常数"),
            ("lim(x^2,x)", "需要"),
        ] {
            let (out, is_err) = run_line(bad, &mut st);
            assert!(is_err, "{bad} 应当报错，实得: {out}");
            assert!(out.contains(want), "{bad} → {out}；期望含 `{want}`");
        }
        // inf 只允许出现在极限点上
        let (out, is_err) = run_line("lim(x^2,inf,1)", &mut st);
        assert!(is_err && out.contains("无穷"), "{out}");
    }

    #[test]
    fn calculus_taylor_end_to_end() {
        let mut st = eq_state(trig::AngleMode::Radian);
        st.evaluator.display_mode = parser::DisplayMode::MathIO;
        for (input, want) in [
            ("taylor(sin(x),x,0,5)", "x - 1 / 6 * x^3 + 1 / 120 * x^5"),
            ("taylor(cos(x),x,0,4)", "1 - 1 / 2 * x^2 + 1 / 24 * x^4"),
            ("taylor(exp(x),x,0,3)", "1 + x + 1 / 2 * x^2 + 1 / 6 * x^3"),
            ("taylor(1/(1-x),x,0,3)", "1 + x + x^2 + x^3"),
            ("taylor(x^2,x,1,2)", "1 + 2 * (x - 1) + (x - 1)^2"),
            // 结果可参与运算（一旦与别的运算组合，就交给通用化简 ⇒ 按次数降序）
            ("taylor(sin(x),x,0,3)+x", "-1 / 6 * x^3 + 2*x"),
        ] {
            let (out, is_err) = run_line(input, &mut st);
            assert!(!is_err, "{input} 报错: {out}");
            assert!(out.contains(want), "{input} → {out}；期望含 `{want}`");
        }
        for (bad, want) in [
            ("taylor(sin(x),x,0,100)", "上限"),
            ("taylor(1/x,x,0,3)", "奇点"),
            ("taylor(x^2,x,y,2)", "常数"),
            ("taylor(sin(x),x,0,1.5)", "非负整数"),
            ("taylor(sin(x),x,0)", "需要"),
        ] {
            let (out, is_err) = run_line(bad, &mut st);
            assert!(is_err, "{bad} 应当报错，实得: {out}");
            assert!(out.contains(want), "{bad} → {out}；期望含 `{want}`");
        }
    }

    #[test]
    fn calculus_sumprod_end_to_end() {
        let mut st = eq_state(trig::AngleMode::Radian);
        st.evaluator.display_mode = parser::DisplayMode::MathIO;
        for (input, want) in [
            ("sum(k,k,1,100)", "5050"),
            ("sum(k^2,k,1,10)", "385"),
            ("sum(k,k,1,1000000)", "500000500000"), // 闭式
            ("sum(2^k,k,0,10)", "2047"),
            ("sum(1/k,k,1,3)", "11 / 6"),
            ("sum(1,k,1,10)", "10"),
            ("prod(k,k,1,10)", "3628800"),
            ("prod(2,k,1,10)", "1024"),
            ("prod(k,k,3,1)", "1"), // 空积
            ("sum(k,k,1,10)+1", "56"), // 可参与运算
        ] {
            let (out, is_err) = run_line(input, &mut st);
            assert!(!is_err, "{input} 报错: {out}");
            assert!(out.contains(want), "{input} → {out}；期望含 `{want}`");
        }
        for (bad, want) in [
            ("sum(sin(k),k,1,100000)", "无闭式"),
            ("sum(k^2,k,1)", "需要"),
            ("sum(k^2,k,1.5,3)", "常数整数"),
            ("sum(k^2,k+1,1,3)", "单个变量"),
        ] {
            let (out, is_err) = run_line(bad, &mut st);
            assert!(is_err, "{bad} 应当报错，实得: {out}");
            assert!(out.contains(want), "{bad} → {out}；期望含 `{want}`");
        }
    }

    /// **性能扫描**：默认 `#[ignore]`（不进常规测试），需要时显式跑：
    /// `cargo test --release perf_scan -- --ignored --nocapture`
    ///
    /// 为什么不做成"每条一个进程"的脚本：本机（沙箱/Defender）启动**任何**进程都要 1.4~1.6 秒
    /// （`hostname.exe` 实测），进程级计时会被这个常数淹没。进程内计时才能看到真实算法耗时。
    #[test]
    #[ignore]
    fn perf_scan() {
        let mut st = eq_state(trig::AngleMode::Radian);
        let cases: &[&str] = &[
            // 基础与精度
            "1+2*3", "1/3+1/6", "phi", "e^2", "100!",
            "2^1000", "2^10000", "2^100000", "10000!",
            "sin(1)", "ln(2)", "log(2,3)", "atan(1)", "exp(10)",
            // 方程 / 方程组
            "x^2-4=0", "x^3-6*x^2+11*x-6=0", "x^4-5*x^2+4=0", "x^50-2*x+1=0",
            "2^x=8", "sin(x)=0.5", "x+y=5, 2*x-y=1", "x^2+y^2=25, y=x+1",
            // 因式分解 / 素因数分解 / 三角形 / 拟合
            "fac(x^4-1)", "fac(x^6-1)", "primefac(360)", "primefac(1000003*1000033)",
            "primefac(2^61-1)", "triangle(a=3 b=4 c=5)", "triangle(a=4 b=5 A=30)",
            "(1,1) (2,4) (3,9) (4,16)",
            // 高等数学
            "diff(x^100,x)", "diff(x^x,x)", "int(x^2,x,0,1)", "int(exp(-x^2),x,0,1)",
            "int(sin(x)/x,x,0,pi)", "lim(sin(x)/x,x,0)", "lim((x^2-1)/(x-1),x,1)",
            "taylor(sin(x),x,0,10)", "taylor(1/(1-x),x,0,30)",
            "sum(k^2,k,1,100)", "sum(k,k,1,1000000000)", "prod(k,k,1,500)",
            "sum(1/k,k,1,200)",
        ];
        let mut rows: Vec<(u128, &str)> = Vec::new();
        for c in cases {
            let t = std::time::Instant::now();
            let _ = run_line(c, &mut st);
            rows.push((t.elapsed().as_micros(), c));
        }
        rows.sort_by(|a, b| b.0.cmp(&a.0));
        println!("=== 最慢 12 条（微秒）===");
        for (us, c) in rows.iter().take(12) {
            println!("{:10} us  {}", us, c);
        }
        let total: u128 = rows.iter().map(|r| r.0).sum();
        println!("--- 合计 {} us / {} 条 ---", total, rows.len());
    }

    /// 回归：初值表要覆盖**非三角方程**的近零根与远根
    /// （性能扫描时发现 `ln(x)=-10` 的真根 4.5e-5 在 0 与 0.5 之间没有任何初值 ⇒ 丢根）
    #[test]
    fn nonpolynomial_roots_near_zero_and_far() {
        let mut st = eq_state(trig::AngleMode::Radian);
        for (input, want) in [
            ("ln(x)=-10", "0.000045399929762484851536"),
            ("ln(x)=-5", "0.0067379469990854670966"),
            ("ln(x)=10", "22026.465794806716517"),
            ("2^x=8", "x = 3"),
            ("exp(x)=0.001", "-6.9077552789821370521"),
        ] {
            let (out, is_err) = run_line(input, &mut st);
            assert!(!is_err, "{input} 报错: {out}");
            assert!(out.contains(want), "{input} → {out}；期望含 `{want}`");
        }
    }

    /// 回归：**没有闭式、又超出精确预算**的求和应当**数值累加**出结果（而不是直接拒绝）。
    ///
    /// 取 3000 项是有讲究的：它同时超过 Fast（200）与 **Deep（2000）**的分数项精确预算，
    /// 所以**无论当前是哪个模式**都会走数值路径 —— `/mode` 是进程级全局，测试并行时
    /// 不能假设自己处于 Fast（第一版就因为这点误判了）。
    #[test]
    fn large_sums_fall_back_to_numeric() {
        let mut st = eq_state(trig::AngleMode::Radian);
        let (out, is_err) = run_line("sum(1/k^3,k,1,3000)", &mut st);
        assert!(!is_err, "{out}");
        assert!(out.contains("≈"), "数值结果应标 ≈（精确值不会带 ≈）: {out}");
        // Σ_{k=1}^{3000} 1/k^3 = ζ(3) − 约 5.6e-8 ⇒ 前缀 1.2020568 足够抓住算错的情况
        assert!(out.contains("1.2020568"), "数值累加结果不对: {out}");
    }

    #[test]
    fn bracket_pair_matching() {
        assert_eq!(match_bracket_pair("(1+2)", 0), Some((0, 4)));
        assert_eq!(match_bracket_pair("(1+2)", 5), Some((4, 0))); // 光标在右括号之后 ⇒ 取左邻
        assert_eq!(match_bracket_pair("sin(x)", 3), Some((3, 5)));
        assert_eq!(match_bracket_pair("((1))", 2), Some((1, 3)));
        assert_eq!(match_bracket_pair("(", 0), None); // 无配对
        assert_eq!(match_bracket_pair("1+2", 2), None); // 没有括号
    }

    #[test]
    fn unmatched_bracket_marking() {
        assert_eq!(unmatched_brackets("1+2)", 4), vec![3]); // 多余 ')' 一律标
        assert!(unmatched_brackets("sin(1+", 6).is_empty()); // 光标在最右 ⇒ 正在输入，不标
        assert_eq!(unmatched_brackets("sin(1+", 0), vec![3]); // 光标移开 ⇒ 标
        assert!(unmatched_brackets("(1+2", 5).is_empty());
    }

    #[test]
    fn enclosing_function_detection() {
        assert_eq!(enclosing_function_call("sin(1,2", 7), Some(("sin".into(), 3, 2)));
        assert_eq!(enclosing_function_call("sin(", 4), Some(("sin".into(), 3, 0)));
        assert_eq!(enclosing_function_call("1+log(2,", 8), Some(("log".into(), 5, 2)));
        assert_eq!(enclosing_function_call("(1+2)", 3), None); // 裸括号分组不算函数调用
        // `|…|` 内的逗号不计入参数分隔
        assert_eq!(enclosing_function_call("abs(|-3|,", 9), Some(("abs".into(), 3, 2)));
        // 嵌套：内层括号里的逗号属于内层函数
        assert_eq!(enclosing_function_call("diff(sin(x),x)", 12), Some(("diff".into(), 4, 2)));
    }

    /// **核心用例**：验证"补成 `sin()` 且光标落在括号内"的落点计算。
    /// （不能直接调 `Completer::update`：它要 `rustyline::Changeset`，而 `Changeset::new()`
    ///   是 `pub(crate)`；把落点逻辑抽成纯函数后即可直接断言。）
    #[test]
    fn completion_edit_puts_cursor_inside_parens() {
        let (range, pos) = completion_edit(0, 3, "sin()");
        assert_eq!(range, 0..3);
        assert_eq!(pos, 4, "光标应落在括号内：sin(|)");
        // 指令/常数/变量：原样补全，光标在末尾
        let (_, pos) = completion_edit(0, 2, "pi");
        assert_eq!(pos, 2);
        // 循环补全：第二次仍按"原区间"替换，不会越补越短
        let (range, pos) = completion_edit(0, 3, "sinh()");
        assert_eq!(range, 0..3);
        assert_eq!(pos, 5);
    }

    #[test]
    fn progressive_argument_hint() {
        // 每输入一个参数，对应的提示就消失
        assert_eq!(remaining_args_hint("(x)", 0), "x)");
        assert_eq!(remaining_args_hint("(x)", 1), ")");
        assert_eq!(remaining_args_hint("(base, x)", 0), "base, x)");
        assert_eq!(remaining_args_hint("(base, x)", 1), "x)");
        assert_eq!(remaining_args_hint("(f, x, a, b)", 2), "a, b)");
        assert_eq!(sig_params("(f, x, a, b)"), vec!["f", "x", "a", "b"]);
    }

    #[test]
    fn help_body_only_lights_up_real_commands() {
        // 帮助正文（command_anywhere=true）：只有真指令才算指令
        assert!(command_token_is_valid("/help", true));
        assert!(command_token_is_valid("/q", true));
        assert!(!command_token_is_valid("/y", true), "x/y/z 里的 /y 不该被点亮");
        assert!(!command_token_is_valid("/z", true));
        // 输入行（false）：宽松判据，正在敲的半个指令也保留指令色
        assert!(command_token_is_valid("/mo", false));
    }

    #[test]
    fn functions_meta_covers_parser_functions() {
        // 元数据表与函数白名单必须双向一致，否则新函数会没有参数提示/补全括号
        for f in parser::FUNCTIONS {
            assert!(fn_meta(f).is_some(), "{f} 缺 FUNCTIONS_META 元数据");
        }
        for m in FUNCTIONS_META {
            assert!(
                parser::FUNCTIONS.contains(&m.name),
                "{} 不在 parser::FUNCTIONS 里",
                m.name
            );
        }
    }

    #[test]
    fn fullwidth_map_is_sane() {
        // 无重复键、不能映射到自己、不含字母（会干扰变量名规则）
        let mut seen = std::collections::HashSet::new();
        for (full, half) in FULLWIDTH_MAP {
            assert!(seen.insert(*full), "全角字符重复: {full}");
            assert!(!half.chars().all(|c| c.is_alphabetic()), "{half} 不该映射到字母");
            assert!(!full.is_ascii(), "{full} 不是全角字符");
        }
        assert!(FULLWIDTH_MAP.iter().any(|(f, h)| *f == '。' && *h == "."));
        assert!(FULLWIDTH_MAP.iter().any(|(f, h)| *f == '（' && *h == "("));
    }

    #[test]
    fn calculus_prefix_follows_display_mode() {
        // MathIO：精确分数算精确 ⇒ `=`
        let mut st = eq_state(trig::AngleMode::Radian);
        st.evaluator.display_mode = parser::DisplayMode::MathIO;
        let (out, _) = run_line("diff(x^4/12,x)", &mut st);
        assert!(out.contains("= 1 / 3 * x^3"), "MathIO → {out}");

        // LineIO：同一个精确分数会显示成 20 位小数 ⇒ 只能算近似 ⇒ `≈`
        let mut st = eq_state(trig::AngleMode::Radian);
        st.evaluator.display_mode = parser::DisplayMode::LineIO;
        let (out, _) = run_line("diff(x^4/12,x)", &mut st);
        assert!(out.contains("≈ 0.33333333333333333333*x^3"), "LineIO → {out}");
    }

    #[test]
    fn parse_args_rejects_bad_input() {
        assert!(parse_args(&args(&["-e"])).is_err());
        assert!(parse_args(&args(&["--lang"])).is_err());
        assert!(parse_args(&args(&["--lang", "klingon"])).is_err());
        assert!(parse_args(&args(&["--nope"])).is_err());
        // 错误文案必须是可翻译模板（能在词条表里命中）：切到英文后应被翻译出来。
        // 语言是进程级全局，切换前先拿测试锁、切换后**还原原值**（写死还原成简体
        // 会和 i18n 的守卫测试打架，造成偶发假失败）
        let e = parse_args(&args(&["-e"])).unwrap_err();
        let guard = i18n::TEST_LANG_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let prev = i18n::get();
        i18n::set(i18n::Lang::En);
        let translated = i18n::t(&e);
        i18n::set(prev);
        drop(guard);
        assert_ne!(translated, e, "缺少取值提示未命中词条");
    }
    #[test]
    fn commands_table_is_consistent() {
        assert!(COMMANDS.len() >= 10);
        let mut seen = std::collections::HashSet::new();
        for c in COMMANDS {
            assert!(c.starts_with('/'), "{c} 应以 / 开头");
            assert!(seen.insert(*c), "{c} 重复登记");
        }
        // 关键指令必须在表里（Tab 补全依赖）
        for need in ["/mode", "/lang", "/timing", "/set", "/let", "/save", "/load", "/reset"] {
            assert!(COMMANDS.contains(&need), "{need} 未登记到 COMMANDS");
        }
    }

    #[test]
    fn completion_respects_implicit_multiplication() {
        let vars = vec!["PI_VAR".to_string(), "AB".to_string()];
        // 指令前缀
        assert_eq!(completion_candidates("/mo", &vars), vec!["/mode".to_string()]);
        // 多字母函数名
        let f = completion_candidates("sin", &vars);
        assert!(f.contains(&"sin".to_string()) && f.contains(&"sinh".to_string()), "{f:?}");
        // 常数
        assert!(completion_candidates("ta", &vars).contains(&"tau".to_string()));
        // 存储变量（全大写）
        assert_eq!(completion_candidates("PI", &vars), vec!["PI_VAR".to_string()]);
        // 单字母 token 不给候选（否则会把 x、y 这类隐式乘法因子补成函数名）
        for t in ["x", "y", "e", "2", "xy", "xz", "P"] {
            assert!(
                completion_candidates(t, &vars).is_empty(),
                "{t} 不应产生候选（破坏隐式乘法）"
            );
        }
        // 空 token 不弹全量候选
        assert!(completion_candidates("", &vars).is_empty());
    }

    #[test]
    fn inline_hints() {
        // 指令用法提示
        let h = inline_hint("/mode").unwrap();
        assert!(h.contains("mathio"), "{h}");
        // 未知指令/前缀输入不提示
        assert!(inline_hint("/mo").is_none());
        assert!(inline_hint("/nope").is_none());
        // 函数签名提示
        assert_eq!(inline_hint("log").unwrap(), "(base, x)");
        assert_eq!(inline_hint("nroot").unwrap(), "(x, n)");
        assert_eq!(inline_hint("1+mod").unwrap(), "(a, b)");
        assert!(inline_hint("log(").is_none());
        assert!(inline_hint("xyz").is_none());
        assert!(inline_hint("").is_none());
    }

}
