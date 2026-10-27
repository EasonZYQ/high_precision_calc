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
use std::path::{Path, PathBuf};

use crate::i18n::Lang;
use crate::parser::{self, DisplayMode, EvalResult, Evaluator};
use hipercalc_core::bigfloat::BigFloat;
use hipercalc_core::calc_mode::{self, CalcMode};
use hipercalc_core::display;
use hipercalc_core::number::Number;
use hipercalc_core::trig::AngleMode;

/// `save()` 自己会写的键。其余行（GUI 的偏好、用户手写的键）一律[原样保留](foreign_lines)。
const OWN_KEYS: [&str; 11] = [
    "display=", "angle=", "calc=", "lang=", "timing=", "prec=", "digits=", "sci=", "group=",
    "var:", "color:",
];

/// 从旧文件里挑出**不属于本模块**的行，原样带回。
///
/// ## 为什么必须有
///
/// CLI 与 GUI **共用**这一份状态文件（用户明确要求）。CLI 的 [`save`] 会把整份文件
/// 重写一遍 —— 若不做这一步，用户每用一次命令行就会把 GUI 里设置的
/// 深色 / 玻璃 / 行布局偏好抹掉。这类"互相清空对方设置"的问题**极难排查**
///（症状只是"设置有时会自己变回去"）。
///
/// 注释行与空行**不带回**：`save()` 自己会重写文件头的注释，
/// 重复带会让注释越攒越多。
fn foreign_lines(path: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter(|l| {
            let t = l.trim();
            if t.is_empty() || t.starts_with('#') {
                return false;
            }
            !OWN_KEYS.iter().any(|k| t.starts_with(k))
        })
        .map(|l| l.to_string())
        .collect()
}

/// GUI 专属偏好。
///
/// CLI **不解释**这几个键，但保存时会[原样保留](foreign_lines) —— 于是两个前端
/// 可以共用一份状态文件而互不干扰。
///
/// 放在 `state.rs` 而不是 `hipercalc-gui`：状态文件的格式应当由**一个地方**说了算，
/// 两个 crate 各写一半是最容易漂移的做法。GUI 侧只通过 `hipercalc::session`
/// 的门面读写它。
// ⚠️ 不能 `Copy` 了：`texture` 是 `String`（模式名要能原样存进配置文件）。
// 需要按值用两次的地方请 `.clone()`。
#[derive(Debug, Clone, PartialEq)]
pub struct GuiPrefs {
    /// 深色模式（默认深色 —— 与 `theme::DARK` 的静态初值一致）
    pub dark: bool,
    /// 玻璃（liquid glass）强度：`0.0` = 关闭（不透明纯色），`1.0` = 最强
    pub glass: f32,
    /// **渲染缩放**（窗口尺寸比例）：`1.0` = 940×900（见 `app::WINDOW_W/H`），`0.8` = 752×720。
    ///
    /// ⚠️ iced 0.14 **没有**"降低渲染分辨率"的能力（surface 恒等于窗口物理像素），
    /// 所以这里能调的只有**窗口的物理像素量** —— 帧时间与像素数近似线性，
    /// 这是唯一确定能压低帧时间的旋钮（见 `docs/liquid-glass-report.md` 第 10 节）。
    pub ui_scale: f32,
    /// **允许被截图 / 录屏**（`true` = 允许）。
    ///
    /// ⚠️ 这两个目标在 Windows 上**不能同时满足**：
    /// 玻璃背景要"窗口背后的桌面"，而 `IDXGIOutputDuplication` 只能拿到
    /// 包含自己的合成画面 —— 想把自己排除掉就得开 `WDA_EXCLUDEFROMCAPTURE`，
    /// 那个开关对**所有**捕获者生效（微信/PrtSc/录屏都会拍不到窗口）。
    /// 所以做成开关，用户按场景选：
    /// - `true`（默认）：能截图；窗口背后的背景用"周围一圈的平均色"近似。
    /// - `false`：背景是**真实桌面**；截图/录屏里窗口会消失。
    pub capture: bool,
    /// **质感模式**：`acrylic`（亚克力）/ `glass`（玻璃）/ `off`（关闭）。
    ///
    /// 用户规格：三选一、互斥生效。每个模式有自己的一组旋钮（见下）。
    pub texture: String,
    /// **玻璃模式**的模糊强度（与亚克力的 `glass` 独立）
    pub glass_blur: f32,
    /// **玻璃模式**的底色浓度（与亚克力的 `glass_tint` 独立）
    pub glass_tint2: f32,
    /// 玻璃**底色的浓度**：`0.0` = 完全透明，`1.0` = 完全不透明。
    ///
    /// 与 `glass` 的分工（用户要求两者都要）：强度是"整体玻璃效果有多强"
    ///（同时影响底色透明度、描边高光、投影），底色只拧"面板底色有多实"这一个旋钮。
    pub glass_tint: f32,
    /// 面板**投影**强度：`0.0` = 完全没有阴影（界面会很"平"），`1.0` = 最强
    pub shadow: f32,
    /// 长结果的处理方式：`true` = 自动换行，`false` = 横向滚动
    pub wrap: bool,
    /// 上次退出时的窗口位置与大小（**逻辑像素**，与 winit 口径一致）。
    ///
    /// `win_w == 0.0` 表示"没有记录"（首次启动）⇒ 由 `main.rs` 居中打开。
    /// 只记**非最大化**时的几何：最大化状态下窗口铺满屏幕，记下来会让用户
    /// 还原后得到一个全屏大小的普通窗口。
    pub win_x: f32,
    pub win_y: f32,
    pub win_w: f32,
    pub win_h: f32,
    /// 上次退出时是否处于最大化（下次启动先还原成这个状态）
    pub win_max: bool,
    /// **设置面板自己的浓度**（亚克力模式）：0 = 全透，1 = 与主面板同档
    pub acrylic_dialog_tint: f32,
    /// **设置面板自己的浓度**（玻璃模式）
    pub glass_dialog_tint: f32,
}

