//! 会话状态持久化：显示/角度/计算模式 + `/let` 存储变量 + `/set` 颜色
//!
//! 状态文件与历史文件同目录：`%USERPROFILE%/.hipercalc_state`
//!（`$HOME` 下同名文件作为回退；删除该文件即恢复全部默认设置与空变量表）
//!
//! 格式（每行一项，`#` 开头为注释，空行忽略）：
//! ```text
//! display=mathio|lineio
//! angle=deg|rad
//! calc=fast|deep
//! lang=zh-CN|zh-TW|en
//! timing=on|off
//! prec=80             # 工作精度（小数位数）
//! digits=20           # 显示有效位数
//! sci=on|off          # 是否允许科学计数法
//! group=on|off        # 是否输出千分位
//! var:<NAME>=E:<MathIO 表达式>      # 精确值：存符号形式，加载时重新解析（无损）
//! var:<NAME>=A:<value>:<precision>  # 近似值：存 BigFloat 内部十进制表示（无损）
//! color:<类别>=<颜色名>              # /set 设置的高亮颜色（类别/颜色名的合法性由调用方校验）
//! ```
//!
//! 为什么精确值不直接存数值：`Exact` 的内部结构（多项 + 分母）序列化复杂且易随实现变动，
//! 而 MathIO 的符号输出（`1 / 2`、`(1 / 2)*sqrt(2)`、`pi / 6`）本就是可解析的表达式，
//! 重新解析即可精确还原。近似值则直接用 BigFloat 的 `value`/`precision`（本身就是十进制表示）。

use num_traits::Signed;
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::bigfloat::BigFloat;
use crate::calc_mode::{self, CalcMode};
use crate::display;
use crate::i18n::Lang;
use crate::number::Number;
use crate::parser::{self, DisplayMode, EvalResult, Evaluator};
use crate::trig::AngleMode;

/// 状态文件路径（与 `.hipercalc_history` 同目录）
pub fn state_path() -> Option<PathBuf> {
    let dir = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()?;
    Some(PathBuf::from(dir).join(".hipercalc_state"))
}

/// 会话模式三件套
#[derive(Debug, Clone, Copy)]
pub struct SavedModes {
    pub display: DisplayMode,
    pub angle: AngleMode,
    pub calc: CalcMode,
    /// 是否显示耗时行（默认开）
    pub timing: bool,
    /// 工作精度（小数位数，默认 80）
    pub prec: usize,
    /// 显示有效位数（默认 20）
    pub digits: usize,
    /// 是否允许科学计数法（默认开）
    pub sci: bool,
    /// 是否输出千分位（默认关）
    pub group: bool,
}

/// 序列化单个变量值
pub fn encode_var(v: &Number) -> String {
    match v {
        Number::Exact(_) => format!("E:{}", display::format_mathio(v)),
        Number::Approx(b) => format!("A:{}:{}", b.value, b.precision),
        // 复数一律走符号形式（`E:1 + 2i`），加载时重新解析即可精确还原
        Number::Complex(_) => format!("E:{}", display::format_mathio(v)),
    }
}

/// 把变量编码成**可回放的脚本文本**（`/save` 用）：
/// - 精确值走 MathIO 符号形式（`1 / 2`、`(1 / 2)*sqrt(2)`、`2*pi`），加载时重新解析即可精确还原；
/// - 近似值走完整十进制串（由 `value`/`precision` 直接写出，无精度损失），加载后成为精确十进制。
/// 注意与 `encode_var` 的区别：后者是状态文件的内部格式（`E:`/`A:` 前缀），不能直接喂给求值器。
pub fn encode_var_for_script(v: &Number) -> String {
    match v {
        Number::Exact(_) | Number::Complex(_) => display::format_mathio(v),
        Number::Approx(b) => {
            let neg = b.value.is_negative();
            let digits = b.value.abs().to_string();
            let p = b.precision;
            let s = if p == 0 {
                digits
            } else if digits.len() > p {
                format!(
                    "{}.{}",
                    &digits[..digits.len() - p],
                    &digits[digits.len() - p..]
                )
            } else {
                format!("0.{}{}", "0".repeat(p - digits.len()), digits)
            };
            if neg { format!("-{s}") } else { s }
        }
    }
}

