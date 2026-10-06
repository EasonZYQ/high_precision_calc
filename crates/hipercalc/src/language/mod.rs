//! 多语言词条：**每种语言一个 JSON 文件**，编译期内嵌。
//!
//! ```text
//! src/language/
//! ├── zh-CN.json    简体（空表：简体原文**就是键**，不需要条目）
//! ├── zh-TW.json    {"简体原文": "繁體譯文", ...}
//! └── en.json       {"简体原文": "English text", ...}
//! ```
//!
//! # 新增一门语言要做什么
//!
//! 1. 在这个目录放 `xx.json`（键为**简体原文**，值与键逐字对应，含 `{0}`/`{1}` 占位符）；
//! 2. 在 [`FILES`] 里登记 `(语言代码, 内嵌内容)`；
//! 3. `src/i18n.rs` 的 `Lang` 加一个变体并映射到该代码；
//! 4. 跑 `cargo test` —— `i18n::tests` 会自动检查各语言**键集是否一致**、
//!    占位符能否对上、译文里有没有混进中文（英文列）或简体字（繁体列）。
//!
//! # 为什么不引 serde
//!
//! 本项目一直保持"零额外依赖"（大整数除法、整数平方根都是自己写的）。
//! 这里需要的只是"平面字符串到字符串的映射"，一个几十行的解析器足够，
//! 而且**可以自己写测试**验证转义处理。

/// 登记所有语言文件：`(语言代码, 内嵌内容)`
static FILES: &[(&str, &str)] = &[
    ("zh-CN", include_str!("zh-CN.json")),
    ("zh-TW", include_str!("zh-TW.json")),
    ("en", include_str!("en.json")),
];

/// 已知语言代码（仅测试用于枚举各语言文件）
#[cfg(test)]
pub fn all_tags() -> impl Iterator<Item = &'static str> {
    FILES.iter().map(|(tag, _)| *tag)
}

/// 取某个语言的 `(简体原文, 译文)` 列表；语言不存在时返回空表。
///
/// 解析结果按语言缓存一次（`OnceLock`），后续调用零开销。
pub fn get(tag: &str) -> &'static [(String, String)] {
    use std::sync::OnceLock;
    static CACHE: OnceLock<Vec<(&'static str, OnceLock<Vec<(String, String)>>)>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| FILES.iter().map(|(t, _)| (*t, OnceLock::new())).collect());
    for (t, slot) in cache {
        if *t == tag {
            return slot.get_or_init(|| {
                let raw = FILES
                    .iter()
                    .find(|(x, _)| *x == tag)
                    .map(|(_, s)| *s)
                    .unwrap_or("{}");
                // 解析失败不 panic（否则整个程序起不来）：退化成"无翻译"，
                // 并由 `json_files_parse` 测试在 CI 阶段拦住。
                parse_object(raw).unwrap_or_default()
            });
        }
    }
    &[]
}

/* ---------------- 极简 JSON 解析（只支持"平面对象：字符串→字符串"） ---------------- */

/// 解析 `{"k": "v", ...}`。不支持嵌套、数组、数字、`null` —— 词条表用不到。
pub fn parse_object(src: &str) -> Result<Vec<(String, String)>, String> {
    let b: Vec<char> = src.chars().collect();
    let mut i = 0usize;
    skip_ws(&b, &mut i);
    expect(&b, &mut i, '{')?;
    let mut out = Vec::new();
    loop {
        skip_ws(&b, &mut i);
        if peek(&b, i) == Some('}') {
            break;
        }
        let key = parse_string(&b, &mut i)?;
        skip_ws(&b, &mut i);
        expect(&b, &mut i, ':')?;
        skip_ws(&b, &mut i);
        let value = parse_string(&b, &mut i)?;
        out.push((key, value));
        skip_ws(&b, &mut i);
        match peek(&b, i) {
            Some(',') => {
                i += 1;
            }
            Some('}') => {
                break;
            }
            other => return Err(format!("期望 `,` 或 `}}`，实际是 {other:?}")),
        }
    }
    Ok(out)
}

fn peek(b: &[char], i: usize) -> Option<char> {
    b.get(i).copied()
}

fn skip_ws(b: &[char], i: &mut usize) {
    while let Some(c) = peek(b, *i) {
        if c.is_whitespace() {
            *i += 1;
        } else {
            break;
        }
    }
}