impl Default for GuiPrefs {
    fn default() -> Self {
        Self {
            dark: true,
            // 默认亚克力（用户规格里的第一项）
            texture: "acrylic".to_string(),
            // 默认允许截图（宁可背景近似，也不要"截图拍不到窗口"这种静默失败）
            capture: true,
            // 默认 100%：缩放在设置里可调
            ui_scale: 1.0,
            glass_blur: 0.6,
            glass_tint2: 0.55,
            // 0.6 是"看得出玻璃、又不至于糊成一片"的中间值（滑条默认落在这里）
            glass: 0.6,
            // 0.7 —— 与 `theme::GLASS_TINT` 的静态初值保持一致，见那里的说明
            glass_tint: 0.7,
            // 100% —— 与 `theme::SHADOW_PERCENT` 的静态初值一致（不改变现有观感）
            shadow: 1.0,
            wrap: true,
            // 设置面板默认与主面板同档（不额外加实）
            acrylic_dialog_tint: 1.0,
            glass_dialog_tint: 1.0,
            // 0 = 没有记录 ⇒ 首次启动居中
            win_x: 0.0,
            win_y: 0.0,
            win_w: 0.0,
            win_h: 0.0,
            win_max: false,
        }
    }
}

impl GuiPrefs {
    /// 从状态文件读（键缺失或文件不存在时用默认值）
    pub fn load() -> Self {
        match state_path() {
            Some(p) => Self::load_at(&p),
            None => Self::default(),
        }
    }