/// 反序列化单个变量值（格式非法或解析失败返回 None，调用方跳过该项）
pub fn decode_var(s: &str) -> Option<Number> {
    if let Some(rest) = s.strip_prefix("A:") {
        // 近似值：BigFloat 的内部十进制表示（尾数:小数位数），直接还原，无精度损失
        let (val, prec) = rest.split_once(':')?;
        let value = val.parse::<num_bigint::BigInt>().ok()?;
        let precision = prec.parse::<usize>().ok()?;
        return Some(Number::Approx(BigFloat { value, precision }));
    }
    // 精确值：重新解析符号表达式（用临时 Evaluator，避免影响会话自身的 ans/vars）
    let expr = s.strip_prefix("E:").unwrap_or(s);
    let mut ev = Evaluator::new();
    match parser::parse_and_eval(expr, &mut ev) {
        Ok(EvalResult::Value(v)) | Ok(EvalResult::SdValue(v)) => Some(v),
        _ => None,
    }
}

/// 读取状态文件：返回（模式, 变量表, 颜色表, 界面语言）。
/// 颜色表为 `(类别, 颜色名)` 原始字符串，合法性（类别/颜色名是否受支持）由调用方校验后应用。
/// 语言为 None 表示文件里没有记录（首次启动）⇒ 调用方按系统语言判定。
/// 文件不存在或不可读时返回 None（调用方保持默认设置）。
pub fn load()
-> Option<(SavedModes, Vec<(String, Number)>, Vec<(String, String)>, Option<Lang>)> {
    let path = state_path()?;
    let text = std::fs::read_to_string(path).ok()?;

    let mut display = DisplayMode::LineIO;
    let mut angle = AngleMode::Radian;
    let mut calc = CalcMode::Fast;
    let mut timing = true;
    let mut prec = crate::bigfloat::DEFAULT_PRECISION;
    let mut digits = crate::bigfloat::DEFAULT_DISPLAY_DIGITS;
    let mut sci = true;
    let mut group = false;
    let mut vars: Vec<(String, Number)> = Vec::new();
    let mut colors: Vec<(String, String)> = Vec::new();
    let mut lang: Option<Lang> = None;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(v) = line.strip_prefix("display=") {
            display = match v {
                "mathio" => DisplayMode::MathIO,
                _ => DisplayMode::LineIO,
            };
        } else if let Some(v) = line.strip_prefix("angle=") {
            angle = match v {
                "deg" => AngleMode::Degree,
                _ => AngleMode::Radian,
            };
        } else if let Some(v) = line.strip_prefix("calc=") {
            calc = match v {
                "deep" => CalcMode::Deep,
                _ => CalcMode::Fast,
            };
        } else if let Some(v) = line.strip_prefix("var:") {
            if let Some((name, val)) = v.split_once('=') {
                let name = name.trim();
                // 变量名合法性：全大写字母/下划线（与 /let 的校验一致）
                if !name.is_empty()
                    && name.chars().all(|c| c.is_ascii_uppercase() || c == '_')
                {
                    if let Some(n) = decode_var(val.trim()) {
                        vars.push((name.to_string(), n));
                    }
                }
            }
        } else if let Some(v) = line.strip_prefix("prec=") {
            if let Ok(n) = v.trim().parse::<usize>() {
                prec = n;
            }
        } else if let Some(v) = line.strip_prefix("digits=") {
            if let Ok(n) = v.trim().parse::<usize>() {
                digits = n;
            }
        } else if let Some(v) = line.strip_prefix("sci=") {
            sci = v.trim() != "off";
        } else if let Some(v) = line.strip_prefix("group=") {
            group = v.trim() == "on";
        } else if let Some(v) = line.strip_prefix("timing=") {
            timing = v.trim() != "off";
        } else if let Some(v) = line.strip_prefix("lang=") {
            lang = Lang::parse(v);
        } else if let Some(v) = line.strip_prefix("color:") {
            if let Some((cat, name)) = v.split_once('=') {
                let cat = cat.trim();
                let name = name.trim();
                if !cat.is_empty() && !name.is_empty() {
                    colors.push((cat.to_string(), name.to_string()));
                }
            }
        }
    }

    Some((
        SavedModes {
            display,
            angle,
            calc,
            timing,
            prec,
            digits,
            sci,
            group,
        },
        vars,
        colors,
        lang,
    ))
}

