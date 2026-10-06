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
        let t = t
            .trim_start_matches('/')
            .trim_start_matches("language")
            .trim_start_matches("lang");
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
        Lang::En
    }
    // 2) 其它平台：退回环境变量（zh* → 中文，其余非空 → 英文）
    #[cfg(not(windows))]
    {
        for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(v) = std::env::var(key) {
                let v = v.to_lowercase();
                if v.starts_with("zh") {
                    if v.contains("tw")
                        || v.contains("hk")
                        || v.contains("mo")
                        || v.contains("hant")
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
    let target = crate::language::get(lang.code())
        .iter()
        .find(|(k, _)| k == template)
        .map(|(_, v)| v.as_str())
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
            if let Ok(k) = num.parse::<usize>()
                && let Some(v) = values.get(k)
            {
                out.push_str(v);
                continue;
            }
        }
        out.push('{');
        out.push_str(&num);
    }
    out
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
    // 词条来自 `src/language/<代码>.json`（编译期内嵌、按语言缓存一次）
    let mut best: Option<(usize, usize, usize, Vec<(usize, usize, String)>, usize)> = None;
    for (idx, (zh, target)) in crate::language::get(lang.code()).iter().enumerate() {
        if target == zh {
            continue; // 该条无需翻译（纯符号，或两种语言写法相同）
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
    let target = &crate::language::get(lang.code())[idx].1;
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
                if let Ok(k) = num.parse::<usize>()
                    && let Some((_, _, v)) = values.get(k)
                {
                    filled.push_str(&translate_run(v, depth - 1));
                    continue;
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
// 词条表已迁移到 `src/language/<语言代码>.json`（编译期内嵌）

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
    fn english_map_has_no_chinese() {
        // 英文词条里混进中文，英文界面就会冒中文
        for (zh, en) in crate::language::get("en") {
            let rest = strip_language_names(en);
            assert!(!has_cjk(&rest), "英文词条残留中文: {zh:?} -> {en:?}");
        }
    }

    #[test]
    fn traditional_map_has_no_simplified_only_chars() {
        for (zh, tw) in crate::language::get("zh-TW") {
            let rest = strip_language_names(tw);
            let hit: String = rest
                .chars()
                .filter(|c| SIMPLIFIED_ONLY.contains(*c))
                .collect();
            assert!(hit.is_empty(), "繁体词条残留简体字「{hit}」: {zh:?} -> {tw:?}");
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