    /// 同 [`GuiPrefs::load`]，但路径由调用方给（单测用临时文件）。
    pub fn load_at(path: &Path) -> Self {
        let mut p = Self::default();
        let Ok(text) = std::fs::read_to_string(path) else {
            return p;
        };
        for line in text.lines() {
            let line = line.trim();
            if let Some(v) = line.strip_prefix("dark=") {
                p.dark = v.trim() != "off";
            } else if let Some(v) = line.strip_prefix("glass=")
                && let Ok(f) = v.trim().parse::<f32>()
            {
                p.glass = f.clamp(0.0, 1.0);
            } else if let Some(v) = line.strip_prefix("ui_scale=")
                && let Ok(f) = v.trim().parse::<f32>()
            {
                p.ui_scale = f.clamp(0.5, 1.0);
            } else if let Some(v) = line.strip_prefix("capture=") {
                p.capture = v.trim() != "off";
            } else if let Some(v) = line.strip_prefix("texture=") {
                p.texture = v.trim().to_string();
            } else if let Some(v) = line.strip_prefix("glass2_blur=")
                && let Ok(f) = v.trim().parse::<f32>()
            {
                p.glass_blur = f.clamp(0.0, 1.0);
            } else if let Some(v) = line.strip_prefix("glass2_tint=")
                && let Ok(f) = v.trim().parse::<f32>()
            {
                p.glass_tint2 = f.clamp(0.0, 1.0);
            } else if let Some(v) = line.strip_prefix("glass_tint=")
                && let Ok(f) = v.trim().parse::<f32>()
            {
                p.glass_tint = f.clamp(0.0, 1.0);
            } else if let Some(v) = line.strip_prefix("shadow=")
                && let Ok(f) = v.trim().parse::<f32>()
            {
                p.shadow = f.clamp(0.0, 1.0);
            } else if let Some(v) = line.strip_prefix("acrylic_dlg_tint=")
                && let Ok(f) = v.trim().parse::<f32>()
            {
                p.acrylic_dialog_tint = f.clamp(0.0, 1.0);
            } else if let Some(v) = line.strip_prefix("glass2_dlg_tint=")
                && let Ok(f) = v.trim().parse::<f32>()
            {
                p.glass_dialog_tint = f.clamp(0.0, 1.0);
            } else if let Some(v) = line.strip_prefix("wrap=") {
                p.wrap = v.trim() != "off";
            } else if let Some(v) = line.strip_prefix("win_x=")
                && let Ok(f) = v.trim().parse::<f32>()
            {
                p.win_x = f;
            } else if let Some(v) = line.strip_prefix("win_y=")
                && let Ok(f) = v.trim().parse::<f32>()
            {
                p.win_y = f;
            } else if let Some(v) = line.strip_prefix("win_w=")
                && let Ok(f) = v.trim().parse::<f32>()
            {
                p.win_w = f.max(0.0);
            } else if let Some(v) = line.strip_prefix("win_h=")
                && let Ok(f) = v.trim().parse::<f32>()
            {
                p.win_h = f.max(0.0);
            } else if let Some(v) = line.strip_prefix("win_max=") {
                p.win_max = v.trim() != "off";
            }
        }
        p
    }

    /// 转成可写进状态文件的键值对
    /// ⚠️ 参数是 `&self`：`GuiPrefs` 里现在有 `String`（模式名），不再是 `Copy` 类型。
    /// 之前写成 `self` 时靠 `Copy` 蒙混过关，去掉 Copy 后调用方会"prefs 被 move"。
    pub fn to_keys(&self) -> Vec<(&'static str, String)> {
        vec![
            ("dark", if self.dark { "on" } else { "off" }.to_string()),
            (
                "capture",
                if self.capture { "on" } else { "off" }.to_string(),
            ),
            ("ui_scale", format!("{:.2}", self.ui_scale)),
            ("glass", format!("{:.2}", self.glass)),
            ("glass_tint", format!("{:.2}", self.glass_tint)),
            ("texture", self.texture.clone()),
            ("glass2_blur", format!("{:.2}", self.glass_blur)),
            ("glass2_tint", format!("{:.2}", self.glass_tint2)),
            ("shadow", format!("{:.2}", self.shadow)),
            (
                "acrylic_dlg_tint",
                format!("{:.2}", self.acrylic_dialog_tint),
            ),
            ("glass2_dlg_tint", format!("{:.2}", self.glass_dialog_tint)),
            ("wrap", if self.wrap { "on" } else { "off" }.to_string()),
            ("win_x", format!("{:.0}", self.win_x)),
            ("win_y", format!("{:.0}", self.win_y)),
            ("win_w", format!("{:.0}", self.win_w)),
            ("win_h", format!("{:.0}", self.win_h)),
            (
                "win_max",
                if self.win_max { "on" } else { "off" }.to_string(),
            ),
        ]
    }
}