fn expect(b: &[char], i: &mut usize, want: char) -> Result<(), String> {
    if peek(b, *i) == Some(want) {
        *i += 1;
        Ok(())
    } else {
        Err(format!("期望 {want:?}，实际是 {:?}", peek(b, *i)))
    }
}

fn parse_string(b: &[char], i: &mut usize) -> Result<String, String> {
    expect(b, i, '"')?;
    let mut out = String::new();
    loop {
        let Some(c) = peek(b, *i) else {
            return Err("字符串未闭合".to_string());
        };
        *i += 1;
        match c {
            '"' => return Ok(out),
            '\\' => {
                let Some(e) = peek(b, *i) else {
                    return Err("转义符后缺少字符".to_string());
                };
                *i += 1;
                match e {
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    '/' => out.push('/'),
                    'b' => out.push('\u{8}'),
                    'f' => out.push('\u{c}'),
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    'u' => {
                        // \uXXXX（含代理对）
                        let hi = read_hex4(b, i)?;
                        let ch = if (0xD800..0xDC00).contains(&hi) {
                            // 高代理：后面必须紧跟 \uXXXX 低代理
                            if peek(b, *i) == Some('\\') && peek(b, *i + 1) == Some('u') {
                                *i += 2;
                                let lo = read_hex4(b, i)?;
                                let cp = 0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00);
                                char::from_u32(cp).ok_or_else(|| "非法代理对".to_string())?
                            } else {
                                return Err("高代理后缺少低代理".to_string());
                            }
                        } else {
                            char::from_u32(hi).ok_or_else(|| "非法码点".to_string())?
                        };
                        out.push(ch);
                    }
                    other => return Err(format!("不支持的转义 \\{other}")),
                }
            }
            other => out.push(other),
        }
    }
}

fn read_hex4(b: &[char], i: &mut usize) -> Result<u32, String> {
    let mut v = 0u32;
    for _ in 0..4 {
        let Some(c) = peek(b, *i) else {
            return Err("\\u 后不足 4 位".to_string());
        };
        *i += 1;
        let d = c
            .to_digit(16)
            .ok_or_else(|| format!("非法十六进制位 {c:?}"))?;
        v = v * 16 + d;
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flat_object_with_escapes() {
        let s = r#"{
  "a": "b",
  "回车": "\r用时：{0}秒",
  "引号": "say \"hi\"",
  "反斜杠": "c:\\tmp",
  "unicode": "\u4e2d\u6587"
}"#;
        let v = parse_object(s).unwrap();
        assert_eq!(v.len(), 5);
        assert_eq!(v[0], ("a".to_string(), "b".to_string()));
        assert_eq!(v[1].1, "\r用时：{0}秒");
        assert_eq!(v[2].1, "say \"hi\"");
        assert_eq!(v[3].1, "c:\\tmp");
        assert_eq!(v[4].1, "中文");
    }

    #[test]
    fn rejects_malformed_input() {
        assert!(parse_object("{").is_err());
        assert!(parse_object(r#"{"a" "b"}"#).is_err());
        assert!(parse_object(r#"{"a": "b""#).is_err());
        assert!(parse_object(r#"{"a": 'b'}"#).is_err());
    }

    #[test]
    fn empty_object_is_fine() {
        assert!(parse_object("{}").unwrap().is_empty());
        assert!(parse_object("{\n}\n").unwrap().is_empty());
    }

    #[test]
    fn all_registered_files_parse_and_agree_on_keys() {
        // 各语言**键集必须一致**：漏翻一条就是界面上残留中文
        let mut base: Option<Vec<String>> = None;
        for tag in all_tags() {
            let entries = get(tag);
            if entries.is_empty() && tag == "zh-CN" {
                continue; // 简体即键，空表是有意的
            }
            let mut keys: Vec<String> = entries.iter().map(|(k, _)| k.clone()).collect();
            keys.sort();
            match &base {
                None => base = Some(keys),
                Some(b) => assert_eq!(&keys, b, "{tag} 的键集与其它语言不一致"),
            }
        }
        assert!(base.is_some());
    }

    #[test]
    fn placeholders_match_keys() {
        for tag in all_tags() {
            for (k, v) in get(tag) {
                let n = k.matches('{').count();
                assert_eq!(
                    n,
                    v.matches('{').count(),
                    "{tag}: 占位符数量不一致 {k:?} / {v:?}"
                );
                for i in 0..n {
                    let ph = format!("{{{i}}}");
                    assert!(v.contains(&ph), "{tag}: 缺占位符 {ph} —— {k:?} / {v:?}");
                }
            }
        }
    }
}
