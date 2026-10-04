//! 界面语言（简体中文 / 繁體中文 / English）
//!
//! 设计要点：
//! - 语言是**进程级**开关（`AtomicU8`），与 `calc_mode` 同理——大量用户可见字符串
//!   产生在 `parser.rs` / `number.rs` / `solver_*.rs` 等拿不到 `Evaluator` 的地方；
//! - 文案表以**简体中文原文为键**（代码里的字面量保持不变），运行时按模式匹配整行输出：
//!   表项里用 `{0}`、`{1}` 标注插值位置，匹配时把插值部分抓出来，替换进目标语言的模板；
//! - 颜色转义（ANSI）与文案分开处理：先把一行按 ANSI 序列切成若干纯文本段，
//!   对每段做匹配翻译再拼回，因此 `"错误".red() + ": " + 消息` 这类拼接串也能正确翻译；
//! - 简体中文时直接原样返回（零开销、零改动风险）；
//! - 未命中的字符串原样保留（永不报错），方便逐步补充词条。

use std::sync::atomic::{AtomicU8, Ordering};

/// 界面语言
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    /// 简体中文
    ZhCn,
    /// 繁體中文
    ZhTw,
    /// English
    En,
}

impl Lang {
    /// 状态文件里的语言代码
    pub fn code(self) -> &'static str {
        match self {
            Lang::ZhCn => "zh-CN",
            Lang::ZhTw => "zh-TW",
            Lang::En => "en",
        }
    }

    /// 用当前语言书写的语言名（用于 /lang 菜单）
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::ZhCn => "简体中文",
            Lang::ZhTw => "繁體中文",
            Lang::En => "English",
        }
    }

    /// 语言菜单的三行（本题要求的固定顺序）
    pub const ALL: [Lang; 3] = [Lang::ZhCn, Lang::ZhTw, Lang::En];

    /// 解析语言代码 / 名称 / 序号（`/lang zh-tw`、`/lang 2`、`/lang English` 等）
    pub fn parse(s: &str) -> Option<Lang> {
        let t = s.trim().to_lowercase();
        let t = t.trim_start_matches('/').trim_start_matches("language").trim_start_matches("lang");
        match t.trim() {
            "zh-cn" | "zh_cn" | "zhcn" | "cn" | "zh" | "chinese" | "1" | "简体" | "简体中文" => {
                Some(Lang::ZhCn)
            }
            "zh-tw" | "zh_tw" | "zhtw" | "tw" | "zh-hk" | "hk" | "2" | "繁體" | "繁体"
            | "繁体中文" | "繁體中文" => Some(Lang::ZhTw),
            "en" | "en-us" | "english" | "3" => Some(Lang::En),
            _ => None,
        }
    }
}

/// 当前语言（默认简体中文；首次启动由 state 层按系统语言决定）
static LANG: AtomicU8 = AtomicU8::new(0);

/// 设置当前语言
pub fn set(lang: Lang) {
    LANG.store(
        match lang {
            Lang::ZhCn => 0,
            Lang::ZhTw => 1,
            Lang::En => 2,
        },
        Ordering::Relaxed,
    );
}

/// 测试专用：语言是**进程级全局**（`AtomicU8`），任何临时切换语言的测试都要先拿这把锁，
/// 否则两个测试的 `set/还原` 会交错，出现"翻译没生效"的偶发假失败。
#[cfg(test)]
pub static TEST_LANG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 读取当前语言
pub fn get() -> Lang {
    match LANG.load(Ordering::Relaxed) {
        1 => Lang::ZhTw,
        2 => Lang::En,
        _ => Lang::ZhCn,
    }
}

/// 检测系统语言：中文（简/繁）→ 对应中文，其余一律英文
pub fn detect_system() -> Lang {
    // 1) Windows：以**系统界面语言**为准（要求"系统不是中文就一律英文"；
    //    Git Bash 等环境常把 LANG 设成 en_US.UTF-8，用环境变量判断会误判成英文）
    #[cfg(windows)]
    {
        let lcid = unsafe { GetUserDefaultUILanguage() };
        // 低 10 位是主语言 ID：0x04 = 中文
        let primary = lcid & 0x3ff;
        if primary == 0x04 {
            // 0x0404/0x0c04/0x1404 = 台湾/香港/澳门 → 繁体
            return match lcid {
                0x0404 | 0x0c04 | 0x1404 => Lang::ZhTw,
                _ => Lang::ZhCn,
            };
        }
        return Lang::En;
    }
    // 2) 其它平台：退回环境变量（zh* → 中文，其余非空 → 英文）
    #[cfg(not(windows))]
    {
        for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(v) = std::env::var(key) {
                let v = v.to_lowercase();
                if v.starts_with("zh") {
                    if v.contains("tw") || v.contains("hk") || v.contains("mo") || v.contains("hant")
                    {
                        return Lang::ZhTw;
                    }
                    return Lang::ZhCn;
                }
                if !v.is_empty() {
                    return Lang::En;
                }
            }
        }
        Lang::En
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    /// 系统界面语言 LCID（仅用到这一个 API，直接声明避免引入 windows-sys 依赖）
    fn GetUserDefaultUILanguage() -> u16;
}

/// 翻译一段文本（可含 ANSI 颜色码）。简体中文直接返回原文。
///
/// 行内**不再**按 ANSI 切段：`match_in` 匹配字面量时会自己跳过颜色码，
/// 于是带占位符的整行词条可以跨颜色边界匹配（旧实现按段翻译，
/// `x = 90 + k·180，k 为整数` 只翻得出前半句）。
pub fn t(s: &str) -> String {
    if get() == Lang::ZhCn {
        return s.to_string();
    }
    translate_plain(s)
}

/// 翻译一段文本（可能跨多行）——按 `\n` 逐行翻译。
///
/// **必须逐行**：`translate_run` 的替换有轮数上限，而一整块多行输出里的可翻译片段动辄十几个
/// （多解三角形一解就有 `面积`/`周长`/`外接圆半径`/`内切圆半径` 四个，两解就八个），
/// 会被上限截断，表现为"前半行翻好了、后半行还留着中文"（实测 `周长` 漏译）。
/// 按行拆开之后每行片段数远小于上限，且翻译本来就是面向整行的。
/// 行内不再按 ANSI 切段（见 `find_lit`）。
fn translate_plain(seg: &str) -> String {
    if !seg.contains('\n') {
        return translate_run(seg, 1);
    }
    let mut out = String::with_capacity(seg.len() + 16);
    for (i, line) in seg.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&translate_run(line, 1));
    }
    out
}

/// 构造期翻译：按 `{0}`/`{1}` 模板查表取译文并填充参数。
///
/// 与 `t()`（输出边界按纯文本段匹配）不同，这里在**构造阶段**就把模板换成目标语言，
/// 用于插值内容自带 ANSI 颜色、因而会被 `t()` 切段导致整行匹配不上的消息
/// （例如启动横幅 `输入 {0} 查看帮助，…` 里的 `/help` 是彩色的）。
/// 模板必须与词条表里的简体原文逐字一致（含占位符编号）；参数会先各自过一遍 `t()`。
pub fn fmt(template: &str, args: &[&str]) -> String {
    let lang = get();
    let target = TABLE
        .iter()
        .find(|row| row.0 == template)
        .map(|row| pick(row, lang))
        .unwrap_or(template);
    let values: Vec<String> = args.iter().map(|a| t(a)).collect();
    fill(target, &values)
}