/// **键级更新**：只替换给定的几个键，文件里其余内容（注释、变量表、其它键）
/// 原样保留。文件不存在时新建（只含给定的键 + 一行注释头）。
///
/// 这是 GUI 侧写配置的入口（见 [`GuiPrefs`]）。与 [`save`] 的分工：
/// - `save` 是 CLI 的"整份重写"（它持有变量表与颜色表，那些只有它知道）；
/// - `update_keys` 是"我只改我关心的那几个键"，不需要读懂文件的其余部分。
pub fn update_keys(pairs: &[(&str, String)]) -> std::io::Result<()> {
    let path = state_path()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "未找到用户主目录"))?;
    update_keys_at(&path, pairs)
}

/// [`update_keys`] 的实现（路径由调用方给，单测用临时文件跑）。
fn update_keys_at(path: &Path, pairs: &[(&str, String)]) -> std::io::Result<()> {
    let old = std::fs::read_to_string(path).unwrap_or_default();

    let keys: Vec<String> = pairs.iter().map(|(k, _)| format!("{k}=")).collect();

    let mut out = String::new();
    if old.trim().is_empty() {
        out.push_str("# HiPerCalc 设置（自动生成）：GUI 与命令行共用这一份文件\n");
        out.push_str("# 删除本文件即可恢复默认设置\n");
    } else {
        // 旧内容：跳过本次要改的键，其余原样保留（含 `#` 注释与空行）
        for line in old.lines() {
            let t = line.trim_start();
            if keys.iter().any(|k| t.starts_with(k.as_str())) {
                continue;
            }
            out.push_str(line);
            out.push('\n');
        }
    }
    // 新值（放在末尾：这几个键是"当前值"，放尾部比夹在中间好找）
    for (k, v) in pairs {
        out.push_str(&format!("{k}={v}\n"));
    }
    std::fs::write(path, out)
}

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
        // 矩阵走 `M:[[1, 2], [3, 4]]` —— 这正是可重新解析的字面量形式
        Number::Matrix(_) => format!("M:{}", display::format_mathio(v)),
        // 量纲值走符号形式（`3000 m`）；注意：SI 基本单位是单字母，重新解析时
        // 不会当成单位（单字母单位刻意不收）—— 即量纲变量不保证往返，已在此注明。
        Number::Quantity(_) => format!("E:{}", display::format_mathio(v)),
        // 复数一律走符号形式（`E:1 + 2i`），加载时重新解析即可精确还原
        Number::Complex(_) => format!("E:{}", display::format_mathio(v)),
    }
}