/// 写入状态文件（模式 + 变量表 + 颜色表）
pub fn save(
    display: DisplayMode,
    angle: AngleMode,
    calc: CalcMode,
    lang: Lang,
    timing: bool,
    prec: usize,
    digits: usize,
    sci: bool,
    group: bool,
    vars: &BTreeMap<String, Number>,
    colors: &[(&'static str, &'static str)],
) -> std::io::Result<()> {
    let path = state_path().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "未找到用户主目录")
    })?;

    let mut out = String::new();
    out.push_str("# HiPerCalc 会话状态（自动生成）：显示/角度/计算模式、界面语言、/let 存储变量与 /set 颜色\n");
    out.push_str("# 删除本文件即可恢复默认设置并清空变量\n");
    out.push_str(&format!(
        "display={}\n",
        match display {
            DisplayMode::MathIO => "mathio",
            DisplayMode::LineIO => "lineio",
        }
    ));
    out.push_str(&format!(
        "angle={}\n",
        match angle {
            AngleMode::Degree => "deg",
            AngleMode::Radian => "rad",
        }
    ));
    out.push_str(&format!(
        "calc={}\n",
        match calc {
            CalcMode::Deep => "deep",
            CalcMode::Fast => "fast",
        }
    ));
    out.push_str(&format!("lang={}\n", lang.code()));
    out.push_str(&format!("timing={}\n", if timing { "on" } else { "off" }));
    out.push_str(&format!("prec={prec}\n"));
    out.push_str(&format!("digits={digits}\n"));
    out.push_str(&format!("sci={}\n", if sci { "on" } else { "off" }));
    out.push_str(&format!("group={}\n", if group { "on" } else { "off" }));
    for (name, v) in vars {
        out.push_str(&format!("var:{}={}\n", name, encode_var(v)));
    }
    for (cat, name) in colors {
        out.push_str(&format!("color:{}={}\n", cat, name));
    }
    std::fs::write(path, out)
}

/// 当前计算模式（从全局开关读取，供保存时使用）
pub fn current_calc_mode() -> CalcMode {
    if calc_mode::is_deep() {
        CalcMode::Deep
    } else {
        CalcMode::Fast
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 求值一个表达式串（用于回放校验）
    fn eval(expr: &str) -> Number {
        let mut ev = Evaluator::new();
        match parser::parse_and_eval(expr, &mut ev) {
            Ok(EvalResult::Value(v)) | Ok(EvalResult::SdValue(v)) => v,
            Err(e) => panic!("{expr} 未能求值: {e}"),
            Ok(_) => panic!("{expr} 返回的不是数值"),
        }
    }

    #[test]
    fn script_encoding_round_trips() {
        // 精确值：MathIO 符号形式可直接回放（分数、根式、π）
        for expr in ["1/2", "sqr(2)/2", "2*pi", "-3/4", "5"] {
            let v = eval(expr);
            let enc = encode_var_for_script(&v);
            let back = eval(&enc);
            assert_eq!(
                display::format_mathio(&v),
                display::format_mathio(&back),
                "{expr} 回放不一致（编码为 {enc}）"
            );
        }
        // 近似值：完整十进制串回放后数值一致（显示 20 位有效数字相同）
        // 注意 pi / sqrt(2) 求值得到的是「精确值」（Pi/Sqrt 项），这里用 sin(1) 这类无精确形式的值
        let approx = eval("sin(1)");
        let enc = encode_var_for_script(&approx);
        assert!(enc.starts_with("0.8414709848"), "{enc}");
        assert!(enc.len() > 60, "近似值应写成完整十进制串: {enc}");
        let back = eval(&enc);
        assert_eq!(
            display::format_lineio(&approx),
            display::format_lineio(&back),
            "近似值回放不一致（编码为 {enc}）"
        );
    }
}