/// 把 `{n}` 占位符替换为 values[n]（越界或非数字占位符原样保留）
fn fill(template: &str, values: &[String]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '{' {
            out.push(c);
            continue;
        }
        let mut num = String::new();
        while let Some(&n) = chars.peek() {
            if n.is_ascii_digit() {
                num.push(n);
                chars.next();
            } else {
                break;
            }
        }
        if !num.is_empty() && chars.peek() == Some(&'}') {
            chars.next();
            if let Ok(k) = num.parse::<usize>() {
                if let Some(v) = values.get(k) {
                    out.push_str(v);
                    continue;
                }
            }
        }
        out.push('{');
        out.push_str(&num);
    }
    out
}

/// 取某一行的目标语言文本
fn pick(row: &(&'static str, &'static str, &'static str), lang: Lang) -> &'static str {
    match lang {
        Lang::ZhCn => row.0,
        Lang::ZhTw => row.1,
        Lang::En => row.2,
    }
}

/// 在一个纯文本段内做模式匹配翻译（depth 用于限制递归，避免相互展开）。
/// 一段里可能有多处可翻译片段（例如 `"错误: 除以零错误"`：标签与消息各是一条词条），
/// 因此反复取"最优匹配"替换，直到没有可替换项（上限 32 轮，防止词条互相包含时死循环）。
///
/// 注意调用方应先按 `\n` 拆行（见 `translate_plain`）：本函数的轮数上限是按**单行**估的。
fn translate_run(run: &str, depth: usize) -> String {
    if run.is_empty() || depth == 0 || get() == Lang::ZhCn {
        return run.to_string();
    }
    let lang = get();
    let mut cur = run.to_string();
    for _ in 0..32 {
        match replace_best(&cur, lang, depth) {
            Some(next) => cur = next,
            None => break,
        }
    }
    cur
}

/// 在 `run` 中找最优词条并替换一处；返回 None 表示没有可替换片段。
/// "最优"= 字面部分总长度最长者优先（更具体的词条优先于泛化词条）；
/// **长度相同则靠左者优先**。
///
/// 那条平局规则不是可有可无的：`未知类别: bogus` 会同时命中 `未知类别` 与 `类别: {0}`，
/// 两者字面长度都是 4，若只按"严格更长"比较、先遍历到谁就选谁，`类别: {0}` 会抢先匹配，
/// 结果把 `未知` 剩在匹配区之外 ⇒ 翻出 `未知Categories: bogus`。
/// 靠左优先则让整词条 `未知类别` 胜出 ⇒ `unknown category: bogus`。
fn replace_best(run: &str, lang: Lang, depth: usize) -> Option<String> {
    let mut best: Option<(usize, usize, usize, Vec<(usize, usize, String)>, usize)> = None;
    for (idx, row) in TABLE.iter().enumerate() {
        let zh = row.0;
        let target = pick(row, lang);
        if target == zh {
            continue; // 该行无需翻译（如纯符号）
        }
        if let Some((start, end, values, literal_len)) = match_in(run, zh) {
            let better = match &best {
                None => true,
                Some((_, best_start, _, _, best_len)) => {
                    literal_len > *best_len || (literal_len == *best_len && start < *best_start)
                }
            };
            if better {
                best = Some((idx, start, end, values, literal_len));
            }
        }
    }
    let (idx, start, end, values, _) = best?;
    let target = pick(&TABLE[idx], lang);
    let mut filled = String::with_capacity(target.len() + 16);
    let mut chars = target.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            let mut num = String::new();
            while let Some(&n) = chars.peek() {
                if n.is_ascii_digit() {
                    num.push(n);
                    chars.next();
                } else {
                    break;
                }
            }
            if !num.is_empty() && chars.peek() == Some(&'}') {
                chars.next();
                if let Ok(k) = num.parse::<usize>() {
                    if let Some((_, _, v)) = values.get(k) {
                        filled.push_str(&translate_run(v, depth - 1));
                        continue;
                    }
                }
                // 占位符越界（模板与运行时不一致）：原样保留
                filled.push('{');
                filled.push_str(&num);
                filled.push('}');
                continue;
            }
            filled.push('{');
            filled.push_str(&num);
            continue;
        }
        filled.push(c);
    }
    let mut out = String::with_capacity(run.len() + filled.len());
    out.push_str(&run[..start]);
    out.push_str(&filled);
    out.push_str(&run[end..]);
    Some(out)
}

/// ANSI 转义序列（`ESC [ … 字母`）的长度；`i` 处不是转义序列开头时返回 0
fn ansi_len(s: &str, i: usize) -> usize {
    let b = s.as_bytes();
    if i < b.len() && b[i] == 0x1b && i + 1 < b.len() && b[i + 1] == b'[' {
        let mut j = i + 2;
        while j < b.len() && !b[j].is_ascii_alphabetic() {
            j += 1;
        }
        if j < b.len() {
            j += 1;
        }
        return j - i;
    }
    0
}

/// 跳过 `i` 处连续的 ANSI 转义序列，返回其后的位置
fn skip_ansi(s: &str, mut i: usize) -> usize {
    loop {
        let n = ansi_len(s, i);
        if n == 0 {
            return i;
        }
        i += n;
    }
}

/// 去掉全部 ANSI 转义序列
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let n = ansi_len(s, i);
        if n > 0 {
            i += n;
            continue;
        }
        let c = s[i..].chars().next().unwrap();
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// 从 `at` 起匹配字面量 `lit`，**允许中间夹 ANSI 转义序列**。
///
/// 返回 `(正文起点, 结束字节位置)`：**起点刻意取第一个正文字符的位置**，把开头的颜色码
/// 留在匹配区间之外——否则替换会把整段开头的颜色码一并丢掉，症状是"翻译后这一行的颜色没了"
/// （实测：计时行的 `\x1b[2m` 被吞掉后 `Time: <1s` 从灰色变成白色、启动横幅标题同理）。
fn match_lit_at(hay: &str, at: usize, lit: &str) -> Option<(usize, usize)> {
    let text_start = skip_ansi(hay, at);
    let mut i = text_start;
    for c in lit.chars() {
        i = skip_ansi(hay, i);
        let ch = hay[i..].chars().next()?;
        if ch != c {
            return None;
        }
        i += ch.len_utf8();
    }
    Some((text_start, i))
}

/// 在 `hay` 中从 `from` 起查找字面量 `lit`（匹配时跳过 ANSI 转义序列），
/// 返回 `(正文起点, 结束字节位置)`，区间内可能夹着颜色码。
///
/// **这是结果行能整句翻译的关键**：结果按 token 着色后，颜色码会把一句话切成很多段，
/// 带占位符的词条（`{0} = {1}，k 为整数`、`解 {0}:`）若只按"段"匹配就永远匹配不上，
/// 症状是"英文模式下半句英文、半句中文"（实测 `x = 90 + k·180，k 为整数`）。
fn find_lit(hay: &str, from: usize, lit: &str) -> Option<(usize, usize)> {
    if lit.is_empty() {
        return Some((from, from));
    }
    let mut i = from;
    while i < hay.len() {
        if let Some(found) = match_lit_at(hay, i, lit) {
            return Some(found);
        }
        let n = ansi_len(hay, i);
        if n > 0 {
            i += n;
            continue;
        }
        i += hay[i..].chars().next()?.len_utf8();
    }
    None
}