/// 把变量编码成**可回放的脚本文本**（`/save` 用）：
/// - 精确值走 MathIO 符号形式（`1 / 2`、`(1 / 2)*sqrt(2)`、`2*pi`），加载时重新解析即可精确还原；
/// - 近似值走完整十进制串（由 `value`/`precision` 直接写出，无精度损失），加载后成为精确十进制。
///
/// 注意与 `encode_var` 的区别：后者是状态文件的内部格式（`E:`/`A:` 前缀），不能直接喂给求值器。
pub fn encode_var_for_script(v: &Number) -> String {
    match v {
        Number::Exact(_) | Number::Complex(_) | Number::Matrix(_) | Number::Quantity(_) => {
            display::format_mathio(v)
        }
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
pub fn load() -> Option<(
    SavedModes,
    Vec<(String, Number)>,
    Vec<(String, String)>,
    Option<Lang>,
)> {
    let path = state_path()?;
    let text = std::fs::read_to_string(path).ok()?;

    let mut display = DisplayMode::LineIO;
    let mut angle = AngleMode::Radian;
    let mut calc = CalcMode::Fast;
    let mut timing = true;
    let mut prec = hipercalc_core::bigfloat::DEFAULT_PRECISION;
    let mut digits = hipercalc_core::bigfloat::DEFAULT_DISPLAY_DIGITS;
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
                    && let Some(n) = decode_var(val.trim())
                {
                    vars.push((name.to_string(), n));
                }
            }
        } else if let Some(v) = line.strip_prefix("prec=") {
            if let Ok(n) = v.trim().parse::<usize>() {
                // 与 `/mode prec` 的范围校验一致：状态文件是可以手改的，
                // 这里不收敛的话就能绕过命令行的 20..2000 限制（`prec=0` 会让显示与判定失真）。
                prec = n.clamp(crate::MIN_PRECISION, crate::MAX_PRECISION);
            }
        } else if let Some(v) = line.strip_prefix("digits=") {
            if let Ok(n) = v.trim().parse::<usize>() {
                // 显示位数上限是 `prec - 1`（见 `/mode digits`），此处 prec 已收敛过
                digits = n.clamp(1, prec.saturating_sub(1).max(1));
            }
        } else if let Some(v) = line.strip_prefix("sci=") {
            sci = v.trim() != "off";
        } else if let Some(v) = line.strip_prefix("group=") {
            group = v.trim() == "on";
        } else if let Some(v) = line.strip_prefix("timing=") {
            timing = v.trim() != "off";
        } else if let Some(v) = line.strip_prefix("lang=") {
            lang = Lang::parse(v);
        } else if let Some(v) = line.strip_prefix("color:")
            && let Some((cat, name)) = v.split_once('=')
        {
            let cat = cat.trim();
            let name = name.trim();
            if !cat.is_empty() && !name.is_empty() {
                colors.push((cat.to_string(), name.to_string()));
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
    let path = state_path()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "未找到用户主目录"))?;

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
    // ⚠️ **保留本模块不认识的键**（GUI 的 `dark=` / `glass=` / `wrap=`，以及用户手写的键）。
    // 不这么做的话，每跑一次命令行就会把 GUI 里设的偏好抹掉 —— 见 `foreign_lines` 的说明。
    for line in foreign_lines(&path) {
        out.push_str(&line);
        out.push('\n');
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

    /// 建一个临时文件路径（测试结束由系统回收，不写用户主目录）
    fn tmp_path(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "hipercalc-state-test-{tag}-{}-{}.txt",
            std::process::id(),
            // 同一进程内不同用例用不同名字，避免互相覆盖
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        ));
        p
    }

    /// ⚠️ **本文件最重要的一条不变量**：GUI 写自己那几个键时，
    /// CLI 的键（变量表、颜色、模式…）必须**一个字都不能变**。
    ///
    /// 反过来说也一样（CLI 的 `save` 走 [`foreign_lines`]）。这条守不住，
    /// 症状就是"用一次命令行，GUI 的深色设置被抹掉；进一次 GUI，命令行的
    /// `/let` 变量全没了" —— 而且没人会想到是保存逻辑干的。
    #[test]
    fn update_keys_preserves_foreign_lines() {
        let path = tmp_path("preserve");
        std::fs::write(
            &path,
            "# 注释\n\
             display=mathio\n\
             var:A=E:1 / 2\n\
             color:number=cyan\n\
             dark=off\n",
        )
        .unwrap();

        // GUI 只改自己的三个键
        let prefs = GuiPrefs {
            dark: true,
            // ⚠️ 加新字段时**必须**在这里补一行：`cargo build` 不编测试代码，
            // 漏了只会在 CI 的 `--all-targets` 里炸（本项目已踩过三次）。
            capture: false,
            ui_scale: 0.9,
            glass: 0.25,
            glass_tint: 0.55,
            shadow: 0.4,
            wrap: false,
            texture: "glass".to_string(),
            glass_blur: 0.7,
            glass_tint2: 0.45,
            acrylic_dialog_tint: 0.8,
            glass_dialog_tint: 0.9,
        };
        update_keys_at(&path, &prefs.to_keys()).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        // ① 别人的东西原样还在
        assert!(text.contains("var:A=E:1 / 2"), "变量行必须保留：\n{text}");
        assert!(
            text.contains("color:number=cyan"),
            "颜色行必须保留：\n{text}"
        );
        assert!(
            text.contains("display=mathio"),
            "CLI 的模式行必须保留：\n{text}"
        );
        assert!(text.contains("# 注释"), "注释行必须保留：\n{text}");
        // ② 自己的三个键是新值，且**没有重复行**
        assert_eq!(
            text.matches("dark=").count(),
            1,
            "不该出现两条 dark：\n{text}"
        );
        assert!(text.contains("dark=on"));
        assert!(text.contains("glass=0.25"));
        assert!(text.contains("glass_tint=0.55"));
        assert!(text.contains("shadow=0.40"));
        assert!(text.contains("wrap=off"));
        // ③ 读回来与写进去的一致
        assert_eq!(GuiPrefs::load_at(&path), prefs);

        let _ = std::fs::remove_file(&path);
    }

    /// 文件不存在时能新建（首次启动的路径），且带一行注释头
    #[test]
    fn update_keys_creates_missing_file() {
        let path = tmp_path("create");
        assert!(!path.exists());
        let prefs = GuiPrefs::default();
        update_keys_at(&path, &prefs.to_keys()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with('#'), "新建文件应有注释头：\n{text}");
        assert_eq!(GuiPrefs::load_at(&path), prefs);
        let _ = std::fs::remove_file(&path);
    }

    /// CLI 的 `save()` 必须把**自己不认识的键**原样带回去（GUI 偏好靠它活命）
    #[test]
    fn save_keeps_gui_keys() {
        let path = tmp_path("save-keeps");
        std::fs::write(&path, "dark=off\nglass=0.10\nwrap=off\nvar:K=E:7\n").unwrap();

        let kept = foreign_lines(&path);
        assert!(
            kept.iter().any(|l| l == "dark=off"),
            "GUI 的 dark 键应被保留：{kept:?}"
        );
        assert!(
            kept.iter().any(|l| l == "glass=0.10"),
            "GUI 的 glass 键应被保留：{kept:?}"
        );
        assert!(
            kept.iter().any(|l| l == "wrap=off"),
            "GUI 的 wrap 键应被保留：{kept:?}"
        );
        // 自己的键不该被当成"外来行"重复带一遍（否则文件会越写越长）
        assert!(
            !kept.iter().any(|l| l.starts_with("var:")),
            "变量行属于本模块，不该被带回来：{kept:?}"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// 缺键 / 坏值时退回默认（状态文件是可以手改的，不能因为一个错字就崩）
    #[test]
    fn gui_prefs_tolerate_bad_input() {
        let path = tmp_path("bad");
        std::fs::write(&path, "glass=abc\ndark=maybe\n").unwrap();
        let p = GuiPrefs::load_at(&path);
        assert_eq!(p.glass, GuiPrefs::default().glass, "glass 坏值应退回默认");
        assert!(p.dark, "dark 只要不是 off 就算开");
        let _ = std::fs::remove_file(&path);
    }

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