/// 在 `run` 中匹配模板 `zh`（含 `{n}` 占位符）。
/// 返回 (匹配起止字节位置, 各占位符捕获的内容, 字面部分总长度)。
fn match_in(run: &str, zh: &str) -> Option<(usize, usize, Vec<(usize, usize, String)>, usize)> {
    // 切分模板：字面段与占位符交替
    let mut literals: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut chars = zh.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            let mut num = String::new();
            while let Some(&n) = chars.peek() {
                if n.is_ascii_digit() {
                    num.push(n);
                    chars.next();
                } else {
                    break;
                }
            }
            if !num.is_empty() && chars.peek() == Some(&'}') {
                chars.next();
                literals.push(std::mem::take(&mut cur));
                continue;
            }
            cur.push('{');
            cur.push_str(&num);
            continue;
        }
        cur.push(c);
    }
    literals.push(cur);
    let literal_len: usize = literals.iter().map(|l| l.chars().count()).sum();
    // 单字词条（颜色名"红/绿"等）只允许**整段精确匹配**，否则会在任意文本里到处替换；
    // 两字及以上允许行内匹配（例如"错误: "、"（有理数域内不可再分解）"这类拼接片段）
    if literal_len <= 1 {
        if strip_ansi(run) == zh {
            return Some((0, run.len(), Vec::new(), literal_len));
        }
        return None;
    }

    // 定位：逐段按顺序查找（匹配时跳过 ANSI，见 `find_lit`）
    let mut positions: Vec<(usize, usize)> = Vec::with_capacity(literals.len());
    let mut from = 0usize;
    for lit in literals.iter() {
        if lit.is_empty() {
            positions.push((from, from));
            continue;
        }
        let Some((pos, end)) = find_lit(run, from, lit) else {
            return None;
        };
        positions.push((pos, end));
        from = end;
    }
    // 首段必须是整体匹配的起点或出现在行内（允许前缀，如颜色码分割后的 ": "）
    let start = positions.first().map(|p| p.0).unwrap_or(0);
    let end = positions.last().map(|p| p.1).unwrap_or(start);
    if end < start {
        return None;
    }
    // 计算占位符捕获：字面段之间的空隙
    let mut values: Vec<(usize, usize, String)> = Vec::new();
    for i in 0..literals.len().saturating_sub(1) {
        let a = positions[i].1;
        let b = positions[i + 1].0;
        if b < a {
            return None;
        }
        values.push((a, b, run[a..b].to_string()));
    }
    Some((start, end, values, literal_len))
}

/// 文案表：(`简体中文原文`, `繁體中文`, `English`)。
/// 简体中文一列必须与代码里的字面量**逐字一致**（含 `{n}` 占位符），否则该条不生效。
#[rustfmt::skip]
static TABLE: &[(&str, &str, &str)] = &[
    // ---- 启动/横幅/计时 ----
    ("超高精度命令行计算器 (HiPerCalc) v1.1", "超高精度命令列計算器 (HiPerCalc) v1.1", "HiPerCalc — Ultra-Precision CLI Calculator v1.1"),
    ("当前模式: {0}", "目前模式: {0}", "Current modes: {0}"),
    ("输入 {0} 查看帮助，输入表达式进行计算，{1} 退出", "輸入 {0} 查看說明，輸入算式進行計算，{1} 離開", "Type {0} for help, enter an expression to compute, {1} to quit"),
    ("已恢复上次会话设置，含 {0} 个存储变量（/var 查看，/del all 清空）", "已還原上次工作階段設定，含 {0} 個儲存變數（/var 檢視，/del all 清除）", "Restored previous session: {0} stored variables (/var to list, /del all to clear)"),
    ("\r用时：{0}秒", "\r耗時：{0}秒", "\rTime: {0}s"),
    ("用时：{0}秒", "耗時：{0}秒", "Time: {0}s"),
    ("用时：<1秒", "耗時：<1秒", "Time: <1s"),
    ("读取错误", "讀取錯誤", "Read error"),
    ("创建 readline 编辑器失败", "建立 readline 編輯器失敗", "Failed to create the readline editor"),
    // ---- 模式 ----
    ("已切换到数学显示模式 (MathIO)", "已切換到數學顯示模式 (MathIO)", "Display mode: MathIO (symbolic)"),
    ("已切换到线性显示模式 (LineIO)", "已切換到線性顯示模式 (LineIO)", "Display mode: LineIO (decimal)"),
    ("已切换到角度模式 (Degree)", "已切換到角度模式 (Degree)", "Angle mode: Degree"),
    ("已切换到弧度模式 (Radian)", "已切換到弧度模式 (Radian)", "Angle mode: Radian"),
    ("已切换到快速模式 (Fast)：规模超限时直接返回提示，输出 20 位有效数字（大数用科学计数法）", "已切換到快速模式 (Fast)：規模超限時直接回報提示，輸出 20 位有效數字（大數用科學記號）", "Calc mode: Fast — oversized results are refused with a hint; 20 significant digits (scientific notation for huge numbers)"),
    ("已切换到死算模式 (Deep)：不设规模上限、完整精度输出（不走科学计数法）；极端输入可能长时间无响应或产生超长输出", "已切換到死算模式 (Deep)：不設規模上限、完整精度輸出（不用科學記號）；極端輸入可能長時間無回應或產生超長輸出", "Calc mode: Deep — no size limits, full-precision output (no scientific notation); extreme inputs may hang or print enormous output"),
    ("{0}: {1}。可选: {2}", "{0}: {1}。可選: {2}", "{0}: {1}. Options: {2}"),
    ("未知模式", "未知模式", "unknown mode"),
    ("数学显示", "數學顯示", "MathIO"),
    ("线性显示", "線性顯示", "LineIO"),
    ("角度制", "角度制", "Degree"),
    ("弧度制", "弧度制", "Radian"),
    ("快速", "快速", "Fast"),
    ("死算", "死算", "Deep"),
    ("未知指令", "未知指令", "unknown command"),
    ("{0}: {1}。输入 {2} 查看可用指令。", "{0}: {1}。輸入 {2} 查看可用指令。", "{0}: {1}. Type {2} to list available commands."),
    ("用法", "用法", "Usage"),
    ("提示", "提示", "Note"),
    ("错误", "錯誤", "Error"),
    // ---- /mode 用法 ----
    ("{0}: /mode <{1}|{2}|{3}|{4}|{5}|{6}>", "{0}: /mode <{1}|{2}|{3}|{4}|{5}|{6}>", "{0}: /mode <{1}|{2}|{3}|{4}|{5}|{6}>"),
    // ---- /set ----
    ("{0}: /set <类别> <颜色>", "{0}: /set <類別> <顏色>", "{0}: /set <category> <color>"),
    ("类别: {0}", "類別: {0}", "Categories: {0}"),
    ("  颜色: {0}", "  顏色: {0}", "  Colors: {0}"),
    ("示例: {0}", "範例: {0}", "Example: {0}"),
    ("可用颜色见 /set 的用法提示", "可用顏色見 /set 的用法提示", "Run /set to list available colors"),
    ("可用类别见 /set 的用法提示", "可用類別見 /set 的用法提示", "Run /set to list available categories"),
    ("当前颜色（按各自颜色显示）:", "目前顏色（依各自顏色顯示）:", "Current colors (shown in their own color):"),
    ("未知类别", "未知類別", "unknown category"),
    ("未知颜色", "未知顏色", "unknown color"),
    ("颜色: {0}", "顏色: {0}", "Colors: {0}"),
    ("颜色已设置为 {0}（已保存，重启后保持）", "顏色已設定為 {0}（已儲存，重啟後保留）", "Color set: {0} (saved, kept after restart)"),
    ("函数", "函數", "functions"),
    ("运算符", "運算子", "operators"),
    ("指令", "指令", "commands"),
    ("括号", "括號", "brackets"),
    ("常量", "常數", "constants"),
    ("数字", "數字", "numbers"),
    ("提示符", "提示字元", "prompt"),
    ("结果", "結果", "result"),
    ("黑", "黑", "black"),
    ("红", "紅", "red"),
    ("绿", "綠", "green"),
    ("黄", "黃", "yellow"),
    ("蓝", "藍", "blue"),
    ("洋红", "洋紅", "magenta"),
    ("青", "青", "cyan"),
    ("白", "白", "white"),
    ("亮黑", "亮黑", "bright_black"),
    ("亮红", "亮紅", "bright_red"),
    ("亮绿", "亮綠", "bright_green"),
    ("亮黄", "亮黃", "bright_yellow"),
    ("亮蓝", "亮藍", "bright_blue"),
    ("亮洋红", "亮洋紅", "bright_magenta"),
    ("亮青", "亮青", "bright_cyan"),
    ("亮白", "亮白", "bright_white"),
    // ---- 变量存储 ----
    ("{0}: /let <大写变量名> = <表达式>", "{0}: /let <大寫變數名> = <算式>", "{0}: /let <UPPERCASE_NAME> = <expression>"),
    ("{0}: 变量名必须为全大写字母（如 X、AB）", "{0}: 變數名必須為全大寫字母（如 X、AB）", "{0}: variable names must be uppercase letters (e.g. X, AB)"),
    ("{0}: 变量名必须为全大写字母（如 X、AB），或用 /del all 清空", "{0}: 變數名必須為全大寫字母（如 X、AB），或用 /del all 清除", "{0}: variable names must be uppercase letters (e.g. X, AB), or use /del all"),
    ("{0}: /let 只能存储数值表达式的值", "{0}: /let 只能儲存數值算式的值", "{0}: /let accepts numeric expressions only"),
    ("{0}: 变量 {1} 不存在", "{0}: 變數 {1} 不存在", "{0}: variable {1} does not exist"),
    ("{0}: 暂无存储变量", "{0}: 尚無儲存變數", "{0}: no stored variables"),
    ("暂无存储变量", "尚無儲存變數", "No stored variables"),
    ("已删除变量 {0}", "已刪除變數 {0}", "Deleted variable {0}"),
    ("已删除全部 {0} 个变量", "已刪除全部 {0} 個變數", "Deleted all {0} variables"),
    ("/del <大写变量名> 或 /del all", "/del <大寫變數名> 或 /del all", "/del <UPPERCASE_NAME> or /del all"),
    ("{0}: 会话状态保存失败（{1}），本次的设置与变量不会在重启后保留", "{0}: 工作階段狀態儲存失敗（{1}），本次的設定與變數不會在重啟後保留", "{0}: failed to save session state ({1}); settings and variables will not survive a restart"),
    // ---- 方程/方程组 ----
    ("{0}: 方程中没有变量", "{0}: 方程中沒有變數", "{0}: the equation has no variable"),
    ("错误: 方程中没有变量", "錯誤: 方程中沒有變數", "Error: the equation has no variable"),
    ("{0}: 方程包含多个变量（{1}），当前单一方程仅支持单变量；线性方程组请用逗号分隔输入", "{0}: 方程包含多個變數（{1}），目前單一方程僅支援單變數；線性方程組請用逗號分隔輸入", "{0}: the equation has multiple variables ({1}); a single equation supports one variable — use commas to enter a system"),
    ("{0}: 方程数量({1})与变量数量({2})不匹配", "{0}: 方程數量({1})與變數數量({2})不符", "{0}: {1} equations vs {2} variables — count mismatch"),
    ("{0}: 非线性方程组暂支持方程数 = 变量数（当前 {1}/{2})", "{0}: 非線性方程組暫支援方程數 = 變數數（目前 {1}/{2})", "{0}: the nonlinear solver needs #equations = #variables (now {1}/{2})"),
    ("{0}: 非线性方程组暂支持不超过 3 个变量", "{0}: 非線性方程組暫支援不超過 3 個變數", "{0}: the nonlinear solver supports at most 3 variables"),
    ("方程组无解", "方程組無解", "No solution"),
    ("方程组有无穷多解", "方程組有無窮多解", "Infinitely many solutions"),
    ("恒等式：对变量的任意取值均成立", "恆等式：對變數的任意取值均成立", "Identity: true for every value of the variable"),
    ("未能找到实数根", "未能找到實數根", "No real root found"),
    ("未找到实数解", "未找到實數解", "No real solution found"),
    ("无实数解", "無實數解", "No real solution"),
    ("无解", "無解", "No solution"),
    ("{0} 的一个解: {1} = {2}", "{0} 的一個解: {1} = {2}", "One solution of {0}: {1} = {2}"),
    // ---- 周期通式 / 结果后缀 ----
    // 周期通式只保留**尾段**词条，不写 `{0} = {1}，k 为整数` 那种整句模板：
    // 整句模板里的字面量 ` = ` 正好夹在"变量（结果底色）"与"数字（数字色）"之间，
    // 替换时该字面量区间内的颜色码会被丢掉，症状是英文模式下 `=` 从黄色变成白色。
    // 尾段 `，k 为整数` 本身是连续的一段纯文本，单独翻译既不丢颜色也用不到占位符。
    ("，k 为整数", "，k 為整數", ", k ∈ ℤ"),
    ("  或  ", "  或  ", " or "),
    ("（候选枚举超出规模上限，结果可能不完整）", "（候選枚舉超出規模上限，結果可能不完整）", " (candidate enumeration hit the size limit; result may be incomplete)"),
    ("（有理数域内不可再分解）", "（有理數域內不可再分解）", " (irreducible over the rationals)"),
    ("（剩余高次因式未作进一步精确分解）", "（剩餘高次因式未作進一步精確分解）", " (remaining high-degree factor not decomposed further)"),
    // ---- 求值/解析错误 ----
    ("除以零错误", "除以零錯誤", "division by zero"),
    ("开方参数必须为非负数", "開方參數必須為非負數", "the radicand must be non-negative"),
    ("负数的平方根在实数范围内无定义", "負數的平方根在實數範圍內無定義", "the square root of a negative number is undefined over the reals"),
    ("负数的非整数次幂在实数范围内无定义", "負數的非整數次幂在實數範圍內無定義", "a negative base with a non-integer exponent is undefined over the reals"),
    ("负数不能开平方", "負數不能開平方", "cannot take the square root of a negative number"),
    ("0 的负次幂未定义", "0 的負次幂未定義", "0 raised to a negative power is undefined"),
    ("ln 参数必须为正数", "ln 參數必須為正數", "ln requires a positive argument"),
    ("ln 的定义域为 x>0", "ln 的定義域為 x>0", "ln is defined for x>0 only"),
    ("log10 的定义域为 x>0", "log10 的定義域為 x>0", "log10 is defined for x>0 only"),
    ("log2 的定义域为 x>0", "log2 的定義域為 x>0", "log2 is defined for x>0 only"),
    ("log 的底数必须 > 0", "log 的底數必須 > 0", "the base of log must be > 0"),
    ("log 的真数必须 > 0", "log 的真數必須 > 0", "the argument of log must be > 0"),
    ("log 的底数不能为 1", "log 的底數不能為 1", "the base of log cannot be 1"),
    ("log 需要两个参数: log(底数, 真数)", "log 需要兩個參數: log(底數, 真數)", "log takes two arguments: log(base, x)"),
    ("arcsin 参数必须在 [-1, 1] 范围内", "arcsin 參數必須在 [-1, 1] 範圍內", "arcsin requires an argument in [-1, 1]"),
    ("arccos 参数必须在 [-1, 1] 范围内", "arccos 參數必須在 [-1, 1] 範圍內", "arccos requires an argument in [-1, 1]"),
    ("arcsec 参数必须满足 |x| >= 1", "arcsec 參數必須滿足 |x| >= 1", "arcsec requires |x| >= 1"),
    ("arccsc 参数必须满足 |x| >= 1", "arccsc 參數必須滿足 |x| >= 1", "arccsc requires |x| >= 1"),
    ("tan 在此角度未定义", "tan 在此角度未定義", "tan is undefined at this angle"),
    ("cot 在此角度未定义", "cot 在此角度未定義", "cot is undefined at this angle"),
    ("sec 在此角度未定义", "sec 在此角度未定義", "sec is undefined at this angle"),
    ("csc 在此角度未定义", "csc 在此角度未定義", "csc is undefined at this angle"),
    ("参数过大（约 10^{0} 弧度），{1} 位精度的 π 归约会失效，三角函数结果不可靠", "參數過大（約 10^{0} 弧度），{1} 位精度的 π 引數縮減會失效，三角函數結果不可靠", "argument too large (~10^{0} rad): π reduction at {1} digits breaks down, trigonometric results are unreliable"),
    ("参数过大（约 10^{0}），指数运算结果将超出支持范围", "參數過大（約 10^{0}），指數運算結果將超出支援範圍", "argument too large (~10^{0}): the exponential would exceed the supported range"),
    ("exp 参数过大（约 10^{0}），结果将超过 10^{1} 位十进制，超出支持范围", "exp 參數過大（約 10^{0}），結果將超過 10^{1} 位十進位，超出支援範圍", "exp argument too large (~10^{0}): the result would exceed 10^{1} decimal digits"),
    ("exp 参数过大", "exp 參數過大", "exp argument too large"),
    ("幂运算结果超出支持范围（指数 × ln(底数) ≈ 10^{0}，结果约 10^{1} 位十进制）", "幂運算結果超出支援範圍（指數 × ln(底數) ≈ 10^{0}，結果約 10^{1} 位十進位）", "power result out of range (exponent × ln(base) ≈ 10^{0}, about 10^{1} decimal digits)"),
    ("幂运算结果约有 {0} 位十进制，超过支持上限（约 10^{1} 位）", "幂運算結果約有 {0} 位十進位，超過支援上限（約 10^{1} 位）", "power result has about {0} decimal digits, exceeding the limit (10^{1})"),
    ("阶乘参数过大（上限 10000，/mode deep 可取消限制）", "階乘參數過大（上限 10000，/mode deep 可取消限制）", "factorial argument too large (limit 10000; /mode deep removes it)"),
    ("阶乘参数超出可表示范围", "階乘參數超出可表示範圍", "factorial argument out of representable range"),
    ("阶乘需要整数参数", "階乘需要整數參數", "factorial requires an integer argument"),
    ("阶乘需要非负整数", "階乘需要非負整數", "factorial requires a non-negative integer"),
    ("科学计数法指数超出支持范围（/mode deep 可取消限制）", "科學記號指數超出支援範圍（/mode deep 可取消限制）", "scientific-notation exponent out of range (/mode deep removes the limit)"),
    ("科学计数法指数超出可表示范围", "科學記號指數超出可表示範圍", "scientific-notation exponent out of representable range"),
    ("未知函数: {0}", "未知函數: {0}", "unknown function: {0}"),
    ("未知标识符: {0}", "未知識別字: {0}", "unknown identifier: {0}"),
    ("未定义变量: {0}", "未定義變數: {0}", "undefined variable: {0}"),
    ("函数 {0} 需要一个参数", "函數 {0} 需要一個參數", "function {0} takes one argument"),
    ("函数 {0} 缺少参数", "函數 {0} 缺少參數", "function {0} is missing an argument"),
    ("函数 '{0}' 缺少右括号", "函數 '{0}' 缺少右括號", "function '{0}' is missing a closing parenthesis"),
    ("位置 {0} 处意外的字符: '{1}'", "位置 {0} 處意外的字元: '{1}'", "unexpected character at position {0}: '{1}'"),
    ("位置 {0} 处多余的字符", "位置 {0} 處多餘的字元", "extra characters at position {0}"),
    ("缺少右括号 ')'", "缺少右括號 ')'", "missing closing parenthesis ')'"),
    ("缺少闭合的 '|'（绝对值）", "缺少閉合的 '|'（絕對值）", "missing closing '|' (absolute value)"),
    ("小数点后缺少数字", "小數點後缺少數字", "missing digits after the decimal point"),
    ("无法解析数字: {0}", "無法解析數字: {0}", "cannot parse number: {0}"),
    ("空表达式", "空算式", "empty expression"),
    ("fac/factor 函数需要参数: fac(表达式)", "fac/factor 函數需要參數: fac(算式)", "fac/factor needs an argument: fac(expression)"),
    ("fac/factor 必须是整个表达式的最外层函数", "fac/factor 必須是整個算式的最外層函數", "fac/factor must be the outermost function of the whole expression"),
    ("fac/factor 必须作为最外层函数使用", "fac/factor 必須作為最外層函數使用", "fac/factor must be used as the outermost function"),
    ("不是 fac/factor 表达式", "不是 fac/factor 算式", "not a fac/factor expression"),
    ("factor 目前仅支持 1~3 个变量", "factor 目前僅支援 1~3 個變數", "factor currently supports 1–3 variables"),
    ("factor 必须作为最外层函数使用", "factor 必須作為最外層函數使用", "factor must be used as the outermost function"),
    ("sd 函数需要参数: sd(表达式)", "sd 函數需要參數: sd(算式)", "sd needs an argument: sd(expression)"),
    ("sd 必须是整个表达式的最外层函数", "sd 必須是整個算式的最外層函數", "sd must be the outermost function of the whole expression"),
    ("sd 只能作为最外层函数使用", "sd 只能作為最外層函數使用", "sd can only be used as the outermost function"),
    ("不是 sd 表达式", "不是 sd 算式", "not an sd expression"),
    ("等式需要在求解模式下处理，不应直接计算", "等式需要在求解模式下處理，不應直接計算", "an equation must be solved, not evaluated"),
    ("等式不能在求值中使用", "等式不能在求值中使用", "an equation cannot be used inside an expression"),
    ("方程组不能在求值中使用", "方程組不能在求值中使用", "an equation system cannot be used inside an expression"),
    ("方程组中每一项都必须是等式", "方程組中每一項都必須是等式", "every item in a system must be an equation"),
    ("数值求根次数过高（{0} 次，Fast 模式上限 {1}）；如确认需要继续，请先执行 /mode deep", "數值求根次數過高（{0} 次，Fast 模式上限 {1}）；如確認需要繼續，請先執行 /mode deep", "numeric root-finding degree too high ({0}; Fast limit {1}); run /mode deep to continue"),
    ("数值求根次数过高", "數值求根次數過高", "numeric root-finding degree too high"),
    ("数值求根未收敛（{0} 次多项式的低精度粗收敛失败），请尝试低次方程", "數值求根未收斂（{0} 次多項式的低精度粗收斂失敗），請嘗試低次方程", "numeric root-finding did not converge ({0}-degree coarse stage failed); try a lower-degree equation"),
    ("数值求根未收敛（{0} 次多项式超出当前迭代策略），请尝试低次方程", "數值求根未收斂（{0} 次多項式超出目前迭代策略），請嘗試低次方程", "numeric root-finding did not converge ({0}-degree exceeds the current strategy); try a lower-degree equation"),
    ("高次求根需要精确的有理系数", "高次求根需要精確的有理係數", "high-degree root-finding needs exact rational coefficients"),
    ("无法因式分解：仅支持有理数系数的多项式", "無法因式分解：僅支援有理係數的多項式", "cannot factor: only polynomials with rational coefficients are supported"),
    ("未找到用户主目录", "未找到使用者家目錄", "home directory not found"),
    // ---- 复数 ----
    ("复数不支持该函数: {0}", "複數不支援該函數: {0}", "complex numbers are not supported by {0}"),
    ("复数的非整数次幂暂不支持", "複數的非整數次幂暫不支援", "non-integer powers of complex numbers are not supported yet"),
    ("复数的幂指数过大", "複數的幂指數過大", "complex exponent too large"),
    ("复数的对数要求 z != 0", "複數的對數要求 z != 0", "the logarithm of a complex number requires z != 0"),
    ("0 没有辐角", "0 沒有輻角", "0 has no argument"),
    // ---- 多项式拟合 ----
    ("多项式拟合至少需要 2 个坐标", "多項式擬合至少需要 2 個座標", "polynomial fitting needs at least 2 points"),
    ("坐标格式错误（应为 (x,y) 或 P(x,y)）", "座標格式錯誤（應為 (x,y) 或 P(x,y)）", "bad point format (expected (x,y) or P(x,y))"),
    ("顶点标记必须是大写 P（小写 p 是普通变量）", "頂點標記必須是大寫 P（小寫 p 是普通變數）", "the vertex marker must be an uppercase P (lowercase p is an ordinary variable)"),
    ("顶点坐标还需要至少一个普通坐标", "頂點座標還需要至少一個普通座標", "a vertex point needs at least one ordinary point as well"),
    ("模板缺少自变量 x", "模板缺少自變數 x", "the template has no independent variable x"),
    ("模板中没有未知参数", "模板中沒有未知參數", "the template has no unknown parameter"),
    ("模板不是多项式，无法提取升幂系数", "模板不是多項式，無法提取升冪係數", "the template is not a polynomial; cannot extract coefficients"),
    ("模板与坐标不一致（可能对参数不是线性的）", "模板與座標不一致（可能對參數不是線性的）", "the template does not match the points (it may be nonlinear in the parameters)"),
    ("自由参数: {0}", "自由參數: {0}", "free parameters: {0}"),
    ("坐标之间存在矛盾，无法同时满足", "座標之間存在矛盾，無法同時滿足", "the points are inconsistent — no polynomial fits all of them"),
    ("坐标重复或退化，无法唯一确定多项式", "座標重複或退化，無法唯一確定多項式", "duplicate or degenerate points — the polynomial is not unique"),
    ("坐标/参数过多（{0} 个，Fast 模式上限 {1}）；如确认需要继续，请先执行 /mode deep", "座標/參數過多（{0} 個，Fast 模式上限 {1}）；如確認需要繼續，請先執行 /mode deep", "too many points/parameters ({0}; Fast limit {1}); run /mode deep to continue"),
    // ---- 三角形求解 ----
    ("triangle() 必须是整个表达式的最外层函数，不能参与其它运算", "triangle() 必須是整個算式的最外層函式，不能參與其它運算", "triangle() must be the outermost function of the whole expression; it cannot take part in other operations"),
    ("triangle 函数需要参数: triangle(a=3 b=4 c=5)", "triangle 函式需要參數: triangle(a=3 b=4 c=5)", "triangle needs arguments: triangle(a=3 b=4 c=5)"),
    ("三角形记号格式错误: {0}", "三角形記號格式錯誤: {0}", "bad triangle notation: {0}"),
    ("三角形记号格式错误", "三角形記號格式錯誤", "bad triangle notation"),
    ("未知的三角形记号: {0}", "未知的三角形記號: {0}", "unknown triangle notation: {0}"),
    ("三角形赋值缺少右端表达式", "三角形賦值缺少右端算式", "triangle assignment is missing its right-hand expression"),
    ("三角形求解至少需要 2 个已知量", "三角形求解至少需要 2 個已知量", "triangle solving needs at least 2 known values"),
    ("三角形求解不支持复数", "三角形求解不支援複數", "triangle solving does not support complex numbers"),
    ("三角形某个量被重复赋值: {0}", "三角形某個量被重複賦值: {0}", "a triangle value is assigned twice: {0}"),
    ("边长必须为正数: {0}", "邊長必須為正數: {0}", "side length must be positive: {0}"),
    ("高必须为正数: {0}", "高必須為正數: {0}", "height must be positive: {0}"),
    ("角度必须在 (0, 180°) 范围内: {0}", "角度必須在 (0, 180°) 範圍內: {0}", "angle must be within (0, 180°): {0}"),
    ("三角形不满足三角不等式", "三角形不滿足三角不等式", "the values do not satisfy the triangle inequality"),
    ("已知量之间存在矛盾，无法构成三角形", "已知量之間存在矛盾，無法構成三角形", "the given values are inconsistent — no such triangle"),
    ("该三角形无解", "該三角形無解", "no such triangle exists"),
    ("三角形求解未收敛", "三角形求解未收斂", "triangle solving did not converge"),
    ("已知信息不足，无法确定三角形", "已知資訊不足，無法確定三角形", "not enough information to determine the triangle"),
    ("已知信息不足，无法确定三角形（形状已确定，仅缺一个长度）", "已知資訊不足，無法確定三角形（形狀已確定，僅缺一個長度）", "not enough information (shape is fixed, only a length is missing)"),
    ("已知信息不足，无法确定三角形（形状已确定 a : b : c = {0}，仅缺一个长度）", "已知資訊不足，無法確定三角形（形狀已確定 a : b : c = {0}，僅缺一個長度）", "not enough information (shape fixed: a : b : c = {0}; only a length is missing)"),
    ("解 {0}:", "解 {0}:", "solution {0}:"),
    ("面积", "面積", "area"),
    ("周长", "周長", "perimeter"),
    ("外接圆半径", "外接圓半徑", "circumradius"),
    ("内切圆半径", "內切圓半徑", "inradius"),
    // ---- /reset /save /load 与内联提示 ----
    ("已重置会话（清空 {0} 个变量与 ans）", "已重置工作階段（清空 {0} 個變數與 ans）", "Session reset ({0} variables and ans cleared)"),
    ("已恢复默认设置（模式、颜色、精度），语言保持不变", "已恢復預設設定（模式、顏色、精度），語言保持不變", "Defaults restored (modes, colors, precision); language unchanged"),
    ("已保存会话到 {0}（{1} 个变量）", "已儲存工作階段至 {0}（{1} 個變數）", "Session saved to {0} ({1} variables)"),
    ("保存文件失败: {0}", "儲存檔案失敗: {0}", "failed to write file: {0}"),
    ("已从 {0} 加载 {1} 行", "已從 {0} 載入 {1} 行", "Loaded {1} lines from {0}"),
    ("已从 {0} 加载 {1} 行（其中部分行出错，已跳过）", "已從 {0} 載入 {1} 行（其中部分行出錯，已跳過）", "Loaded {1} lines from {0} (some lines failed and were skipped)"),
    ("/reset [all]（all = 连模式与颜色一起恢复默认）", "/reset [all]（all = 連模式與顏色一起恢復預設）", "/reset [all] (all = also restore modes and colors)"),
    ("/save <文件路径>", "/save <檔案路徑>", "/save <file path>"),
    ("/load <文件路径>", "/load <檔案路徑>", "/load <file path>"),
    ("zh-CN|zh-TW|en（无参数进入选择菜单）", "zh-CN|zh-TW|en（無參數進入選擇選單）", "zh-CN|zh-TW|en (no argument opens the menu)"),
    ("<类别> <颜色>", "<類別> <顏色>", "<category> <color>"),
    ("NAME = <表达式>（变量名须全大写）", "NAME = <算式>（變數名須全大寫）", "NAME = <expression> (uppercase name)"),
    ("NAME 或 all", "NAME 或 all", "NAME or all"),
    // ---- 精度与显示开关（/mode prec|digits|sci|group） ----
    ("已设置工作精度为 {0} 位小数（显示 {1} 位有效数字）", "已設定工作精度為 {0} 位小數（顯示 {1} 位有效數字）", "Working precision set to {0} decimal digits (displaying {1} significant digits)"),
    ("提示: 显示位数（{0}）少于工作精度，可用 /mode digits 提高", "提示: 顯示位數（{0}）少於工作精度，可用 /mode digits 提高", "Note: display digits ({0}) are lower than the working precision; raise them with /mode digits"),
    ("已设置显示 {0} 位有效数字", "已設定顯示 {0} 位有效數字", "Display digits set to {0}"),
    ("已开启科学计数法显示", "已開啟科學記號顯示", "Scientific notation enabled"),
    ("已关闭科学计数法显示（大数完整写出）", "已關閉科學記號顯示（大數完整寫出）", "Scientific notation disabled (large numbers printed in full)"),
    ("已开启千分位分隔", "已開啟千分位分隔", "Digit grouping enabled"),
    ("已关闭千分位分隔", "已關閉千分位分隔", "Digit grouping disabled"),
    // ---- 二元函数（nroot / mod / idiv / nCr / nPr / gcd / lcm / isprime） ----
    ("函数 {0} 需要两个参数", "函數 {0} 需要兩個參數", "function {0} takes two arguments"),
    ("nroot 的次数必须是 >= 2 的整数", "nroot 的次數必須是 >= 2 的整數", "nroot requires an integer degree >= 2"),
    ("nroot 的次数超出支持范围（2 ~ 1000000）", "nroot 的次數超出支援範圍（2 ~ 1000000）", "nroot degree out of range (2 ~ 1000000)"),
    ("负数的偶次根在实数范围内无定义", "負數的偶次根在實數範圍內無定義", "an even root of a negative number is undefined over the reals"),
    ("取模与整除需要精确数值", "取模與整除需要精確數值", "mod/idiv require exact values"),
    ("取模与整除的除数不能为 0", "取模與整除的除數不能為 0", "the divisor of mod/idiv cannot be 0"),
    ("组合数需要非负整数参数", "組合數需要非負整數參數", "nCr requires non-negative integers"),
    ("排列数需要非负整数参数", "排列數需要非負整數參數", "nPr requires non-negative integers"),
    ("组合数参数过大（上限 10000，/mode deep 可取消限制）", "組合數參數過大（上限 10000，/mode deep 可取消限制）", "nCr argument too large (limit 10000; /mode deep removes it)"),
    ("排列数参数过大（上限 10000，/mode deep 可取消限制）", "排列數參數過大（上限 10000，/mode deep 可取消限制）", "nPr argument too large (limit 10000; /mode deep removes it)"),
    ("组合数参数过大", "組合數參數過大", "nCr argument too large"),
    ("排列数参数过大", "排列數參數過大", "nPr argument too large"),
    ("gcd/lcm 需要整数参数", "gcd/lcm 需要整數參數", "gcd/lcm require integer arguments"),
    ("gcd/lcm 的参数不能为 0", "gcd/lcm 的參數不能為 0", "gcd/lcm arguments must be non-zero"),
    // ---- 素因数分解 ----
    ("primefac 的参数必须是非零整数", "primefac 的參數必須是非零整數", "primefac requires a non-zero integer argument"),
    ("primefac() 必须是整个表达式的最外层函数，不能参与其它运算", "primefac() 必須是整個算式的最外層函式，不能參與其它運算", "primefac() must be the outermost function of the whole expression; it cannot take part in other operations"),
    ("primefac 函数需要参数: primefac(12)", "primefac 函式需要參數: primefac(12)", "primefac needs an argument: primefac(12)"),
    ("素因数分解超出预算（Fast 模式 Pollard 迭代上限 2^17）；如确认需要继续，请先执行 /mode deep", "質因數分解超出預算（Fast 模式 Pollard 迭代上限 2^17）；如確認需要繼續，請先執行 /mode deep", "prime factorization exceeded the budget (Fast mode Pollard iteration limit 2^17); run /mode deep to continue"),
    ("0 没有素因数分解", "0 沒有質因數分解", "0 has no prime factorization"),
    // ---- 高等数学：求导 ----
    ("求导变量必须是单个变量（如 x）", "求導變數必須是單個變數（如 x）", "the differentiation variable must be a single variable (e.g. x)"),
    ("函数 {0} 不支持求导", "函數 {0} 不支援求導", "function {0} cannot be differentiated"),
    ("求导结果规模过大（/mode deep 可放宽）", "求導結果規模過大（/mode deep 可放寬）", "derivative too large (run /mode deep to relax)"),
    ("求导展开嵌套过深（/mode deep 可放宽）", "求導展開嵌套過深（/mode deep 可放寬）", "differentiation nesting too deep (run /mode deep to relax)"),
    ("求导变量被求和绑定变量遮蔽", "求導變數被求和綁定變數遮蔽", "the differentiation variable is shadowed by the summation variable"),
    ("高等数学函数 {0} 不能出现在此处（fac/sd/triangle/primefac 内部）", "高等數學函數 {0} 不能出現在此處（fac/sd/triangle/primefac 內部）", "calculus function {0} cannot be used here (inside fac/sd/triangle/primefac)"),
    ("函数 {0} 需要 {1} 个参数", "函數 {0} 需要 {1} 個參數", "function {0} takes {1} argument(s)"),
    ("此处不能使用无穷（inf）", "此處不能使用無窮（inf）", "infinity (inf) is not allowed here"),
    ("等式不能出现在符号运算中", "等式不能出現在符號運算中", "equations cannot appear in symbolic computation"),
    // ---- 高等数学：积分 ----
    ("积分上下限必须是常数（可为 inf）", "積分上下限必須是常數（可為 inf）", "the integration bounds must be constants (may be inf)"),
    ("被积函数在积分区间内出现奇点或非有限值", "被積函數在積分區間內出現奇點或非有限值", "the integrand is singular or non-finite on the interval"),
    ("积分求值次数超出预算（/mode deep 可放宽）", "積分求值次數超出預算（/mode deep 可放寬）", "integration evaluation budget exceeded (run /mode deep to relax)"),
    ("无穷限积分需要能求出原函数并收敛（本次无法判定）", "無窮限積分需要能求出原函數並收斂（本次無法判定）", "an infinite integral needs a convergent antiderivative (could not be determined here)"),
    ("无法求出初等原函数（可改用定积分做数值积分）", "無法求出初等原函數（可改用定積分做數值積分）", "no elementary antiderivative found (use a definite integral for numeric integration)"),
    ("初等原函数表未覆盖该形态（可用定积分做数值积分）", "初等原函數表未覆蓋該形態（可用定積分做數值積分）", "the antiderivative table does not cover this form (use a definite integral for numeric integration)"),
    ("isprime 需要整数参数", "isprime 需要整數參數", "isprime requires an integer argument"),
    ("nextprime 需要整数参数", "nextprime 需要整數參數", "nextprime requires an integer argument"),
    ("素性判定参数过大（上限 10^24，/mode deep 可取消限制）", "素性判定參數過大（上限 10^24，/mode deep 可取消限制）", "primality argument too large (limit 10^24; /mode deep removes it)"),
    // ---- 反双曲 / 双曲倒数 ----
    ("arccosh 的定义域为 x >= 1", "arccosh 的定義域為 x >= 1", "arccosh is defined for x >= 1 only"),
    ("arctanh 的定义域为 |x| < 1", "arctanh 的定義域為 |x| < 1", "arctanh is defined for |x| < 1 only"),
    ("coth 在 x=0 处未定义", "coth 在 x=0 處未定義", "coth is undefined at x=0"),
    ("csch 在 x=0 处未定义", "csch 在 x=0 處未定義", "csch is undefined at x=0"),
    // ---- 命令行 / 计时开关 ----
    ("已开启耗时显示", "已開啟耗時顯示", "Timing display enabled"),
    ("已关闭耗时显示", "已關閉耗時顯示", "Timing display disabled"),
    ("/timing <on|off>", "/timing <on|off>", "/timing <on|off>"),
    ("参数 {0} 缺少取值", "參數 {0} 缺少取值", "option {0} requires a value"),
    ("未知参数: {0}", "未知參數: {0}", "unknown option: {0}"),
    ("无法识别的语言代码: {0}", "無法識別的語言代碼: {0}", "unrecognized language code: {0}"),
    (
        "用法: hipercalc [-e <表达式>] [-f <文件>] [--stdin] [--lang <代码>] [-q]",
        "用法: hipercalc [-e <算式>] [-f <檔案>] [--stdin] [--lang <代碼>] [-q]",
        "Usage: hipercalc [-e <expr>] [-f <file>] [--stdin] [--lang <code>] [-q]",
    ),
    ("读取文件失败: {0}", "讀取檔案失敗: {0}", "failed to read file: {0}"),
    // 文件读写失败的原因（由 `main::io_reason` 归纳，不走操作系统的区域化文案）
    ("文件不存在", "檔案不存在", "file not found"),
    ("没有访问权限", "沒有存取權限", "permission denied"),
    ("文件内容不是有效的文本", "檔案內容不是有效的文字", "the file is not valid text"),
    ("无法访问该文件", "無法存取該檔案", "cannot access the file"),
    // ---- 语言切换（自身） ----
    ("语言 / Language:", "語言 / Language:", "Language:"),
    ("↑/↓ 选择，回车确认（也可直接输入序号或 /lang <代码>）", "↑/↓ 選擇，Enter 確認（也可直接輸入序號或 /lang <代碼>）", "↑/↓ to choose, Enter to confirm (or type the index / `/lang <code>`)"),
    ("已切换语言：{0}（仅对后续输出生效）", "已切換語言：{0}（僅對後續輸出生效）", "Language switched to {0} (applies to subsequent output only)"),
    ("{0}: 无法识别的语言，可选: 1 简体中文 / 2 繁體中文 / 3 English", "{0}: 無法識別的語言，可選: 1 简体中文 / 2 繁體中文 / 3 English", "{0}: unknown language; choose 1 简体中文 / 2 繁體中文 / 3 English"),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// 是否含 CJK 表意文字或中日韩标点（英文/繁体界面里不该出现的东西）
    fn has_cjk(s: &str) -> bool {
        s.chars()
            .any(|c| matches!(c as u32, 0x3000..=0x303F | 0x4E00..=0x9FFF | 0xFF00..=0xFFEF))
    }

    /// 简体**专用**字（繁体写法不同、繁体文本里绝不出现）。只取确定无疑的，宁可漏检不可误报。
    const SIMPLIFIED_ONLY: &str = "为个这说读对时错误认边线门关开进过还将无须见体现实务组织级显术标纪录师\
页码确认语让归键应该们与从会变图数据结设简单独转选输长内层则处决规统记运达观览择环节复杂织";

    /// 语言名按设计用**母语写法**展示（`简体中文` / `繁體中文` 在任何界面里都写自己那一份），
    /// 检查前先剔除，避免把它们当成漏译。
    fn strip_language_names(s: &str) -> String {
        s.replace("简体中文", "").replace("繁體中文", "")
    }

    fn with_lang<T>(lang: Lang, f: impl FnOnce() -> T) -> T {
        let _guard = TEST_LANG_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let prev = get();
        set(lang);
        let r = f();
        set(prev);
        r
    }

    #[test]
    fn english_column_has_no_chinese() {
        // 词条表三列必须同进同退：英文列里混进中文，英文界面就会冒中文。
        for (zh, _tw, en) in TABLE {
            let rest = strip_language_names(en);
            assert!(!has_cjk(&rest), "英文列残留中文: {zh:?} -> {en:?}");
        }
    }

    #[test]
    fn traditional_column_has_no_simplified_only_chars() {
        for (zh, tw, _en) in TABLE {
            let rest = strip_language_names(tw);
            let hit: String = rest
                .chars()
                .filter(|c| SIMPLIFIED_ONLY.contains(*c))
                .collect();
            assert!(hit.is_empty(), "繁体列残留简体字「{hit}」: {zh:?} -> {tw:?}");
        }
    }

    /// 模拟"结果行按 token 着色后"的形态：颜色码会把一句话切成很多段。
    /// 词条匹配必须**跳过颜色码**（见 `find_lit`），否则带占位符的长词条
    /// （`{0} = {1}，k 为整数`、`解 {0}:`）永远匹配不上，英文模式只翻前半句。
    /// 这是真实发生过的 bug（用户实测 `x = 90 + k·180，k 为整数`），用本测试守住。
    const A: &str = "\x1b[97m"; // 结果底色
    const O: &str = "\x1b[1;33m"; // 运算符
    const N: &str = "\x1b[37m"; // 数字
    const R: &str = "\x1b[0m"; // 复位

    #[test]
    fn translations_survive_color_codes() {
        with_lang(Lang::En, || {
            // 角度模式的周期通式
            let line = format!(
                "{A}x {R}{O}={R}{A} {R}{N}90{R}{A} {R}{O}+{R}{A} k{R}{O}·{R}{N}180{R}{A}，k 为整数{R}"
            );
            let out = t(&line);
            assert!(!has_cjk(&out), "周期通式未整体翻译: {out:?}");
            assert!(out.contains("k ∈ ℤ"), "{out:?}");

            // 多解三角形的分节标题（断言前先去掉颜色码：翻译**不会**动原文本里的颜色码，
            // 所以 `solution ` 与 `1:` 之间仍夹着数字色，直接 contains 会误判）
            let line = format!("{A}解 {R}{N}1{R}{A}:{R}");
            let out = t(&line);
            assert!(!has_cjk(&out), "分节标题未翻译: {out:?}");
            assert!(strip_ansi(&out).contains("solution 1:"), "{out:?}");

            // `未知类别` 与 `类别: {0}` 的字面长度相同 ⇒ 必须靠左优先，才能整词条翻译
            let line = format!("\x1b[1;31m未知类别{R}{A}: {R}bogus");
            let out = t(&line);
            assert!(!has_cjk(&out), "未知类别未整词条翻译: {out:?}");
            assert!(strip_ansi(&out).contains("unknown category"), "{out:?}");

            // 带颜色码的多行块也要逐行翻全（旧实现按 ANSI 切段 + 轮数上限会漏尾巴）
            let line = format!(
                "{A}解 {R}{N}1{R}{A}:{R}\n{A}面积 {R}{O}={R}{A} {R}{N}6{R}{A}, 周长 {R}{O}={R}{A} {R}{N}12{R}"
            );
            let out = t(&line);
            assert!(!has_cjk(&out), "多行块未逐行翻全: {out:?}");
            assert!(strip_ansi(&out).contains("perimeter"), "{out:?}");
        });
    }

    #[test]
    fn chinese_mode_is_passthrough() {
        let line = "x = 90 + k·180，k 为整数";
        assert_eq!(with_lang(Lang::ZhCn, || t(line)), line);
        // 简体原文在简体模式下原样返回（含颜色码）
        let colored = format!("{A}x {R}{O}={R}{A} 1{R}");
        assert_eq!(with_lang(Lang::ZhCn, || t(&colored)), colored);
    }
}
