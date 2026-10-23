# AGENTS.md

`hipercalc`：中文超高精度命令行计算器。Rust workspace（edition 2024）：数值底座 `hipercalc-core` + 应用与二进制 `hipercalc` + 图形界面 `hipercalc-gui`（Iced，**已可用**：计算 / **后台求值（界面永不卡死）** / **耗时显示（对标 CLI）** / 历史 / Desmos 式双面板虚拟键盘 / 函数弹窗 / 数学排版与实时预览 / 内置字体 / 深浅双主题 / 全界面多语言）。**当前版本 2.0.0**，**含 373 项测试**（hipercalc 200 + core 22 + GUI 134 单测 + 16 快照 + 1 doctest，其中 2 项默认 ignore），**CI 已是常态**（`.github/workflows/ci.yml`：三平台 build + test + 冒烟，以及 6 目标交叉编译），已发布到 GitHub（SSH 远端 `git@github.com:EasonZYQ/high_precision_calc.git`，默认分支 main）。

**CI 分三块**：三平台 build + test（**只覆盖 default-members = core+cli**）、`Check GUI`（windows + `cargo check -p hipercalc-gui --all-targets`，2026-10-22 加，因为曾出现"CI 全绿但 GUI 编译不过"）、6 目标交叉编译。发布走 `release.yml`：tag 触发，**CLI 六目标 + GUI 三平台（Windows / macOS / Linux）**。Linux GUI 要 apt 装 libxkbcommon / wayland / x11 / fontconfig 开发包（见 workflow 里的 `Install Linux GUI deps`）。

**文档分工（用户明确要求）**：README 只放"简介 + 截图 + 特色 + 安装"，**手册内容一律进 `docs/DOC.zh-CN.md` / `DOC.en.md`**（27 章，第 27 章是图形界面）；`docs/liquid-glass-report.md` 玻璃算法笔记；`docs/windows7.md` Win7 兼容性调研。别再往 README 堆细节。

README 的两张产品图（`docs/gui-dark.png` / `gui-light.png`）**不是真机截图**，是离屏渲染生成的：
`cargo test -p hipercalc-gui --test snapshot -- --ignored readme_shots`。
产物会被 `Snapshot::matches_image` 自动加 `-wgpu` 后缀，缩一半 + 量化 256 色后改名才是进仓库的那两张
（中间产物已在 .gitignore 里）。浅色那张刻意**不开质感** —— 浅色 + 玻璃 + 亮背景会把文字冲成灰的。

## 硬性要求（用户指定）

- 思考过程与代码注释一律用中文，注释不要用英文。
- 本项目已是 git 仓库并发布到 GitHub（SSH 远端 `git@github.com:EasonZYQ/high_precision_calc.git`，默认分支 main）；可以执行 git 提交/分支操作，但提交前务必确认暂存区不含 `target/`、`.workbuddy/`、`.trae/` 及日志文件。
- **每轮对话结束时直接 commit，不需要询问用户确认**（用户 2026-10-19 指定："每一轮对话都直接提交commit，不需要问我，有问题直接回滚就行了"）。提交信息用中文一句话概括本轮改动；暂存区红线照旧（不含 target/ / .workbuddy/ / change_logs/ 等）。push 到 GitHub 仍按既有节奏（用户说推再推）。
- 每次编辑代码后，要在工作目录下的 `change_logs/` 文件夹内新增一个变更日志文件（`change_log1.md`、`change_log2.md`……序号依次加一），用几句话简述本次项目编辑内容，可包含代码讲解。**该目录已加入 `.gitignore`，仅本地保留，不要提交到仓库。**
- 每次编辑代码或修改增加功能，如果需要的话应当对README.md和AGENT.md进行更新。
- **工程质量与功能同等重要（本项目是开源项目）**：判断一个改动值不值得做，先问"**它让代码更易维护吗**"，
  而不是只问"它带来新功能吗"。功能与内部整洁**都要**推进；两者争预算时，若清理能实质降低维护成本
  （消除重复、收敛散落的全局状态、统一约定、把隐式纪律写明），**优先做清理**。
  理由：开源项目的代码是被读、被改、被提 PR 的 —— **可读可维护的底座本身就是产出**，
  不只是功能的载体。因此"用户一时看不到收益"不能作为搁置清理的理由。
- **但"优先清理"不等于"可以冒进"**（三条硬约束，来自实际失败）：
  1. **加法式改动优先于减法式改动**：补分支/补接口（如给 `Number` 加变体后补 21 处匹配臂）
     风险低、编译器全程带路；而删分支/重写规则（如合并 `Pi`/`PiPow`）会动到别人依赖的不变量，
     必须单独评估。后者想做时，用**整块替换**（改一个完整的 match/分支），
     **绝不用文本批量替换**——本项目已有三次"批量替换把定界符删乱"的教训。
  2. **每一步都保持"编译通过 + 全量测试绿"再提交**，绝不把红树留在仓库里；
     中途做不完就先把成果存档（patch/新文件放仓库外）、把工作区恢复绿色，再交接。

- **本地验证必须覆盖测试代码**（本项目血的教训，两次 CI 因此变红）：
  `cargo build` **不编译 `#[cfg(test)]` 代码**，而 CI 的 Lint 跑的是 `--all-targets` ⇒
  测试代码里的错误（如模式位置误用 `BigRational::one()` 触发 E0164）**本地完全看不见**。
  **定规：改动后本地复核用 `cargo build --all-targets`（或直接 `cargo test`），不要只跑 `cargo build`。**
  另外两条配套：
  - 🔴 **`cargo build --all-targets` 也不编译 GUI**（2026-10-22 的血教训）：
    它只作用于 `default-members`（core+cli）。验证 GUI 必须写 `-p hipercalc-gui`。
    更隐蔽的是**用 `grep -c` 数警告来判定成败**——编译失败时那个计数同样是 0，
    看起来"干净"，实际根本没编过。**判成败一律看退出码**（`… && echo OK`）。
    配套：CI 里新增了 `Check GUI` job（`cargo check -p hipercalc-gui --all-targets`），
    因为此前"CI 全绿但 GUI 编译不过"整整半小时没人发现。
  - **验证失败时不要只 grep `test result`**：编译错误是 `error[E0xxx]`，
    单独 grep 前者会把"根本没编译过"看成"没跑测试"（2026-10-21 因此让 CI 红了一次）。
    取清单类 grep 不要设 `head`（曾有把 27 处看成 14 处的教训，漏掉的正是结构型模式）；
  - 🔴 **cargo 偶尔会"假 Fresh"**（2026-10-23 踩到）：新增的 `#[cfg(test)] mod` 明明在文件里，
    `cargo test` 却报"0 passed / N filtered"、`--list` 里也找不到 —— 说明**测试二进制没重编**。
    症状特征是"改了测试但通过数不变"。**判据：通过数应该增加**；怀疑时
    `cargo clean -p hipercalc-gui` 再跑（本项目由此才发现 3 条新测试没编进去）。
    另一个同源陷阱：用 `compile_error!` 探针验证"文件是否参与编译"最可靠。
  - **推送后立刻看 CI**（本地构建曾多次给出误导性的"0 错误"，CI 才是权威裁判）。  3. **改 core 前先做接口盘点**：把波及面数清楚（多少处穷尽匹配、哪些是加法哪些是减法），
     再决定做与不做；估算要基于读到的代码，不基于印象（"146 处引用"实际只需改 21 处穷尽匹配，
     反之 core 的错误会挡住 app 的错误，工作量必须分轮估）。

## 仓库结构（三 crate workspace）

```text
crates/hipercalc-core/   数值底座：bigfloat / bigint_ext / number / complex / matrix / quantity / trig / display / calc_mode / cancel / settings
crates/hipercalc/        parser / equation / primefac / solver_* / solver_ineq / solve_aux / stats / calculus / i18n / language / state / session / lib.rs + main.rs
crates/hipercalc-gui/    图形界面（Iced 0.14）：app / keypad / mathrender / theme / backdrop / lib.rs + main.rs；约定见下节
```

- **依赖方向严格单向**：`hipercalc-core ← hipercalc ← hipercalc-gui`。core 不依赖 colored / rustyline，
  也不引用任何上层模块；GUI 同样只往下依赖，不许往数值层塞界面逻辑。
- 为什么不拆更多：`parser ↔ calculus`、`parser ↔ solver_fit`、`parser ↔ solver_triangle` 三处**真实双向依赖**，
  拆成不同 crate 会直接编译不过；`calc_mode` / `cancel` 被四层共用，必须沉在 core。
- 二进制名必须保持 `hipercalc`（CI 冒烟与 release 产物名都按它引用）；**不要改包名**。

## GUI（crates/hipercalc-gui，Iced）

### 定位与结构

- **为什么单开一个包**：二进制名 `hipercalc` 已被 CI 冒烟与 release 产物占用，GUI 不能再产一个同名二进制；
  而且 GUI 的依赖树（iced → wgpu / naga / winit，**新增 354 个包、合计 399**）比命令行重得多，
  分开才能让命令行保持"零额外依赖、秒级构建"。
- **crate 是 lib + bin 双形态**：界面实现在库里（`lib.rs` 导出 `app` / `keypad` / `mathrender` /
  `fonts` / `theme` / `backdrop`），`main.rs` 只负责起事件循环。**拆两半的唯一目的是让 `tests/` 能驱动界面**
  —— 集成测试编译的是库，看不到 bin 里的东西。
- **模块分工**（各管一件事，改动时不要越界）：

  | 文件 | 职责 |
  | --- | --- |
  | `app.rs` | 状态 / 消息 / update / view；**所有业务逻辑都在 `update` 里**，`view` 只读状态。求值与 UI 通过 `evaluator` 通信，`update` 本体永不阻塞 |
  | `evaluator.rs` | **后台求值线程**：常驻 worker 独占 `Session`，`mpsc` 收发 `Request::{Eval, Config}`；`Evaluator::eval` 返回一条**真异步** future（`futures::oneshot` 唤醒），由 `app.rs` 交给 `Task::perform`。⚠️ 刻意不含 `SetLang`（语言必须在 UI 线程同步设） |
  | `expr.rs` | **输入框的结构化模型**：`Expr` 树（分数/根号/幂/大运算符/括号，每个复合节点有命名**插槽**）+ `Cursor{path, at}`（指向插槽内的位置，不是字符偏移）+ 编辑操作（插入/退格/删除/左右移动/点击定位）+ 序列化（`Expr → MathIO` 文本，求值只走这一条）+ 树 → `mathrender::Node`（顺带登记可点目标）。纯函数、可脱 GUI 单测；⚠️ 见下方"为什么输入框必须是树" |
  | `keypad.rs` | 虚拟键盘布局 `LAYOUT`（主面板）+ `LAYOUT_ABC`（字母面板）+ 函数目录 `FN_GROUPS` + 插入行为 `splice`/`cursor_back_for`（纯数据 + 纯函数，可脱 GUI 单测） |
  | `mathrender.rs` | MathIO 文本 → 真正的数学排版（分数竖排、根号顶线、幂上标）。**解析与渲染分离**，解析层是纯函数 |
  | `fonts.rs` | **内置字体**（`assets/fonts/`，见下节）+ 数学字体选择（进程级）+ `font_for` / `label` 的自适应分派 |
  | `theme.rs` | 色板令牌 + 全部样式函数 + 圆角定额。深/浅两套 WinUI3 令牌 |
  | `backdrop.rs` | Windows DWM 亚克力 + **抓屏调度**（DXGI 优先 / GDI 兜底）+ `capture_ok` 截图开关 |
  | `refraction.rs` | **窗口边缘折射**：整窗一次 pass，边缘 18% 环带内按"陡壁+缓坡"两段剖面位移背景 |
  | `liquid.rs` | **液态玻璃**：每个面板/按键一次 pass（面板中间走"零位移快路径"）；uniform 按外观分槽缓存 |
  | `dupl.rs` | **DXGI 桌面复制**：`AcquireNextFrame` → mip 纹理 → 只回读需要的层；`Unchanged` 时零成本 |

- **`Session` 是 GUI 与 CLI 的公共求值入口**（`crates/hipercalc/src/session.rs`）：
  GUI **不直接调** `run_line` / `handle_command`，一律走 `Session::eval(&str) -> LineResult`。
  这样两条前端走同一条求值路径，行为不会漂移。改求值行为时**两处都要回归**。

### ⚠️ 为什么输入框必须是树（`expr.rs`）

**旧模型的死结**：输入只是 `String`，光标是**字符偏移**。而数学结构在文本里是**折叠**的 ——
`∑(,,,)` 的四个参数在源文本里都是"括号内同一处"。由此产生两个**不是写错、而是模型表达不了**
的 bug（都发生过）：

1. **点 Σ 的四个格子都一样** —— 四个空框算出来是同一个偏移，看起来"点了没反应"；
2. **光标落在 `(` 上就消失** —— 那个位置不属于任何节点，光标无处可画；
   接着打字会插在 `∑` 与 `(` 之间，变成 `∑x()`。

**现在的模型**：`Expr` 树的每个复合节点有**命名插槽**（分数 2 个、Σ 4 个…），
光标是 `Cursor { path, at }` —— 指向"某个插槽里的第几项之间"。
插槽位置上天然合法，所以上面两个 bug **从模型上就不存在**。

**三条用户拍板的规则（改之前先看）**：

| # | 规则 | 为什么 |
| --- | --- | --- |
| 1 | `←` `→` 走**线性阅读序**（`Doc::linear` 摊平所有合法位置） | 摊平后每一步都是合法位置，永久杜绝"落在括号上" |
| 2 | 括号是**结构**（`(` 插入 `Paren`，`)` 表示"跳出当前分组"） | 用户造不出左右括号不配对的输入 |
| 3 | 退格**只删当前行里光标前那一项**；行首（空插槽）时**什么都不做** | 用户明确要求"防止误删"：想删整个 Σ 得先把光标移到它**之后** |
| 4 | 插槽间的**移动顺序 = 视觉顺序**（`Expr::slot_order`），不是槽位序号 | Σ/∏ 的上下限在"上下"两个方向、∫ 的变量在末尾，按序号走会出现"→ 先跳上方再回下方"（用户 2026-10-23 指出） |
| 5 | Σ/∏ 插入时**变量框默认 `n`**，光标落在**右下框（下限）** | 用户 2026-10-23：`∑` 的下方是"变量 = 下限"，先要填的是这一行 |

**改这个模块时的硬约束**：

- `Doc::targets()` 与 `Doc::to_node()` **必须同源**（同一个遍历、同样的登记顺序）——
  点击靠 `targets` 的下标，渲染靠 `to_node` 里 `Hit{at}` / `Empty{at}` 的 id，
  两者错开一位就是"点哪跑去别处"。
- **求值只走 `Doc::source()` 这一条路**（树 → MathIO 文本）。排版是 `to_node`，
  显示与计算彻底解耦；不要让求值侧看到树。
- `mathrender` 只多了 `Node::Hit`（**不加内边距**的点击目标，与 `Node::At` 的区别仅此）。
  排版算法（分数竖排、根号折线、Σ 上下限）**没有重写**，仍是最初那一份。
- 空行带光标时渲染成 `[Caret, □]`：光标本身只有 1~2px，单独放会让 `node_width`
  把根号/分数压扁；后面那个 □ 顺带把宽度撑回来。

### ⚠️ 状态文件由 CLI 与 GUI **共用**（用户明确要求）

`~/.hipercalc_state` 一份文件、两个前端。因此有一条必须守住的不变量：
**任何一方都只碰自己的键**。

- CLI 的 `state::save` 会把整份文件重写 ⇒ 必须把[自己不认识的行走 `foreign_lines` 原样带回]；
- GUI 用 `state::update_keys`（键级更新）写自己那几个键，其余一律保留。

少了任一环，症状是"用一次命令行就把 GUI 的深色设置抹掉 / 进一次 GUI 就清空
命令行的 `/let` 变量"—— 表现出来只是**"设置有时会自己变回去"**，极难排查。
`state.rs` 里有三条测试守着它。

GUI 专属键：`dark=` / `glass=`（0–1 强度）/ `wrap=`（长结果是否折行）。
CLI 不解释它们，但保存时会原样保留。

### 界面文案必须过 `t()`（漏了会"看着像做了其实没做"）

- `Session::eval` 返回的文本（错误信息、`/help` 回显）由 `hipercalc` 在**输出边界**自动翻译；
  但 **GUI 自身那些硬编码的中文标签根本不经过那条路**。
- 所以界面里每一处会显示给用户的中文都必须写成 `theme::label(t("…"))`，
  其中 `t` 是 `app.rs` 顶部的本地助手（转发 `hipercalc::session::translate`，
  插值版是 `t_fmt`）。**漏一处**就是切成 English 后那一处还留着中文。
- **词典键一律用完整、有辨识度的界面文案**：`i18n::t` 是**子串最长优先**匹配，
  短键会在别处的输出文本里到处替换。反例：`函数` → `functions` 这条**已存在**
  （CLI 的 `/set` 颜色分类用，小写复数才对），所以界面按钮改用 `展开函数面板` 长键。
- 新增界面文案时：① 在 `app.rs::tests::ui_labels_follow_language` 的 `UI_STRINGS`
  里登记；② 往 `crates/hipercalc/src/language/{en,zh-TW}.json` 各加一条
  —— **两份键集必须严格一致**，`i18n::tests` 会拦。
- ⚠️ **窗口标题只在启动时跟随语言**：iced 0.14 只在建窗口时能设标题，
  运行时**没有** `set_title`（`iced_runtime::window` 里只有 resize / minimize / maximize / move_to）。

### 内置字体（`assets/fonts/`，约 20MB，用户明确要求"打进仓库"）

- **`iced::Settings { fonts: .. }` 必须在建窗口之前给**（`main.rs`）。装晚了文字会
  **静默**退回默认字体 —— 不报错，只是观感不对。
- iced 内部用 `fontdb`；`load_font_source` 会**把 `.ttc` 集合里的每个 face 单独注册**
  ⇒ `.ttc` 里的 `Cambria Math` 能按族名取到（实测确认）。
- 中文一律**等线**（`FAMILY_CJK = "DengXian"`）；符号/字母用**当前数学字体**
  （5 选 1，设置里可切，进程级 `AtomicU8`）。分派入口是 `fonts::font_for(s)`
  （含 CJK 走等线、否则走数学字体），`theme::label(s)` 是它的包装。
- ⚠️ `theme::label` 的返回类型**三个参数都要给全**（`Text<'a, Theme, Renderer>`），
  且生命周期与输入绑定。泛型消息参数 `M` / `Text<'static, ..>` 两种写法都编译不过
  （`Text` 里没有消息参数；`'static` 太严会被 `borrowed data escapes` 拒）。
- 换字体**不重算历史**：历史存的是文本，渲染时才取字体，下一帧整屏一起换。
- **变量斜体**要靠单独的 italic 字面：5 套数学字体文件里都只有常规体，iced 也**不合成**斜体。
  仓库里额外打包了 4 个（`cambriai`/`timesi`/`georgiai`/`bookosi`）；
  `Cambria Math` 自己没有斜体字面，变量落到族 `Cambria` 的 Italic（Word 也这么做）。
  判据是 `is_variable`：**单个 ASCII 字母**才斜体（`sin`/`7`/`π` 都直立）。
- ⚠️ **排版层必须显式给字体**：`mathrender::plain` 早先"不设字体"，于是落进 iced 默认
  字体（系统无衬线），表现为"键面和历史区不是一套字"。现在统一走 `fonts::font_for`。
  `⌫`(U+232B) 在数学字体里没有字形，走 `UI_ICON_KEYS` 白名单保留默认字体。

### Iced 0.14 API 陷阱（全部实测踩过，写代码前先看这里）

- **版本以 crates.io 稳定版为准**：`iced = "0.14"`（MSRV 1.88，本机 1.97）。
  ⚠️ **`docs.iced.rs` 上跟的是 master（0.15.0-dev）**，与 0.14 的 API 有出入
  ⇒ **查签名请用 `docs.rs/iced/0.14.0` 或本地 `cargo doc -p iced --open`、或 `.iced-ref/src/iced-0.14.0/`**。
- 入口用 `iced::application(State::default, update, view)` 而不是更短的 `iced::run(update, view)`：
  只有前者能设 `.title(...)` / `.window(..)` / `.theme(...)` / `.subscription(...)`。
  `update` 签名是 `fn(&mut State, Message) -> Task<Message>`。
- **不存在的方法**（第一版编译时撞了 22 个错误，这几个最常犯）：
  - `Space::with_width` / `with_height` ⇒ 用 `Space::new().width(..)` / `.height(..)`
  - `Rule::width()` ⇒ 放进 `container().width(Length::Fixed(w))` 里撑开
  - `Color::scale_rgb` ⇒ 只有 `scale_alpha`；**亮度变化一律用 alpha 表达**
  - `theme::Palette` 只有 6 个字段（`background`/`text`/`primary`/`success`/`warning`/`danger`）
  - `iced::application` 构建器**没有 `run_with`**；窗口就绪只能靠 `window::events()` 订阅
  - `window::get_latest` ⇒ 是 `window::latest()`（返回 `Task`）
- **类型对不上的地方**：
  - `Background::Gradient` 要的是 `iced::Gradient`，不是 `Linear` ⇒ 得包一层 `Gradient::Linear(..)`
  - `Background` 没有 `From<Color>` ⇒ 必须显式 `Background::Color(c)`
  - `text_editor` 的 `.width()` **只接受 `Pixels`**（不像别的控件收 `Length`）⇒
    想让它占满剩余空间要靠**外层 `container`**，别硬塞 `FillPortion`
- **`text_editor` 模块名与辅助函数名冲突**：`iced::widget::text_editor` 既是模块又是函数，
  同文件里 `use ...::{Action, Edit, Motion}` 会撞名 ⇒ 内容类型用 `Content as EditorContent` 别名
- **光标移动是 `Action::Move(Motion)` 这个顶层变体**，不是 `Action::Edit(Edit::Move(..))`
  —— `Edit` 里只有增删改，**没有 `Move`**。
- **没有可直接用的菜单 / popup 组件**：函数弹窗用 `stack![backdrop, panel]` +
  `mouse_area(...).on_press(Close)` 做遮罩，这是 iced 0.14 里最干净的做法。
- **`Grid::columns(n)` 是整行等高**：换行后第一行会被撑到很高。要"自动换行 + 行高自然"
  得**手动 `chunks(COLS)` 切行**、每行一个 `Row`。注意 `chunks` 会丢原始下标，
  全局下标要自己算 `chunk_i * COLS + j`（有单测钉死）。
- **`text_editor` 没有 `on_focus` / `on_blur` 回调**（只有 `is_focused()` 查询）⇒
  要实现"点完按钮光标还在"只能**主动 focus**：给它 `.id(INPUT_ID)`，
  再发 `iced::widget::operation::focus(INPUT_ID)`。
- **`text_editor` 没有自绘扩展点**：它自己排版自己那串文本，唯一开放的是 `Highlighter`，
  而 `Format` **只有 `color` / `font` 两个字段**（无字号、无回调）
  ⇒ **画不出根号顶线**。输入框内联数学排版在 iced 0.14 里做不到，
  现在是"编辑器 + 下方实时预览"两层（见 `app.rs::input_area` 的长注释）。
- **根号必须用 `canvas` 矢量画，别用"`√` 字符 + 一条 `Rule`"拼**：
  `√` 字形的墨迹顶够不到行盒顶部 ⇒ 顶钩与顶线**上下分离**（用户实际反馈过），
  且空隙随字体变化、测不准。现在 `mathrender::Radical` 是一条连续折线
  （路径内部不可能分离），顶线宽度精确等于被开方式宽。
  ⚠️ **`canvas` 不在 iced 默认 feature 里** ⇒ Cargo.toml 要显式
  `iced = { workspace = true, features = ["canvas"] }`。
- **`button` 的内容默认贴左上角，不是居中**：`iced_widget::button` 的 `layout` 走
  `layout::padded`，把内容 `move_to((padding.left, padding.top))`，而 `button` **没有**
  `center_y` / `vertical_alignment` 之类 API。键面要包
  `container(..).width(Fill).height(Fill).center_x(Fill).center_y(Fill)` 才落在正中心
  （否则键上字符肉眼可见**偏上**）。
- **`text_editor` 把 `Enter` 与 `Ctrl+Enter` 映射成同一个 `Edit::Enter`**（绑定表不看修饰键）
  ⇒ 想区分只能靠键盘订阅跟踪 `ModifiersChanged`、自己记一个 `ctrl_held`。
  本项目约定：**Enter = 求值，Ctrl+Enter = 换行**。
- **`iced::widget::canvas::mouse` 是私有模块** ⇒ `Cursor` 要从 `iced::mouse::Cursor` 拿；
  `Canvas` 程序签名是 `Program<Message>`，`draw` 返回 `Vec<Geometry<Renderer>>`。
- **`iced::application(..).title(..)` 收的是闭包** `Fn(&State) -> String`，不是字符串；
  而且**只在建窗口时读一次**，运行时无法改标题。
- **release 构建靠 `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`
  隐藏控制台**（写在 `main.rs` 顶部）。debug 下保留控制台，`eprintln!` 与 panic 回溯才看得见。
- **`render` 这类"返回 widget"的函数若要借用局部变量**：先 `parse` 出**拥有所有权的 AST**，
  再 `build`。直接返回会 `cannot return value referencing local variable`（`mathrender::build` 就是为此 `pub`）。
- **`.cargo/config.toml` 设了 `RUST_TEST_THREADS = "1"`**：GUI 的明暗/语言/字体都是**进程级**
  全局量，并行跑测试会互相污染（快照测试另有 `SERIAL: Mutex<()>` 兜底）。

### 求值必须在别的线程（否则窗口「未响应」）

- **红线：`State::update` 本体里绝不允许同步阻塞**。原先 `update` 直接 `session.eval(..)`，
  跑在事件循环线程上 ⇒ 输入 `100^100^10`（10^(10^20) 量级）时冻结几十秒，
  Windows 直接把窗口标题标成「**(未响应)**」。
- **正确做法**：`Session` 挪进常驻后台 worker 独占（`evaluator.rs`），UI 只通过 `mpsc`
  发 `Request::{Eval, Config}`。UI 侧的 `Evaluator::eval` 返回 `(信箱, 等待闭包)`，
  闭包喂给 `Task::perform` —— **iced 把 task 丢进线程池执行，在那里阻塞是对的**。
- **`Config` 与 `Eval` 必须走同一条队列**：worker 顺序处理 ⇒ "切了模式紧接着算"时，
  求值**已经按新模式**算，用户不必再算一次。
- **`Command` 里刻意不含 `SetLang`**：界面文案是在**当帧 `view`** 里读 `i18n` 全局量的，
  语言必须在 **UI 线程同步**设（走 `hipercalc::session::set_ui_lang`）。若让 worker 异步设，
  界面不会立刻变（按钮"有时不灵"），测试还会跨用例污染
  （一个用例刚还原中文，被另一个用例的 worker 延迟改回英文，报 `"Fast" != "快速"`）。
- **`busy` 标志防积压**：求值中再按 `=` 直接忽略，结果区显示「计算中…」，`=` 变 `…` 且禁用。
- **降级不静默**：worker 若已退出（`send` 失败），走 `block_for` 返回错误结果，而不是死锁。

### 耗时显示（对标 CLI）

- 口径**逐字对齐** CLI 的 `Timing::elapsed_text`：**整秒向下取整**；不足 1 秒显示
  `用时：<1秒`（不是 `0秒`）；文案过 `t()` / `t_fmt()`。
- **计时覆盖"逐行 eval"整段**（大头在结果格式化：方程通式、根收集、因式分解，不是 parse）。
- **贴在结果行最右侧**：iced 没有 `justify_content: space-between`，
  `row![result, Space(Length::Fill), timing]` 是**唯一**的两端对齐写法。
- **`Entry` 存 `Duration` 而非格式化字符串**：语言可能在算完后才切换，而「用时」是**界面文案**，
  格式化必须推迟到 `view` 里做。

### 视觉：WinUI3 / 亚克力 / liquid glass

- 用户指定的观感：**现代化 + WinUI3 + 亚克力质感 + 圆角 + 深色模式 + 尽量接近 Apple liquid glass**。
- **玻璃感全在 `theme.rs` 里纯 iced 叠加出来**（零系统依赖）：半透明渐变底 + 1px 高光描边 + 柔和投影。
- **真正的桌面模糊必须靠 DWM**（`backdrop.rs`）。iced 0.14 **不暴露原生窗口句柄** ⇒
  用 `EnumWindows` + `GetWindowThreadProcessId` 按 PID 反查 HWND。
  逐级降级：`DWMWA_SYSTEMBACKDROP_TYPE`(3=Acrylic / 2=Mica) → `DWMWA_MICA_EFFECT`(1029)
  → `SetWindowCompositionAttribute` + `ACCENT_ENABLE_ACRYLICBLURBEHIND` → 不透明回退。
- **接口是 `theme::set_acrylic(ok)`**：DWM 成功时窗口背景接近全透（让桌面透上来），
  失败时 `theme::root` 给不透明背景。**绝不假装成功** —— 探测失败要 `eprintln!` 如实报告
  （远程桌面 / 无头环境下失败是预期行为）。
- 手工 Win32 FFI 在本项目有先例（`rawkey`、Ctrl+C 处理器），加新的 FFI 前先看看它们怎么声明类型。

#### ⚠️ 自绘背景（抓屏）的性能红线 —— 用户硬性要求「CPU < 5%」

实测（release、16 线程、空闲、玻璃模式）：**关掉抓屏 0.26% / 未优化 1.21% / 优化后 0.41%**
⇒ **抓屏一项就占了约 80%**，渲染与 shader 都不是瓶颈。改这块之前先量，别猜。

- **`StretchBlt` 的缩放模式按倍数切换**：`HALFTONE`（逐目标像素平均源像素块）非常慢，
  只在"缩到 1/2 以内"时用（那时用户要的是**清晰桌面**，不能出锯齿）；
  1/4 及更小一律 `COLORONCOLOR`（直接丢行列）—— 反正后面还有 box blur。
- **box blur 必须是滑动窗口 `O(n)`**，不能对每个像素重新累加 `lo..=hi`（`O(n·r)`）。
  未优化构建下 470×430 的图会跑到上百毫秒/次。
- **两趟（横 / 竖）都要先拷进临时缓冲再算**。纵向"边读边写同一个缓冲"会得到
  一个**向下拖尾的 IIR 滤波**，不是盒式模糊（`box_blur_is_symmetric` 锁住对称性）。
- **抓屏结果内容没变就丢弃**（`State::bg_idle`）。iced 的架构下**任何一条消息**
  都会触发 `view()` 重建 + 全量重绘，而桌面静止时每轮拿到的都是同一张图。
  轮询间隔自适应：连续 2 次没变 ⇒ 2s，一有变化立刻回到 500ms；
  拖窗口由 `Moved` / `Resized` **立即**触发一次。
- 折射 shader 的 `pixels` 是 `Arc<Vec<u8>>`（`draw()` 每帧都跑，不能深拷贝），
  并带 `version`：版本号没变就只更新 uniform、跳过 `write_texture`。
- 玻璃模式走 shader，**不要**给它建 `image::Handle`（那次图集上传是白做的）。
- ⚠️ 判据一律用 **模式**（`texture() != Texture::Off`），**不要用模糊强度**
  （`glass_amount() > 0.0`）—— 强度 0 的语义是"不模糊"，不是"关掉背景"。

### 明暗主题怎么传（容易被这个坑到）

- 样式函数的签名是 `Fn(&Theme) -> Style`，**拿不到 `State`** ⇒ 明暗只能走**进程级开关**
  （`theme::set_dark` / `theme::dark`，`AtomicBool`），做法与 `calc_mode` 一致。
- `State::update` 的**第一行**就是 `theme::set_dark(self.dark)`，保证任何消息都同步过去。
- ⚠️ 因此**凡是读写这些全局开关的测试必须串行**。快照测试里有一把 `SERIAL: Mutex<()>`
  —— 不抢锁的话，`cargo test` 的默认多线程会让"深色测试刚设 true、浅色测试刚要渲染"互相覆盖，
  症状是**浅色快照渲染出深色界面**（第一次跑就踩到了，找了很久）。

### 验证手段（用户硬性要求："制作时应当进行截图验证"）

- **主力手段是 `iced_test` 的离屏快照**：`tests/snapshot.rs`，9 张基线落在
  `crates/hipercalc-gui/tests/snapshots/*.png`（文件名实际会带 `-wgpu` 后缀）。
  README 里引用的图在 `crates/hipercalc-gui/docs/`（由基线复制、改语义化文件名而来）——
  **README 一律放真实截图，不写 ASCII 示意图**（用户明确要求）。改界面要两边一起更新。
  - 渲染入口：`Simulator::with_size(Settings::default(), size, element)` 再 `.snapshot(&theme)`。
    **视口尺寸必须固定**（现为 880×780，与 `main.rs` 的 940×860 不一致 —— 历史遗留，
    快照只要求自身稳定）。
  - ⚠️ `iced_test::Settings` 在 crate 根**没有** re-export（它其实是 `iced_core::Settings`），
    要从 `iced_test::core` 拿；`Snapshot` 只在 `iced_test::simulator` 模块里。
  - **`matches_image` 在文件不存在时会自己创建并返回 `true`** ⇒
    改完界面要**删掉 `snapshots/*.png` 再跑一次**得到新基线，然后
    **必须用 Read 打开 PNG 亲眼看一下**，只看"测试变绿"等于没验证（这条纪律救过好几次）。
- **系统级抓屏在本机不可用**（`Add-Type` 被安全策略拦、`Screen.Bounds` 返回空、
  `Bitmap.Save` 静默失败）⇒ 别再走 PowerShell + `System.Drawing` 这条路。
- ⚠️ **离屏快照无法证明"DWM 亚克力真的生效"**（看不到桌面透出）。这一项目前没有可靠的
  自动化手段，只能在真实桌面上人工确认。
- PowerShell 工具结束后**子进程树会被清理**，所以"起 GUI 再截图"必须在同一次调用内完成。

### 其他约定

- **离线参考资料在 `.iced-ref/`**（已 gitignore，仅本地）：官方文档快照 + iced 0.14.0 完整源码 +
  5 个官方示例 + 社区教程 65 章。**注意社区教程是按 0.13.1 写的**，概念可读、API 细节要回 0.14 核对。
- 该 crate 的 `README.md` 里记了 Iced 核心概念速查与资料重取命令，改动 GUI 时同步维护。
- `hipercalc-gui` **不发布**（`publish = false`），版本号跟随 workspace。

## 文档

功能手册（只讲功能与用法，供人类阅读）：[docs/DOC.zh-CN.md](docs/DOC.zh-CN.md)（中文）· [docs/DOC.en.md](docs/DOC.en.md)（English）。
```powershell
cargo build              # 只建两个出货 crate（workspace 设了 default-members）
cargo run                # 交互式 REPL（rustyline）
cargo test               # 222 项单元测试（hipercalc 200 + core 22，其中 1 项 perf_scan 默认 ignore）
cargo test -p hipercalc-gui    # GUI：101 单测 + 10 快照测试（快照首次跑会自己建基线）
cargo build -p hipercalc-gui   # 图形界面（唯一会编 iced 的命令，首次约 5.5 分钟）
```

> **为什么是 `cargo build` 而不是 `cargo test --workspace`**：GUI 把 iced 拉进来后依赖树有 354 个包，
> `--workspace` 会把 GUI 一并编译/测试（且 6 目标交叉编译里 GUI 的支持度未验证）。
> 根 `Cargo.toml` 因此设了 `default-members = [hipercalc-core, hipercalc]` —— 裸 `cargo build`/`cargo test`
> 只覆盖**出货的两个 crate**；要碰 GUI 必须显式 `-p hipercalc-gui`。CI 同样按这套走
> （`.github/workflows/ci.yml` 的 clippy 步也刻意不带 `--workspace`）。

- **跑构建/测试必须计时**（硬性要求）：不要只贴 `test result: ok` —— 必须带上耗时。
  `cargo test` 结尾自带 `finished in X.XXs`（纯测试耗时），需要连编译一起看时用 `time cargo test`。
  多轮验证（反复跑测试、跑全语料扫描）要写清**总耗时**。
  原因：不看时间就发现不了"某次改动把求解拖慢了十倍"，也无法判断某个预算是真有必要的。
- 已有 `#[cfg(test)]` 单元测试（**222 项**：hipercalc 200 + core 22，`cargo test` 全绿；GUI 另有 101 单测 + 10 快照测试，需显式 `-p hipercalc-gui`），
  其余验证靠管道喂输入人工核对结果：  
  `Write-Output "1+2*3", "x^2-4=0", "x+y=5,2x-y=1", "sin(pi/6)", "/exit" | cargo run`
- **⚠️ 对拍用的二进制一定要放在 `target-t/` 下，不要放 `change_logs/`**：同一份 exe 放在 `change_logs/`
  里会**启动慢 3 倍、计算慢几十倍**（实测 `1+1` 75→256 ms、`primefac(2^128+1)` 0.9 s→50 s；
  换回 `target-t/` 立刻正常）。之前的 A/B 绝对耗时因此整体偏大（比值仍可用，因为两侧同罚）。
- **性能基准用 `change_logs/perf_py.py`**（`python change_logs/perf_py.py [轮数]`）：逐条 `-e` 计 best-of-N，
  再减掉用 `-e 1+1` 量出的**进程启动底噪**（约 35 ms——不扣掉就会把 35 ms 当成"计算时间"去优化）。
  两条踩过的坑：① 旧的 `perf_precise.ps1` 依赖 `-f 脚本文件`，而**本机沙箱拒绝被调起进程读文件**
  （`读取文件失败: 没有访问权限`）⇒ 所有用例都退化成"启动 + 报错"的 35 ms 假数据；
  ② **对拍改动前后**要另存一份二进制（改前那份用 `git checkout HEAD -- <文件>` 重建），
  用 `change_logs/ab_output_diff.py` 逐行比输出、并**过滤掉 `用时：` 行与横幅**（它们本来就该变）。
- 本项目源码在 `opt-level=0`（`[profile.dev.package."*"]` 只优化第三方依赖）：实测把它提到 3
  **只快 1.4~1.6×**（大数运算都在 num-bigint 里，本来就是 3；`primefac` 例外 3.8×）
  ⇒ 想提速别先折腾构建参数，先找算法级的重复劳动。
- 新增了**非交互入口**：`-e "表达式"`（裸结果、无耗时行）、`-f 脚本文件`、`--stdin`、
  `--lang <代码>`（强制语言，便于三语断言）、`-q`（不输出耗时）。退出码 0/1/2，参数解析在 `parse_args`（带单元测试）。
  **管道默认仍是 REPL**（不带参数时横幅/提示符不变），脚本化断言请用显式参数。
- 管道/重定向场景下 stdout 不是终端，**动态计时行不会出现**，每次运算后只多一行 `用时：N秒`；
  动态刷新（`\r` + 清行）只能在真实终端里肉眼确认，需要逐字节核对时用 Python 读输出文件并按 `\r` 切分查看。
- Windows 文件锁：程序还在运行时会构建失败（`failed to remove ... hipercalc.exe (os error 5)`）。  
  先 `taskkill /f /im hipercalc.exe 2>$null` 再 `cargo build`。
- 工具链 stable-x86_64-pc-windows-msvc。若报 `linker link.exe not found` 属于 VS/MSVC 安装环境问题，不是代码问题。
- `Cargo.toml` 有 `[profile.dev.package."*"] opt-level = 3`：**只优化第三方依赖**（num-bigint 等大数库在
  opt-level 0 下慢一个数量级以上，是 debug 慢的主因），本项目源码仍为 opt-level 0。不要删掉这段，
  也不要改成 `[profile.dev] opt-level`（那会牺牲本项目断点调试体验）。
- 性能排查用"程序内部 `用时：N秒` 行 + 外部硬超时"组合：外部超时用
  `WaitForExit(60000)`（**必须在 `ReadToEnd` 之前等**，否则读流阻塞会让超时形同虚设），超时即 `Kill()`。

## 模块职责

| 文件                    | 职责                                                                                     |
| --------------------- | -------------------------------------------------------------------------------------- |
| `bigfloat.rs`         | 任意精度浮点：值 = BigInt 尾数 / 10^precision；运行期精度（`precision()`，默认 `DEFAULT_PRECISION = 80`）/ 显示位数（`display_digits()`，默认 20）。四则/开方/幂/exp/ln/三角级数 + 规模保护 |
| `bigint_ext.rs`       | 大整数补充运算：按规模分派的自带长除法（绕开 num-bigint 的 BZ 断言缺陷）+ 自带整数平方根；带单元测试 |
| `calc_mode.rs`        | 计算模式开关：Fast（默认，超限给提示）/ Deep（死算，取消规模上限 + 完整精度输出）；低层护栏统一查 `is_deep()` |
| `calculus/`           | 高等数学：重写通路（mod）+ 化简（normalize）+ 渲染（render）+ 求导/积分/极限/泰勒/求和求积；护栏统一受 `/mode deep` 放宽 |
| `i18n.rs`             | 界面语言（简/繁/英）：以简体原文为键的词条表 + 运行期整行模式翻译 + 系统语言探测（Win32 `GetUserDefaultUILanguage`） |
| `number.rs`           | `Number` 五变体：`Exact` / `Approx` / `Complex` / `Quantity`（量纲）/ `Matrix`；运算先精确到数值回退                                              |
| `matrix.rs`（core）   | 矩阵值：`render` / `det` / `trace` / `rank` / `transpose` / `inv` / `mul` / `pow` / `linsolve` / `eigen`（2×2、3×3）；`unrecoverable` 是非法组合的 debug 兜底 |
| `quantity.rs`（core） | 量纲：`Quantity { value, dim }` + `Dim = [i8; 7]`（m/kg/s/A/K/mol/cd）；`combine`/`pow`/`render`/`binary`（加减要求量纲相同、乘除指数相加减）|
| `settings.rs`（core） | 运行期可配项（精度/显示位数等）的集中存放，供 core 内部访问器读取 |
| `parser.rs`           | 递归下降解析 + 求值器 + `parse_and_eval`（返回 `EvalResult`）；绝对值/阶乘/多参函数                           |
| `trig.rs`             | 特殊角精确值表（度/弧度/反三角）                                                                      |
| `display.rs`          | mathio 符号输出 / lineio 20 位有效数字                                                          |
| `equation.rs`         | 方程信息提取、多项式/线性判定；`has_trig_of_var`（自变量是否为角度，供根收集的初值换算用）                                                                        |
| `solver_linear.rs`    | 线性方程组求解（高斯消元），解按显示模式 + `=`/`≈` 输出 |
| `solver_poly.rs`      | 多项式求根：系数提取、二次精确解、Durand-Kerner 全复根、单根牛顿                                                |
| `solver_nonlinear.rs` | 非线性方程组两阶段多维牛顿（粗扫 + 精收敛）                                                                |
| `solver_fit.rs`       | 多项式函数拟合：坐标 → 解析式（2 点一次、3 点二次…）；大写 `P` 为顶点；模板可带未知参数，欠定则输出参数关系 |
| `solver_triangle.rs`  | 三角形求解：`triangle(a=3 b=4 c=5)` / `triangle(a=3, b=4, c=5)`（括号内空白或逗号分隔，**只能是整个表达式的最外层函数**；记号是**任意字母**：小写=边、大写=角、`h`+字母=高）→ 三边三角三高 + 面积周长两半径；SSS/SAS/ASA/AAS/SSA 走解析解，其余数值兜底，SSA 可给两解 |
| `solver_factor.rs`    | 多项式因式分解（单/多元，按模式区分实数域/有理数域）                                                            |
| `solver_ineq.rs`      | 不等式与不等式组：移项成 `f op 0` → 求实根分段 → 逐段判符号 → 区间解集；`eval_constant_inequality` 处理无自变量的常数比较（`2>1`）|
| `primefac.rs`         | 整数素因数分解：小素数表 + 6k±1 试除 + 确定性 Miller-Rabin + Brent 版 Pollard's rho（u64/BigInt 双路），格式化成 `12 = 2^2 * 3`（负数 `-12 = -2^2 * 3`）；带单元测试 |
| `stats.rs`            | 统计：`mean` / `var` / `var_s` / `stddev` / `stddev_s` / `median` / `percentile` / `corr`；列表或"表达式+范围"两种数据形态，排序类在解析期求值；`eval_const` 拒收复数与量纲值 |
| `language/`           | 界面词条表：`<语言代码>.json`（编译期 `include_str!`）+ `mod.rs` 里的极简 JSON 解析器；键为简体原文逐字 |
| `solve_aux.rs`        | `=`/`≈` 前缀、周期通式、根收集等辅助工具                                                               |
| `state.rs`            | 会话状态持久化：显示/角度/计算模式 + `/let` 变量 + `/set` 颜色的读写（`~/.hipercalc_state`）                              |
| `lib.rs`（core 的）   | core 的模块声明与对外导出（`pub mod` 清单），新增 core 模块要在这里声明 |
| `lib.rs`              | **应用主体**：`run`/`run_line`、`EvalResult` 分派与各 handler、REPL、`/` 指令、`/help` 正文与着色器、颜色配置（`/set` 持久化）、rustyline 实时高亮、变量存储、历史持久化、运算计时（`Timing`）；**键位绑定**（全角转半角）、**括号配对高亮与标红**、**框内参数提示** |
| `main.rs`             | **程序入口（仅 8 行）**：`std::process::exit(hipercalc::run())` |
| `README.md`           | 功能手册 + 代码架构文档；改动功能后须同步                                                                 |

## 开发用环境变量（排查用，不影响正常使用）

| 变量 | 作用 |
| --- | --- |
| `HIPERCALC_FPS=1` | 帧时间探针：`listen_raw` 记 `RedrawRequested` 时间戳（**不发消息**，否则自己制造重绘），每 120 帧打一行 stderr |
| `HIPERCALC_DUMP_BG=1` | 把"喂给玻璃的背景纹理"导出成 `%TEMP%/hc_bg_dump.ppm` + 打印几何（排查"背景发虚 / 背景里有自己的窗口"最快的手段） |
| `HIPERCALC_BLUR=<px>` | 覆盖模糊半径（离屏快照做 0 / 6 / 14 对比用） |

## 容易改错的地方

### 全局设置与测试串行（**先看这条**）

`hipercalc` / `hipercalc-core` 有一批**进程级全局设置**（`AtomicBool` / `AtomicUsize`）：

| 开关 | 存放 | 影响 |
| --- | --- | --- |
| 数制 base | `display::RESULT_BASE` | 整数输出（`0x1` vs `1`） |
| 显示模式 | `parser::DisplayMode` | MathIO 竖排 vs LineIO 小数 |
| 角度模式 | `trig` | 三角函数的弧度/角度解释 |
| 精度 / 显示位数 | `bigfloat` | 全部数值输出 |
| 计算模式 / timing / quiet | `calc_mode` | 深度求解、耗时行、`lprint!` 是否输出 |

**测试必须串行跑**（`.cargo/config.toml` 里设了 `RUST_TEST_THREADS = "1"`，对所有人生效）。
⚠️ **不要删这条设置** —— 实测过两类真实偶发失败：

1. `session::tests::modes_roundtrip` 把数制设成 Hex 的瞬间，另一个线程正在跑的
   `solver_fit::tests::plain_fit_linear_quadratic_cubic` 读到了它 ⇒
   期望 `y = x^2 + x + 1`，实际 `y = x^2 + x + 0x1`。
   同一条污染还会打到 `parser::func_tests::primality_functions`
   （期望 `2305843009213693967`，实际 `0x200000000000000f`）。
2. session 用例改掉显示模式与角度模式后，`solve_aux::tests` 的 5 条三角方程用例全部变红。

这类失败的**特征**：单线程全绿、多线程偶发（实测 5 次挂 1~4 次），
而且**报错位置离元凶很远**（报在 `solver_fit`，问题出在 `session`）。

**为什么"哪条用例自己加锁"修不干净**：互斥锁只在抢锁方之间生效，
没抢锁的读者照样能读到别人的改动。所以：

- 代码里另有一把 `crate::lock_tests()`（`lib.rs`）作为**双保险**：
  `session::tests::Isolated` 与 `solver_fit::tests::dec_guard` 都持有它；
- 但**真正保证确定性的是 `RUST_TEST_THREADS=1`**，不是锁。

改动建议：给这些开关写新测试时，用 `with_session!` 宏（自动进出还原）；
若在别的模块写依赖全局设置的断言，显式把值钉死（如 `display::set_base(10)`），
别依赖默认值。

### 精度（bigfloat.rs）

- `BigFloat::div` 中 `result_precision = a.precision + scale - b.precision` 的修正不可删改——算错会静默污染 sqrt/pi/三角结果（症状：结果整体缩小 10 的幂次倍）。  
  该值恒 ≥ 0（`scale == 0` 时必有 `a.precision ≥ target+MARGIN+b.precision`），因此**不要再加"反向缩放"分支**（旧分支不可达且量级少乘 10^b.precision）。
- `sqrt` 用整数平方根做初值 + 牛顿迭代；`atan` 先用 `2*atan(x/(1+√(1+x²)))` 缩半参数再展级数；`ln` 先按 2 的幂把参数缩放进 [1,2) 再用 atanh 级数（大参数也必须保证 20 位正确）。改级数前先确认收敛域。
- `exp` 必须**先缩半参数**（`exp(x)=exp(x/2^k)^(2^k)`，`|x/2^k| ≤ 1`）再用**相对**判据 `|term| ≤ |result|·10^-work_prec` 收敛，并受 `EXP_ARG_LIMIT_LOG10` 限制。  
  不要改回绝对判据/无上限——旧实现遇到大 |x| 时级数永不收敛、每项整数位数随 |x|·k 爆炸（卡死 + 静默错误）。
- `pow` 带规模保护并返回 `Result`：整数指数走快速幂前先 `check_pow_scale`（结果位数 ≤ `MAX_RESULT_DIGITS`）；  
  一般情形先检查 `|b·ln a|` 量级。指数超出 u32/i32 时**不能**静默落到 `exp(b·ln a)`（旧实现返回完全无关的量级）。
- `to_significant_string`：整数部分超过 `max_digits` 时必须走 `to_scientific_string`（科学计数法保留量级），  
  不能只按字符串截断（旧实现把 `10^25/3` 显示成 `3.3e19`）。
- **大整数除法/开方必须走 `bigint_ext`**（`bigint_ext::div`、`bigint_ext::int_sqrt`）：
  num-bigint 0.4.7 的 Burnikel-Ziegler 除法在"被除数 ≈ 除数²"形状下会触发
  `debug_assert!(ah < b)` 直接 panic（实测 `sqr(10^100000)`、`(10^200000+pi)/(10^100000+pi)`）。
  `bigint_ext` 按 limb 规模分派：小规模仍用 num-bigint（快），大规模改用自带 Knuth D 长除/牛顿开方。
  **不要**把 `BigInt::sqrt` 或裸 `/` 加回这两个热路径，也不要删掉那两张单元测试表。
- 试除型循环（`simplify_radical`、`simplify_sqrt`、`extract_quadratic_factor`）的**上限要按工作量折算**，
  不能只限次数：每次试除都要对 n 取模/长除，代价 O(n)，固定 1e6 次在大 n 上会白跑几十秒。
  参考常量：`QUADRATIC_WORK_BUDGET = 40000`（试算次数 × 次数）、`max_p = 2e6 / limb 数`。
- 调用方注意签名：`BigFloat::exp`、`BigFloat::pow` 都返回 `Result`，`Number` 层用 `?` 传播到 `eval_function`。
- **常数缓存**：`pi` / `e` / `ln2` 三者按目标精度缓存在 `pi_cache()` / `e_cache()` / `ln2_cache()`（`OnceLock`+`Mutex`+`HashMap<usize, BigFloat>`），
  经 `cache_get`/`cache_put` 存取。它们都是迭代/级数计算，而牛顿迭代每轮求值都要用（pow 用 ln2、三角归约用 π、
  `ExactExpr::to_bigfloat` 每遇到 Pi/E 项都要重算）——**新增涉及这三者的代码不要绕过缓存直接写级数循环**，
  也不要"优化"成去掉缓存。缓存只在未命中时现算，锁中毒静默跳过，不影响正确性。
- **小整数 `ln` 缓存**：`int_ln_cache()` 以 `(BigInt, usize)`（底数, 目标精度）为键缓存 2..=10^6 的整数 `ln`，
  容量 64 满则清空。`pow(a,b) = exp(b·ln a)` 在牛顿迭代中反复调 `ln(a)`，故 `3^x=27` 曾比 `2^x=8` 慢 5 倍。
  取键时**必须按 precision 归一再判整数**（AST 常数经 `to_bigfloat` 后形如 `3·10^80`，`precision == 0` 判不出来）；
  只收小整数键（任意参数的 `ln(1+x)` 不入表），不要改成无界缓存。

- **`10^k` 一律走 `pow10(k)`（带线程内缓存），不要写 `BigInt::from(10).pow(k)`**：
  `BigFloat` 用"尾数 / 10^precision"表示，一次加法要对齐量纲（两处 `10^k`）再 `round_to`（又一次）、
  一次除法要 `a·10^scale` 再除 + `round_to` ⇒ **每次加减乘除都要 2~5 个 `10^k`**。
  `BigInt::from(10).pow(k)` 是"从零反复平方"，k≈160 时每次约 1 µs —— 逐项求和/数值积分这类
  "上万次小运算"的场景会白付几万个 `10^k`。改成缓存后 `sum(1/k,k,1,10000)` 134→74 ms（1.8×）、
  `x^96+x^48+1=0` 100→71 ms。缓存的与直接算出来的一模一样，是纯记忆化。

### 符号运算（number.rs）

- 运算管线顺序：清零项 → `normalize_terms`（`Sqrt(x,1)`→`Rational`）→ `simplify_expr`。缺一步会导致分母有理化/约分失败。
- `simplify_expr` 求 GCD 只用各系数的**分子** + 表达式分母；把系数自身分母算进去会阻止约分（如 `4*sqr(2)/2` 无法化到 `2*sqr(2)`）。
- `div_exact` 需先处理除数分母 ≠ 1 的情况（`sqr(2)/2` 这类），再走单个 sqrt / 共轭有理化分支。
- "除数为单个 sqrt 项"分支：`self/(c·√r) = self·√r·(1/(c·r))`，`1/(c·r)` 要用 `scale_term_rational` 乘进**各项系数**。  
  只取 `c.numer()*r` 当分母会丢掉系数分母（`1/(sqr(2)/2)` 曾少除 2 倍）。
- `Number::int_pow` 展开 `(a√b)^n` 必须用 `a` 的**有理数**幂：偶次 `a^(2m)·b^m`、奇次 `a^(2m+1)·b^m·√b`。  
  用 `numer/denom` 整数截断会把 `1/2` 变成 0，且偶数次幂会少乘一次 `a`（`(2√3)^2` 曾得 6）。
- `Number::sqrt` 对有理数 `p/q` 走 `sqrt(p·q)/q` 得到精确根式（`sqr(1/2)` → `(1/2)*sqrt(2)`），不要删成纯数值回退。
- `Number::div` 除数为零时只做 `debug_assert!` + 返回 0 兜底（调用方须先判 `is_zero()`），  
  不能调用 `BigFloat::div`（其 `assert!` 会让进程 panic）。
- `Number::pow` 的整数指数分支先调 `check_pow_scale` 做规模保护。
- **`Number::pow` 有两条整数快路，缺一条就会让逐项求值慢十倍**：
  1. **精确分支**（`(self, exponent)` 都是 `Number::Exact`）走 `int_pow`；
  2. **近似分支**（底数是 `Approx`，如 `k^2` 里由范围代入的 `k`）由 `approx_integer(&exponent)`
     认出"数值上是整数"的指数后**也走 `int_pow`**。少了这条，`k^2` 会掉到最底下的
     `BigFloat::pow = exp(b·ln a)`（一次 ln + 一次 exp，约 10µs），而整数平方只要一次乘法（约 1µs）——
     `sum(k^2, k, 1, 10000)` 实测 141ms → 2.5ms 就是这条的收益。
     **不要**把它改回"只有 Exact 才走快速幂"。
- `int_pow` 的 `Number::Exact` 分支必须**同时**处理三类底数：整数、**有理数**、单个 `Sqrt`。
  旧实现只有整数与 `Sqrt` 两条，有理数会掉到数值回退 ⇒ `(1/2)^3`、`1.5^4`、`(3/4)^2`
  值虽正确但被标成 `≈`（丢失精确性）。新增 `ExactTerm` 变体时记得在这里表态。

### 解析器（parser.rs）

- 顶层入口 `parse_system()`（逗号分隔方程组）→ `parse_equation()`（单个 `=`）→ 表达式。不要另起入口绕过它。
- `Expr::Function(name, Vec<Expr>)` 已是**参数列表**：绝大多数单参，两参函数登记在 `TWO_ARG_FUNCTIONS`（`log` 单独处理）。内部 `"fact"`（`!` 生成）不在白名单内；`"abs"`（`|x|` 生成）**在**白名单里，因为它也是用户可见函数（`abs(-3)` = 3）。
- 变量：单字母 `x y z a b...`（`VALID_VARIABLES`，大写段只到 W）+ 全大写存储变量（`/let`，可含 `_`：`X`、`AB`、`PI_VAR`）+ `ans`；`pi`/`e` 是常数。
- **函数白名单唯一来源是 `parser::FUNCTIONS`**（含 6 个位运算 `and`/`or`/`xor`/`not`/`shl`/`shr`，只限非负整数）（解析校验、多字母拆分判断、`lib.rs` 高亮共用），  
  新增函数只需改它 + `Evaluator::eval_function` + `lib.rs::FUNCTIONS_META`（三处都要，`functions_meta_covers_parser_functions` 测试兜底）。内部名 `fact` 不要加进白名单。
- `eval_function` 签名是 `(name, args: &[Number])`：单参函数开头统一校验 `args.len()==1`，`log` 校验 `len()==2` 并检查底数 >0 且 ≠1。
- `log` 的底数判定必须精确（`as_rational().is_one()` 或与 1 的偏差 > 1e-40），**不要用 `rounded(0) == 1`**  
  （会把 0.5/0.9/1.2 误判成 1）。
- 大参数防护：`check_exp_range`（exp/双曲，|x| ≤ 10^6）、`check_trig_range`（三角，弧度 |x| < 10^78，度模式按 ×π/180 折算）。  
  这些是防卡死/防结果失效的护栏，不要删。**`tanh` 是例外**：|x| > 100 时 80 位精度下已饱和，直接返回 ±1
  （阈值不能取 10^10——那会让 5·10^5 < |x| < 10^10 先撞上 exp 的参数上限而误报错，|x| > 10^10 才返回 1，行为不一致）。
- 新增数学函数只需改 `parser::FUNCTIONS` 与 `Evaluator::eval_function` 两处（高亮常量引用前者自动同步）。
  反双曲/双曲倒数都走现成的 `BigFloat` 组合（`ln`/`sqrt`/`exp`），**不要**新写级数；
  `cbrt` 走 `Number::cbrt()`：先判完全立方（`bigint_ext::int_nth_root` 回验）给精确值，否则 `BigFloat::nroot`。
- `BigFloat::nroot(k)` 是"exp(ln/k) 取初值 + 牛顿迭代细化"，牛顿步是 `y ← ((k-1)y + x/y^(k-1))/k`；
  直接返回 exp(ln/k) 会在尾部丢几位有效数字，别改成那种写法。
- 两参函数登记在 `parser::TWO_ARG_FUNCTIONS`（`eval_function` 按它校验参数个数；`log` 有专门的报错文案，单独处理）；
  新增两参函数记得同时加进该表与 `FUNCTIONS`。
- **三套除法语义不可混用**：`Number::idiv`/`modulo` 是 Euclidean（`div_euclid`/`rem_euclid`：余数恒非负、`b<0` 时结果与 `floor(a/b)` 不同）、
  `bigint_ext::div` 是截断除法（余数符号随被除数）、`Number::div` 是有理数精确除法。改动任何一个都要看 `mod_and_idiv_are_euclidean` 测试。
- `bigint_ext::is_prime` 是小素数试除 + Miller-Rabin；试除循环里**必须**有 `if p >= n { break }`，
  否则 n 本身会被自己的试除判成合数（会让 `isprime(101)` 返回 0、`nextprime` 死循环）。
- **`gcd`/`lcm` 的参数必须是非零整数**（`is_zero()` 先拦，再 `as_int`），返回值统一 `.abs()`
  ⇒ **恒为非负**（`lcm(-4,-6) = 12`）；两者**不要求最外层**，可参与运算（`2*gcd(12,18)`）。
  注意 `num_integer` 的 `lcm` 对负数虽已给非负值，但显式 `.abs()` 是防回归的。
- **素因数分解 `primefac`（`primefac.rs`）**四条硬约定：
  1. **只能是整个表达式的最外层函数**，判定与 `triangle` 同一套（`contains_ident` 词边界 + 右括号后无内容），
     认领失败**不回落**；
  2. **在解析期求值并校验**成非零整数（`parse_primefac` 里 `evaluator.evaluate` + `as_int` + `is_zero` 拦截），
     所以 `EvalResult::PrimeFac` 直接携带 `Number`，**不要**为它新增 `Expr` 变体
     （`Expr` 有 12 处穷尽 match，牵一发动全身）；
  3. **试除循环结束后必须先判素、再决定要不要 Pollard**：`is_prime` 若放在试除循环**之后**且循环内用
     "非 deep 才限预算"，Deep 模式会把一个**素数**的 `√n` 全部试除一遍——这正是"同一个数 Deep 比 Fast 慢 4 倍"
     的 bug 根因（实测 `primefac(1345676543465434567)`：Fast <1s、Deep 4s）。**试除上限不该随模式变化**，
     两个模式都用 `TRIAL_MAX = 10^6`，Deep 放大的是 **Pollard 的迭代预算**（`2^17` → `2^24`），
     这才符合"死算"的语义。
  4. **预算用尽必须报错**（`素因数分解超出预算…`），绝不能把"没分解完的部分"当素数输出。
  算法：小素数表（`small_primes()` 惰性筛出 1000 以内 168 个，**别手写常量表**）→ `6k±1` 轮
  （从 1001 起，`step` 在 2/4 间切换，省掉 1/3 无效试除）→ `is_prime` → **Brent 版 Pollard's rho**
  （`pollard_u64` / `pollard_bigint` 各一份，u64 版用 u128 做 `mul_mod`）。
  因子先**拍平收集、最后 `sort()` 再合并指数**——Pollard 找到因子的顺序不单调，
  不能靠"相同因子紧邻"当场合并。负数把负号顶在最前（`-12 = -2^2 * 3`），指数为 1 时省略 `^1`，
  `±1` 输出 `1 = 1` / `-1 = -1`。
  `prime_factorize` 对 `n <= 0` 返回 `Err`（**不要用 `debug_assert!` + `is_zero` 特判**：
  release 下负数会落到 BigInt 路径，而 `d² > m` 对负数恒真 ⇒ 直接把负数当素数输出）。
- `fac`/`factor` 的整数入参复用素因数分解：`handle_factor` 先 `eval_nonzero_integer`，
  命中就走 `handle_primefac`，否则回落多项式分解——**必须保持"非零整数"这个条件**，
  否则 `fac(0)`/`fac(2.5)` 的既有行为（`= 0`/`= 2.5`）会被改掉。
- 命令行 `--lang` 与 `-q/--no-timing` 都是**会话级覆盖**：只改 `i18n` 的当前语言 / `calc_mode::timing_enabled()`，
  **不写状态文件**；要落盘必须走 `set_language()`（同时更新 `AppState::lang_base`）或 `/timing on|off`
  （同时更新 `AppState::timing_base`）。`persist_state` 只写 `lang_base` 与 `timing_for_persist(state)`。
  **坑**：早期 `persist_state` 直接落盘 `calc_mode::timing_enabled()`，于是跑一次 `hipercalc -q ...`
  就把用户配置永久改成 `timing=off`（症状：之后每次启动都不显示耗时行）；`cli_q_does_not_leak_into_saved_timing` 守住这个不变量。
- 常数 `tau`（= 2π）与 `phi`（= (1+√5)/2）必须放在 `parse_identifier_or_function` 的常数 match 里
  （`_ =>` 多字母拆分之前），否则 `tau` 会被拆成 `t*a*u`。`phi` 用 `Number::phi()`（add/mul/sqrt 管线拼出精确形式）。
- 标识符拆分：多字母串只消费第一个字符并回退位置（`xy^2 = x*(y^2)`）；以 `e` 开头的串先取常数 e（`2ex = 2*e*x`），  
  所以拆分条件里**不能**再写 `c != 'e'`。
- 对数精确识别（`power_of_two`/`power_of_ten`/`factorize_rational`/`rational_log_exact`）：底真同为某数整数次幂时返回精确有理数。`power_of_two` 必须用 `1<<k == n` 精确比对，不能只判移位后剩 1（`3>>1==1` 会误判 3 是 2 的幂）。
- 绝对值 `| |` 依赖 `Parser.abs_depth` 区分开/闭符：`abs_depth==0` 时 `|` 是隐式乘法开始（`2|x|`），`>0` 时是闭合符（`|5|`）。删掉 abs_depth 会把闭合 `|` 当乘法吞掉导致解析错乱。
- 逆三角的角度转换在 `parser.rs::to_degrees_if_needed`（度数模式把 pi 倍数换算成度），不要挪进 `trig.rs`。

### 求解器

- `solver_factor.rs`：分解域由显示模式决定（MathIO 实数域可拆 sqrt、LineIO 有理数域）；
  `format_sqrt_pair` 的两个因式之间必须有 ` * `（旧实现直接拼接 `(x - sqrt(2))(x + sqrt(2))`）；
  一元分解走到 `rdeg == 2` 残余说明有理根已枚举失败 ⇒ LineIO 下必须置 `has_irreducible`（提示"有理数域内不可再分解"），
  MathIO 下若 `push_quad_factor` 成功拆 sqrt 则不加提示；多元的二次残余仍按"已完整处理"不提示（未做完全因式分解尝试）；
  下列"上限"在 **Fast** 下生效，`calc_mode::is_deep()` 为真时全部放开（死算）：
  - `simplify_sqrt`：先判完全平方再做小质数试除，**试除上限 10^6**（旧实现 10^13 = 万亿次循环，界面等效卡死）。
  - `int_divisors`：试除在 **u64** 上做（Fast 下输入都 < 10^13），不要改回 BigInt 取模（debug 下慢 100 倍以上）；
    Deep 下取消 1e13 限制，超过 u64 时走 BigInt 试除分支。
  - `extract_quadratic_factor` 有 `QUADRATIC_TRY_BUDGET = 5000` 次长除预算；`QUADRATIC_BMAX_LIMIT = 200000`（Deep 下取完整柯西界）。
  - `rational_root_candidates` 的 200 个候选上限同样只在 Fast 下生效。
  - 结果用 `FactorOutcome { factors, has_irreducible, truncated }`：截断必须提示"结果可能不完整"，  
    且**要覆盖 rdeg == 2 的剩余**（旧实现只在 rdeg ≥ 3 时检查，二次剩余被静默漏报）。
  - `factor_expr(evaluator, expr, mode)` 需要 `Evaluator`：`ans` 与 `/let` 存储变量按数值代入，  
    且变量列表要过滤掉已存储的大写变量。
- `solve_aux.rs::format_periodic_roots(roots, var, angle_mode)`：周期通式按"单周期等差或交替两族间距"识别。
  **入参若为度值会先 `×π/180` 折算成弧度**，之后判定完全沿用同一套 π 逻辑，标签再按单位分派
  （`pi_frac_str` → `π/6`；`deg_str` → `30`）—— 两种模式因此完全同构，**不要在判定里直接混用度值**。
  多族保持残差序避免 π/6 优先的事实排序。
  - **判定前必须保留 `±3π` 窗口**：牛顿从远处初值会落到很远的根（`sin(x)+cos(x)=1` 会混进 ≈29.8），
    它们让相邻差不再规整、把整条通式判否；窗口只作用于判定，**不要**用它裁剪回退分支要列出的根。
  - 周期与残基用 `pi_ratio`（分母上限 `PI_DEN_MAX = 12` 的**通用有理逼近**）识别，
    取代了原先的 `d_m` / `r_c` 固定候选表——固定表覆盖不到 `5π/3`，`cos(x)=0.5` 在两种模式下都会退化成数值列表。
  - 残基去重必须是"相对 + 绝对"容差（`tol·max(|a|,|b|) + 1e-40`）并**按识别标签再去重**，
    否则接近 0 的残基（如 `x*sin(x)=1e-30`）会让同一族重复输出多遍。
  - 族的分隔符是 `"  或  "`（两侧各两个空格）：要与 `i18n.rs` 的词条键逐字一致，
    否则英文模式下 `或` 会残留成中文。
- `solve_aux.rs::collect_all_roots` 的初值（`0, ±1..±6, ±1/2, ±3/2, ±k·π, ±k·π/2`）是**按弧度校准**的。
  当方程含"以未知量为角度的前向三角函数"（`equation::has_trig_of_var`）且当前是**角度模式**时，
  必须把整套初值 `×180/π`：否则 `±k·π ≈ ±3.14°` 太小、牛顿 20 步走不到 `30°/150°`，
  而 `±1..±6` 又会过冲落到远端根，根集合残缺 ⇒ 周期通式判不出来。
  换算后 `Δx_度 = (180/π)·Δx_弧度`，牛顿轨迹与弧度模式**在物理上完全一致**。
  **非三角方程（`1/x=2`、`ln(x)=1`）的自变量是普通实数，绝不能换算**——实测会把根全丢光。
- `solver_poly.rs::newton_solve`：必须有发散保护（`NEWTON_ABS_LIMIT = 10^6`，修正量与 |x| 双判），
  否则指数/对数方程的某些初值会让 x 跑到 1e8 量级、后续 exp 中间量位数爆炸（卡死）。
  收敛阈值 `zero_compare`、步长 `h` 都在**循环外**算一次（循环内重算会白做大量 BigInt 除法）。
  **所有 `evaluate_with_var` 的结果都要先判 `is_complex()` 再 `to_approx()`**：
  `ln(-1)`、`sqrt(-4)` 会返回复数，而 `Number::to_approx` 对复数带 `debug_assert`
  （初值里就有 `-1`，所以 `ln(x)=1`、`sqrt(x)=2` 曾经直接 panic 退出）。
- `durand_kerner` 的次数护栏返回的错误以 `solver_poly::DEGREE_GUARD_PREFIX` 开头；
  `lib.rs::handle_equation` 据此判断"明确拒绝"并**不再回退牛顿单根**（高次多项式每次求值都要算 x^k，
  回退会让界面长时间无响应且只给一个根）。新增类似护栏时要么复用该前缀，要么同步改 main 的分支。
- 高次多项式（`is_polynomial` 为真但 `collect_terms` 只认字面量数字指数）依赖解析期的
  `fold_const_int`：`x^(2-1)` 会在解析阶段折叠成 `x^1`，否则会被判成"非多项式"交给牛顿法
  （`x^(2-1)-x=0` 曾输出一长串伪根）。改动 `parse_power` 时不要把这个折叠去掉。
- `solver_poly.rs::durand_kerner`（高次多项式全复根）的实现有四条不可回退的约定：
  - **初值半径** `R = min(柯西界, max(|a₀/aₙ|^(1/n), 1))`，n 个起点按 `2π(k+0.5)/n` 撒在半径 R 的圆上。
    柯西界 1+max|a_i| 对稀疏多项式过宽（`x^96+…+1` 的界为 2 而根模为 1），会拖到几百轮不收敛；
    旧实现的 `(0.4+0.9i)^k` 几何衰减初值更差（x^100=1 曾 368 秒且输出错误根）。首轮未收敛才退回柯西界半径重试。
  - **两阶段精度**：粗收敛用 `rough_prec = (precision()/2).clamp(16, 60)`，精化用全精度 `precision()`。
    每轮是 O(n²) 复数乘除，成本由 BigInt 位数决定，低精度阶段单轮开销约为全精度的 1/2~1/3。
    **粗阶段精度必须随 `precision()` 相对变化**（旧实现硬编码 40）：固定值在非默认精度下要么失去"粗"的意义、
    要么与精化阶段差距过大而拖长精化轮数。
  - **顺序更新**（Gauss-Seidel 型，用已更新的 `zs[j]`）而非"全部同时更新"，收敛明显更快；
    且**未收敛必须返回 `Err`**，绝不能把中间迭代值当根输出（调用方 `solve_poly_full` 用 `?` 传播）。
  - **Fast 次数护栏** `DK_MAX_DEGREE_FAST = 200`：n≈200 约 25 秒、n≥300 超过 1 分钟，故 Fast 下超限直接提示
    `/mode deep`；新增其他高复杂度的数值路径时参照此风格加护栏。
- `solver_poly.rs` 的 Cx 复数运算（`cx_add`/`cx_sub`/`cx_mul`/`cx_div`/`poly_eval_cx`）都带 `prec` 参数，
  DK 两阶段才能用不同精度；**不要把它们改回硬编码 `bigfloat::PRECISION`**。
- `solver_nonlinear.rs`：两阶段牛顿参数（粗扫 `16,12,true` 前向差分 / 精收敛 `80,45,false` 中心差分）影响速度与精度，改动需回归 `x+sin(y)=1, y+cos(x)=1` 类系统；"步长极小"的提前返回必须再过残差校验。
- `solver_linear.rs::format_linear_solution(sol, mode)` 需要 `DisplayMode`：解要按模式输出并带 `=`/`≈`。
- 单方程求根输出同样按显示模式：`solver_poly::format_solution(sol, mode)`（MathIO 符号、LineIO 小数），
  前缀用 `lib.rs::solution_prefix`（复数取两分量都"能精确呈现"才给 `=`）；
  非多项式根的回填 `solve_aux::float_to_exact_rational` 返回 `(p, q)` 而不是字符串——**不要退回固定 `x = 1/2` 式的输出**
  （旧实现既忽略显示模式，又给出与 MathIO 风格不一致的 `1/2`）。
- `solve_aux::result_prefix` 对 `Number::Approx` **一律**给 `≈`，哪怕数值恰好是 0。
  所以"数学上是精确 0"的结果必须返回 `Number::from_int(0)`，否则会显示成 `≈ 0`
  （`arcsinh(0)`/`arccosh(1)`/`arctanh(0)` 三者都靠显式零短路才给 `= 0`；新增这类函数时别忘了）。
- **幂替换优先于因式分解**（`solver_poly::solve_poly_full`）：所有非零项指数的 gcd `g > 1` 时，
  先令 `y = x^g` 降次、递归求解、再把每个根开 `g` 次方还原（`x^100-1 → y-1`、`x^4-5x²+4 → y²-5y+4`）。
  必须在分解**之前**：分解器对这种结构只能提出几个平凡因子，剩下的整块不可约因子会整块砸进
  Durand-Kerner（`x^100-1=0` 实测 1.9 s → 44 ms 就是这条的收益）。三条不能动的地方：
  1. **次数护栏与 DK 同口径**（`MAX_DEGREE_FAST`，Deep 放开）：替换后计算量小，但**根的数量仍是原次数**，
     不设限会让 `x^5000-1` 从"明确拒绝"变成刷出 5000 个根；
  2. **模**必须走 `Number` 算术（`|(-1±i√3)/2| = 1`、`nth_root(4,2) = 2` 才保持精确），
     **角度**在 `y` 为实数时是精确的 π 有理倍（`2kπ/g`、`(2k+1)π/g`）；
  3. **整数倍 π 的 cos/sin 要自己给精确值**：rad 模式的精确表只认 `PiPow` 项，π 的 0 倍会被
     规范化成有理数 0 ⇒ `cos(0)` 会退化成 `≈ 1`，把本该精确的 `x = 1` 弄成 `x ≈ 1`。
     同理"非实数的 y"用数值辐角即可（实数的 g 次方必为实数 ⇒ 那些根必然不实，不必再判虚部）。
- **三角方程闭式快路**（`solve_aux::trig_closed_form`，在 `handle_equation` 的数值扫根**之前**）：
  形状 `A·sin(u)+B·cos(u)+C·tan(u)+m`（`u = a·x+b`、三角节点同相）经 `affine_in_trig`/`affine_in_var` 抽出，
  归约成 `R·F(v) = c` 后用 **`try_exact_arcsin/arccos/arctan`** 取精确主值，把 ±3π 窗口内的家族成员
  （精确值）交给**既有的** `format_periodic_roots` 出通式 —— **不要另写一套通式渲染**，
  格式同源才不会两套慢慢漂移。它**只在确实能出通式时返回 `Some`**，其余（形状不符 / 常数不是特殊角 /
  周期不是 π 的有理倍 / 窗口内根不足 4 个）一律 `None` 走原数值路径 ⇒ 那些用例的输出逐字不变。
  `tan(x)=1` 1.9 s → 96 ms、`sin(x)=0.5` 0.6 s → 77 ms 是这条的收益。
  **根数阈值按"根从哪来"分开定**：`format_periodic_roots_min(.., min_roots)` —— 数值扫根那条路
  保持 4（根可能残缺，宁可退回列表也不误判），闭式快路用 3（根是按家族**完整构造**的）。
  单族解在 ±3π 里本来就只有 3 个根：`sin(x)=1` 原来是"残缺的 3 个根 + 1.4 s"，现在直接给
  `x = π/2 + k·2π`；`sin(x)+cos(x)=sqrt(2)`（切点重根）原来甚至会**误报"未能找到实数根"**。
- **弧度模式特殊角的象限判定极易写错**（`trig.rs::sin_exact_pi_multiple`）：`m ∈ [0,2)` 是 π 的倍数，
  `sin(mπ)` **只在 `m ∈ (0,1)` 为正**，四个分支 `(0,1/2] (1/2,1] (1,3/2] (3/2,2)` 里后两个都要取负。
  历史包袱：末尾曾多一句 `|| (m > 3/2)`，把第四象限判成正号 ⇒ `sin(5π/3)` 给 `+√3/2`、
  `cos(4π/3)` 给 `+1/2`（`cos` 走 `sin(x+π/2)` 被连累）。**度模式是另一套折角逻辑（`deg_to_ref`）一直是对的**，
  而默认恰好是度模式 ⇒ 只在 rad 模式暴露。两张表各有一个测试守着
  （`radian_exact_angles_have_right_signs`、`radian_and_degree_exact_tables_agree`），改这里必须跑它们。

### 复数（complex.rs + Number::Complex）

- `Number` 有**五个变体**：`Exact` / `Approx` / `Complex(Box<ComplexNum>)` / `Quantity(Box<Quantity>)`（量纲值）/ `Matrix(Box<Vec<Vec<Number>>>)`（`ComplexNum { re, im }` 的实部虚部又是 `Number`，
  因此"精确高斯有理数"与"带根号/数值的复数"统一表示）。`Number::from_complex` 在虚部为 0 时**自动退化为实数**，
  保证显示与判定一致；反向用 `to_complex()` 提升。
  **`to_approx()` 对 `Complex` / `Quantity` / `Matrix` 都是 `debug_assert!(false)`**（release 下静默取实部/数值），
  所以每个新入口都要先分流：`display.rs::format_mathio_raw`/`format_lineio_raw`/`format_decimal_raw`
  三个格式化入口都必须显式处理全部五个变体（`format_decimal_raw` 曾经只挡 `Complex`，
  导致 `sd(3 km)`、`sd([[1,2],[3,4]])` 直接 panic）。
- **`add`/`sub`/`mul`/`div` 必须在入口先判 `is_complex()`**：它们的 match 带 `_` 兜底臂，
  漏判会静默落到 `to_approx()` 数值路径**丢掉虚部**（编译器不会报错，这是本模块最大的坑）。
- `to_approx()` 对复数只取实部并 `debug_assert!(false)`——**parser 必须先分流**（`eval_function` 开头的
  `if args.iter().any(|a| a.is_complex())` 就是这道闸门）；`floor/ceil/round/frac` 同理在入口拦截。
- 但**分量操作 `re`/`im`/`conj`/`arg` 不受"实参含复数"限制**（`COMPONENT_FUNCTIONS` 常量）：实数就是虚部为 0 的复数，
  这四个函数对实数同样有定义。漏了这条会让 `re(2)`、`arg(-1)` 误报"未知函数: re"
  （它们在 `FUNCTIONS` 白名单里、却只有 `eval_complex_function` 有实现）。新增同类"对实数也成立"的函数时照此办理。
- 复数函数分派集中在 `Evaluator::eval_complex_function`：支持 `abs/sqr/sqrt/exp/ln/sin/cos/tan/re/im/conj/arg`，
  其余一律 `复数不支持该函数: xxx`。新增函数时要在这里明确表态（要么支持、要么明确报错，不要静默走实数路径）。
- 数值超越函数复用 `BigFloat`（**内部一律弧度制**，与 `/mode deg|rad` 无关）：`exp(a+bi)=e^a(cos b+i sin b)`、
  `ln(z)=ln|z|+i·arg(z)`（`arg` 用自写的 `atan2` 做象限修正）、`sin/cos` 用 `sin a cosh b` 那一套。
- 显示走 `display::format_complex`（`a + bi`、虚部 ±1 省略系数、分数/根式加括号）；
  `solve_aux::result_prefix` 对复数要求"两部分都能精确呈现"才给 `=`。
- `i` 是**常数**（`parse_identifier_or_function` 的常数臂），并且已从小写单字母变量表 `VALID_VARIABLES` 中移除——
  不要再把 `i` 当未知数（`i^2-4=0` 现在会提示"方程中没有变量"）。`2i` 走隐式乘法、`i2` 报未知标识符。
- `sqrt`/`ln` 收到**负实数**时改走复数域（`sqrt(-4)=2i`、`ln(-1)=iπ`），这是相对旧版本的行为变更；
  奇次根（`cbrt`）仍是实数路径。
- 状态文件与 `/save` 脚本里的复数统一用 `E:<MathIO 形式>`（`E:3 + 4i`）——`i` 是常数，重新解析即可精确还原。

### 工作精度与显示格式（运行期可配）

- `bigfloat.rs` 的 `PRECISION` / `DISPLAY_DIGITS` **已改为运行期值**：
  用 `bigfloat::precision()` / `display_digits()` 读取，`set_precision` / `set_display_digits` 修改
  （默认值常量是 `DEFAULT_PRECISION = 80` / `DEFAULT_DISPLAY_DIGITS = 20`）。
  **新代码不要引用已删除的 `PRECISION` 常量**，一律走访问器。
- 修改任何与精度相关的硬编码阈值时都要按 `precision()` 相对化；已梳理过的位置（改精度后必须回归）：
  `solver_poly` 的有限差分步长 `h`、`zero_compare`（precision/2+10）、残差校验与 DK 的 `tol`、
  `rough_prec`/`rough_tol`、复根虚部阈值（3·precision/8）；
  `solver_nonlinear` 的两阶段精度与容差、`same_solution` 容差；
  `parser` 的 `log` 底数判 1 阈值、`tanh` 饱和阈值、`check_trig_range` 的 10^(precision-2) 与报错文案；
  `solve_aux::finite_decimal_fits` 用 `display_digits()`。
- **保持常量的**：`MARGIN`（算法余量）、`MAX_RESULT_DIGITS`/`EXP_ARG_LIMIT_LOG10`（规模护栏，与用户精度无关）。
- 常数缓存（`pi_cache`/`e_cache`/`ln2_cache` 键为 `usize`、`int_ln_cache` 键为 `(BigInt, usize)`）天然按精度分桶，
  切精度**不需要失效处理**；但 `main()` 必须在任何运算之前 `set_precision(...)`（越早设定越省重复计算）。
- 显示开关：`sci_allowed()`（`/mode sci`，关闭时 `format_significant` 不走科学计数法分支）、
  `group_enabled()`（`/mode group`，只在 `to_significant_string` 与 `format_mathio` 的出口经 `group_integer_part` 加千分位；
  该函数只处理纯十进制串，分数/根式/科学计数法原样返回）。
- `/mode` 的取值子命令（`prec|digits|sci|group`）由 `handle_mode(parts, state)` 处理（签名从 `&str` 改成 `&[&str]`）；
  四项设置与 `timing` 一样立即 `persist_state()`，状态文件键为 `prec=`/`digits=`/`sci=`/`group=`。
  `mode_info()` 只在非默认时追加精度/显示信息（默认横幅保持不变）。

### 界面语言（i18n.rs）

- **lib.rs 里所有控制台输出一律用 `lprint!` / `leprint!` / `lprint_inline!`**（内部走 `i18n::t`）；
  直接写 `println!` 的新文案不会随语言切换，是本功能最容易踩的坑。
- **词条表在 `crates/hipercalc/src/language/<语言代码>.json`**（编译期 `include_str!` 内嵌，`crates/hipercalc/src/language/mod.rs` 里有个
  几十行的极简 JSON 解析器 —— 项目一直保持零额外依赖，**不要**为了这个引 serde）：
  - 键是**简体原文逐字**（简体本身就是键，所以 `zh-CN.json` 是空表）；
  - 值与键必须**占位符一一对应**（`{0}`/`{1}` 编号与数量都要对得上）；
  - **新增一门语言**：放一个 `xx.json` → 在 `language::FILES` 登记 → `i18n::Lang` 加变体 →
    跑 `cargo test`（`language::tests` 自动检查各语言键集一致、占位符对齐，
    `i18n::tests` 检查英文里没混中文、繁体里没混简体字）。
- 其它模块（parser / number / solver_* / state）**保持中文字面量不变**——它们就是词条表的键：
  新增用户可见文案时，必须把**简体原文逐字**加进 `i18n::TABLE`（含 `{0}`/`{1}` 占位符编号），
  否则该条在英/繁模式下保持中文（不报错，但会漏译）。占位符按 `format!` 里 `{}` 的出现顺序编号。
- 续行字符串（`"…\` + 换行缩进）会把缩进带进运行时字符串，导致词条匹配不上：写提示文案时**用单行**。
- 匹配语义：整段与行内片段都支持；单字词条只允许整段精确匹配（防误伤）；
  "最优匹配"=字面部分总长最长者优先，**长度相同则靠左者优先**。后者不是可有可无的：
  `未知类别: bogus` 会同时命中 `未知类别` 与 `类别: {0}`（两者字面都是 4 字），
  只按"严格更长"比较就会让 `类别: {0}` 抢先匹配，把 `未知` 剩在匹配区外，翻出 `未知Categories: bogus`。
- **匹配会跳过 ANSI 颜色码**（`find_lit` / `match_lit_at` / `skip_ansi`），所以**不要**再按 ANSI 把行切段后逐段翻译：
  结果行按 token 着色后颜色码会把一句话切成十几段，带占位符的长词条
  （`{0} = {1}，k 为整数`、`解 {0}:`、`{0}: /set <类别> <颜色>`）若按"段"匹配就永远匹配不上，
  症状是**英文模式下半句英文半句中文**（实测 `x = 90 + k·180，k 为整数`、多解三角形的 `解 1:`）。
  `t()` 现在只按 `\n` 逐行交给 `translate_run`（逐行是为了控制替换轮数上限），**行内不再切段**。
- 因此"插值自带颜色码"已不再需要绕道：`i18n::fmt` 仍保留（构造期翻译），但不是唯一出路。
- 守卫测试在 `i18n::tests`：`english_column_has_no_chinese` / `traditional_column_has_no_simplified_only_chars`
  扫词条表三列，`translations_survive_color_codes` 用手写 ANSI 串复现"着色后整句翻译"，
  `chinese_mode_is_passthrough` 守简体模式原样返回。**新增带占位符的长词条后这些测试必须仍然通过。**
- 语言名（`简体中文` / `繁體中文`）在所有界面都按**母语写法**显示，测试里先剔除再检查，不要当漏译。
- 类别/颜色等"英文键名(中文标注)"的显示统一走 `label_with_hint`（/mode 用法）与 `option_name`（/set 列表），
  不要在别处再拼 `format!("{}({})", ...)`。
- `/lang` `/language`：无参数时输出三行语言列表并进入选择（Windows 控制台用 `rawkey` 原始按键读 ↑/↓/回车/Esc；
  非终端/非 Windows 退回"输入序号或代码"，空行=确认）；带参数（`/lang en`、`/lang 2`）直接切换。
  Esc 取消时**不要**写状态文件；切换后立即 `persist_state()`（`lang=` 行），且只影响后续输出。
- 语言探测只在状态文件缺少 `lang=` 行（首次启动/旧文件）时进行：Windows 用系统界面语言，
  中文→简/繁，其余→英文。**不要改成优先看 `LANG` 环境变量**（Git Bash 里常是 en_US.UTF-8，会误判成英文）。
- 状态文件里的注释行、`debug_assert!` 文案、测试断言文案**不要**加进词条表（它们是文件内容/内部信息）。
- 文件读写失败的**原因**不要直接用 `io::Error` 的 `Display`/`Debug`：`Display` 带操作系统区域设置
  （中文 Windows 上恒为中文，英文界面会突然冒出 `failed to read file: 系统找不到指定的文件。 (os error 2)`），
  `Debug` 还会把 `Os { code: 2, .. }` 原样打给用户。统一走 `main::io_reason`（按 `ErrorKind` 归纳成可翻译的一句话）。
- **低精度下崩溃的坑**：`BigFloat::div` 内部是 `assert!`，除数为零直接 panic。
  牛顿迭代里"导数为零就放弃"的判断曾写成 `derivative.value.abs() <= 1 && derivative.precision >= 60`
  ——那个 `60` 是按默认精度 80 硬编码的**精度闸门**，`/mode prec 40` 时恒不成立，
  于是精确零除数一路走到 `BigFloat::div` 把进程打崩（实测 `sin(x)=0`）。
  **凡是拿 `BigFloat::div` 的除数，都要显式 `is_zero()` 先拦**，不要指望"某处已经判过"。
- `/lang` 的原始按键菜单必须让 `LANG_MENU_LINES`（上移行数）**等于实际打印行数**（表头 + 3 语言 + 提示 = 5），
  每行都以换行结束；行数对不上时每按一次方向键就会多滚出一份菜单。箭头颜色取 `colors.operator`（运算符色）。
- `mode_info()` 末尾附加当前语言（`i18n::get().native_name()`），启动横幅与 `/mode` 共用。

### 多项式拟合（solver_fit.rs）

- **识别在 `parser::looks_like_polynomial_fit`，必须在 `parse_system` 之前**（`parse_and_eval` 里插在 `parse_sd` 之前）；
  认领规则是"行首 `(` 或 `P(`/`p(` + 第一个括号组内 depth==1 层有逗号 + `)` 之后只允许 行尾/`(`/字母"，
  这三重约束保证 `(1+2)*3`、`f(1,2)`、`P(2)`、`(1,2)+3`、`P+1`、`2P` 都**不认领**（行为与旧版一致）。
  **认领后失败即报错、不回落**（回退只会给出误导性的"缺少右括号"）；**不要改 `parse_atom` 的括号分组**来容纳逗号。
- 顶点**只认大写 `P`**（小写 `p(x,y)` 给专门提示）；`P` **不是保留标识符**，变量仍可叫 `P`。
- 约束计数：`constraints = 普通点数 + 2×顶点数`；无模板时次数 = constraints − 1，顶点行是**解析导数** `[0,1,2a,3a²,…]`；
  用 `solver_linear::gaussian_elimination` 解（先判 `unique`；`infinite` ⇒ 重复/退化；`None` ⇒ 矛盾）。
- 带模板时用"单位向量代入法"线性化：`φ_j = extract_polynomial(只有 p_j=1) − g`（**必须减 g**，
  否则含常数偏移的模板如 `A*x + b` 会算错）；矩阵解用自写 `rref`（要拿"主元变量 = 自由参数的表达式"，
  `gaussian_elimination` 只给唯一解）。解与系数都是 `LinForm`（自由参数的线性形式），
  解析式按次数相加 ⇒ 自动展开合并同类项；欠定时输出关系行 + `自由参数:` 行。
- 两道**必要**校验：`check_param_linearity`（样本参数值比较"直接代入模板"与"线性组合"）与
  `verify_solution`（把解代回模板逐个坐标复验）——单位向量法只对"参数线性"的模板成立，
  它们负责拦住 `a*b*x + c`、`a^2*x + b` 这类模板。
- 输出**恒为 `y = …`**（模板左值只是装饰，一律忽略）；二次函数在"系数全为常数、实际二次、首项非 0、m≠0"时
  追加顶点式 `y = a*(x - m)^2 + k`；`m = 0` 时顶点式与一般式逐字相同 ⇒ 不重复输出。
- 系数是 `Number`（可为 `pi`/无理/自由参数形式），**不要**改用 `solver_factor::format_poly`（只吃 `BigRational`）。
- 护栏：`FIT_MAX_SIZE_FAST = 16` 且用 `calc_mode::is_deep()` 放行 Deep；新文案都进 `i18n::TABLE`。
- 顺带修掉的既有 bug：`number::check_pow_scale` 对**零底数**（`0^2`）会估出 `inf` 位数而误报错——
  现已提前 `if self.is_zero() { return Ok(()) }`，不要删。

### 三角形求解（solver_triangle.rs）

- **输入形式是 `triangle(a=3 b=4 c=5)` 函数调用，识别在 `parser::parse_triangle_call`，必须排在 `parse_system` 之前**
  （插在拟合守卫之前/之后都可以，两者首字符不同、不冲突）。**旧版的裸写 `a=3 b=4 c=5` 已取消**，
  原因：那种写法与"方程组 / 隐式乘法"边界太近（`a=3 b=4 c=5` 本会被读成 `a = 3*b`），很容易误解，
  改用显式函数调用后语义一目了然（`a=3 b=4 c=5` 现在回到"位置 5 处多余的字符"）。
- **记号是"任意字母 + 大小写区分角色"**：小写 = 边、大写 = 角、`h`/`H` + 字母 = 高；`a` 与 `A` 是同一个顶点。
  `Alphabet` 保存三个顶点字母，边/角/高标签全由它派生（`TriPart::index()` → `side_label`/`angle_label`/`height_label`）。
  顶点按**首次出现顺序**排定（`x=1 y=2` 第三顶点补 `z`、跳过已用字母、`y`/`z` 用满后回绕到 `a`），超过 3 个不同字母报错。
  **内部索引语义不变**（`Side(i)` = 第 i 个顶点的对边、`Angle(i)` = 第 i 个顶点的角、`Height(i)` = 第 i 条高），
  求解公式对下标是轮换一致的 ⇒ 换字母只是换标签，**不要碰几何逻辑**。记号解析集中在 `parse_part_name`
  （单字母 / `h`+字母；`lower_form` 拒绝汉字这类无大小写区分的字符）；`format_triangle_solution`、错误提示、
  `underdetermined_message` 的比例标签都必须用 `alphabet` 取名，**不要硬编码 `"a"`/`"A"`/`"hA"`**。
- **与 `sd`/`fac` 同一约定：只能是整个表达式的最外层函数**。判定分两层，缺一不可：
  ① `triangle` 标识符必须出现在**输入开头**（`contains_triangle_ident` 做词边界匹配，别处出现即报"必须是整个表达式的最外层函数"）；
  ② 配对右括号之后**不能再有内容**（否则同样报最外层错误）。认领后失败**不回落**——回落只会给出误导性提示。
  注意 `EvalResult` 没有实现 `Debug`，测试里取错误串要用自写的 `perr()` 而不是 `unwrap_err()`。
- **括号内的分隔符是"顶层逗号**或**空白"**（`split_triangle_items` 做深度感知，`log(2,8)` 里的逗号不会被切开）。
  旧的 `looks_like_triangle` 因为要避开 `,`（方程组）才禁止 RHS 含逗号，改成函数形式后这个限制消失了。
- **内部角度一律弧度**：输入用 `angle_to_radians` 按 `Evaluator::angle_mode` 换算（`deg` 时乘 `π/180`，
  保精确），输出用 `parser::radians_to_degrees` 精确转回（`Pi(coeff) → Rational(coeff*180)` ⇒ `π/2` 显示 `90`）。
  **反三角必须先走 `trig::try_exact_*` 再转换**，顺序反了 `arccos(0)` 就退化成 `≈ 90`。
  `rad` 模式下 `A=30` 是 30 弧度 > π ⇒ 会报"角度必须在 (0,180°) 范围内"，这是预期行为。
- **面积/角度公式约定**：`a = 2R·sinA`（**不要写成 `2R·sinA/2`**，SSA/AAS 两条路径都踩过）；
  `S = a·b·c/(2·2R)`；`R = abc/(4S)`、`r = S/(p/2)`、`hA = 2S/a` 全走 `Number` 精确运算。
- **SSS 必须先验三角不等式再调海伦公式**——矩形不成立时乘积为负，`BigFloat::sqrt` 会**直接 panic**
  （不是返回 Err）。`solve_sas` 里的 `sq` 同理要先判非负。
- **SSA 双解**：`h = b·sinA`；`a < h` 无解、`a = h` 一解（直角）、`h < a < b` **两解**、`a ≥ b` 一解。
  比较一律走 `Number` 的 `sub().is_negative()/is_zero()`，**`Number` 没有 `PartialEq`**。
- **高度归约**只做"能唯一确定"的方向：`hA·a = 2S`（高×底边→面积）、`hA = b·sinC = c·sinB`（高×邻角→邻边）、
  `a·hA = b·hB = 2S`（两条高+一条边→另一条边）。`sinC = hA/b` 这类**反解是两义的，不要放进归约**
  （会给出错误的对角），交给数值兜底。「三个高」只能定形状，会补一行最简整数比 `a : b : c`。
- **数值兜底**：未知量 `(A, B, s=2R)`（`C = π−A−B`、`a = s·sinA`、`hA = s·sinB·sinC`），
  残差按**已知值本身归一化**（角度按 π），多起点 Gauss-Newton 解法方程 `JᵀJ Δ = −Jᵀ r`（3×3 自写列主元消元），
  收敛后**去重 + 回代验证**。法方程奇异 ⇒ 报"信息不足"而不是"未收敛"（两者提示语义不同）。
- **已知量是"真值"**：`verify_givens` 逐项回代比对（超定矛盾在这里拦住），通过后 `to_solution` 把
  已知的边/角/高原样写回 ⇒ `A=30` 显示 `30` 而非 `29.9999…`。不要省掉写回，否则用户输入精确值时输出会变丑。
- 输出固定 5 行，`=`/`≈` 由 `solve_aux::result_prefix` 逐值判定；标签 `面积/周长/外接圆半径/内切圆半径`
  是**词条**（`t()` 支持 2 字以上的行内匹配），符号标签（`a`…`r`、`hA`…按用户字母而定）不翻译。多解时由 `handle_triangle` 加 `解 N:` 分段。
- 顺带修掉的既有 bug：计时行曾写成 `println!("{}", t.dimmed())`，绕过 `i18n` ⇒ 英/繁模式下留中文；
  现已改回 `lprint!`。**任何用户可见输出都必须走 `lprint!`/`leprint!`/`lprint_inline!`**。

### 计算模式（calc_mode.rs）

- `CalcMode::{Fast, Deep}` 存在 `AtomicBool` 里（低层数值函数拿不到 `Evaluator`，且 REPL 单线程执行运算）。
- **新增任何规模护栏时都要用 `crate::calc_mode::is_deep()` 判断是否放行**，否则 Deep 模式会"半死算"；
  放行前别忘了 Deep 下可能出现的极端规模（例如 `int_divisors` 需要 BigInt 兜底分支）。
- 反向要求：**牛顿迭代的发散保护（`NEWTON_ABS_LIMIT`）不属于规模护栏，两种模式都必须保留**，
  否则 `2^x=8` / `e^x=2` 会立刻回到卡死状态。
- 显示差异集中在 `bigfloat::to_significant_string`（Deep 完整精度、不走科学计数法）与
  `solve_aux::finite_decimal_fits`（Deep 下有限小数一律算"能完整显示" ⇒ 前缀 `=`）。
- 模式只影响行为，不影响 `Evaluator` 里的显示/角度模式；`/mode` 无参数时由 `mode_info()` 拼成 `LineIO/Radian/Fast`。

### REPL（lib.rs）

- prompt 必须是纯文本 `"> "`；prompt 里放 ANSI 颜色会让光标位置偏移。
- 运算计时（`Timing`）：**计时范围是"解析 + 结果生成"整段**（结果格式化阶段——通式识别、根收集、因式分解——常比 parse 本身更耗时，只包 parse_and_eval 会导致动态计时从未出现）；handler 只生成结果字符串，统一在单次打印前 `stop_dynamic()`。动态行用 `\r` 刷新、无换行，所以**清行必须在结果打印之前**，且要先 `join` 刷新线程（否则线程可能在结果之后又写一次，画面错位）；停止线程靠关闭 `mpsc` 通道（`recv_timeout` 立即返回），不要用 `sleep` 轮询等待。`is_terminal()` 为假的管道场景要跳过全部 `\r` 输出。
- 结果文本的生成统一在 `run_line(input, state) -> (String, bool)`（REPL 与非交互入口共用）；
  **打印结果行必须走 `lprint!`**——写成 `println!` 会绕过翻译，英文模式下标签/提示不翻译（批次 1 踩过这个坑）。
  计时与输出由调用方负责：REPL 用 `Timing`，非交互模式只打印裸结果。
- `/timing on|off` 开关存在 `calc_mode::timing_enabled()`（AtomicBool，默认开），持久化为状态文件 `timing=` 行；
  **落盘值取 `AppState::timing_base`（经 `timing_for_persist`），不要直接写运行期开关**——否则 `-q` 会污染配置；
  `/timing on|off` 与 `/reset all` 都要同时更新 `timing_base`。
  `Timing::begin()` 在关闭时不起刷新线程、`elapsed_text()` 返回空串（调用方要先判空再打印，否则会多出空行）。
- Tab 补全与内联提示的逻辑抽成纯函数 `completion_candidates(token, vars)` 与 `inline_hint(line)`（都有单元测试），
  `Completer`/`Hinter` 只做转发；`CalcHelper` 除 `colors` 外还带 `vars`（存储变量名），
  **增删变量后要连颜色一起刷新**（主循环里两处同步点已改）。候选集只含**多字母**函数/常数/变量名与指令，
  单字母 token 一律不给候选——否则会把 `xy` 这类隐式乘法补成函数名。
- 指令清单 `COMMANDS`（补全用）与 `COMMAND_HINTS`（内联提示用）、`FUNCTIONS_META`（函数元数 + 参数签名，供补全与框内提示用）都在 lib.rs 顶部；
  新增指令/两参函数时记得同步这几张表（有 `commands_table_is_consistent` 测试兜底）。
- `calc_mode::quiet()`：输出宏 `lprint!`/`leprint!`/`lprint_inline!` 在静默状态下跳过打印（`/load` 回放脚本时开启）。
  开启后**必须**恢复，否则后续输出全被吞掉。
- `/save` 用 `state::encode_var_for_script`（精确值走 MathIO 符号、近似值走完整十进制串）——它与状态文件内部的
  `encode_var`（`E:`/`A:` 前缀）**不是一回事**，不要混用（后者喂给求值器会解析失败）。
- `/reset all` **不重置语言**（避免误删用户偏好），但会重置模式、颜色、精度、计时开关。
- `/set` 改色后要立即 `rl.helper_mut().unwrap().colors = state.colors.clone()`，否则高亮不更新；
  `/set` 成功后立即 `persist_state()`（颜色写入状态文件的 `color:<类别>=<颜色名>` 行）。
  类别与颜色名的唯一来源是 `COLOR_CATEGORIES` / `COLOR_OPTIONS` 两张表（英文名 + 中文标注 + 颜色值），
  `/set` 解析、当前颜色列表与状态文件读写都走 `parse_color` / `color_name` / `ColorConfig::{set_category,get_category,to_pairs}`，
  **不要在别处再写一份 match 颜色名**。
- 文本着色只有两个入口，共用底层 `colorize_impl(line, colors, command_anywhere, base)`：
  - `colorize_text(line, colors, command_anywhere)` → `base=None`，未识别文本**原样**输出。
    输入行传 `false`（只有行首 `/xxx` 是指令，行内 `/` 是除法）、`/help` 与用法提示传 `true`。
  - `colorize_result(line, colors)` → `base=Some(colors.result)`，未识别文本用 result 色打底。
    **所有计算结果都必须走它**（`format_result_line`、`handle_factor`、`handle_equation`、`handle_system`、
    `handle_fit`、`handle_triangle`、`format_solutions`、`/let` 与 `/var` 的值显示）。
  两点不能忘：
  ① **不要**回到"整行刷单一 `result` 颜色"（`line.color(colors.result).bold()`）——那样结果看起来和没高亮一样，
     用户明确要求结果与输入用同一套高亮；
  ② 底色**不要加粗**：`result` 默认 `BrightWhite`，加了 bold 会把同行的数字（`number` 默认 `White`，且
     `colorize_impl` 里的数字分支不 bold）衬得过暗，纯白加粗标签 / 灰白数字的对比很刺眼。
  多行/多段结果要**整体着色**后再拼接（如 `format_solutions` 是 `colorize_result(&parts.join(", "))`），
  否则分隔用的 `, ` 会掉出底色。
  `/help` 正文是 `HELP_TEXT` 常量（无边框简版），`【…】` 小节标题用 result 颜色加粗；仅输入 `/set` 时
  `print_set_usage` 先给用法（类别、颜色均带中文标注）再列当前颜色（颜色名用各自的颜色显示）。
  `OPS` 含 `!`/`=` 以及**只在结果里出现**的 `≈`/`×`/`·`，`CONSTANTS` 含 `π`；`/mode`、`/let`、`/del` 的用法提示用 `colorize_text` 生成，
  「用法/提示」标签用 `prompt` 颜色，未知指令/模式/颜色用 `error` 颜色。
  验证颜色**必须**加 `CLICOLOR_FORCE=1`（`colored` 在非 TTY 下自动关闭颜色，管道里看不到 ANSI，
  这也意味着纯文本输出与着色改造前逐字节一致）。
- `sd(x)`（`EvalResult::SdValue`）的前缀**按互换后的显示模式**判定：MathIO→小数用 `result_prefix(_, LineIO)`、
  LineIO→符号用 `result_prefix(_, MathIO)`；不能无条件写 `≈`（旧实现把 `sd(1/2)` 的精确结果标成 `≈ 0.5`）。
- 变量存储：`/let`（名须全大写，含 `_`）、`/var`（列出）、`/del A`（删单个）、`/del all`（清空，**严格匹配小写 `all`**），存于 `Evaluator.vars`；
  未知数提取统一走 `unknown_variables()`（`handle_equation` 与 `handle_system` 共用），
  且 `extract_linear_rec` **必须**把存储变量与 `ans` 代入常数项——否则 `x+A=5, y=1`（A=4）会被解成 `x = 5`。
- `handle_let` 解析参数用"剥离 `/` + 命令词（`take_while(is_ascii_alphabetic)`）"的方式，
  **不要**用"跳到第一个大写字母"的写法——那会把 `/let 1A=5` 静默当成 `A=5` 存下来。
- 会话状态持久化（`state.rs`）：`~/.hipercalc_state` 存显示/角度/计算模式、全部变量与 `/set` 颜色，
  启动 `state::load()` 恢复、每次变更后 `persist_state()` 落盘、退出时再存一次。
  编码约定：精确值存 `E:<MathIO 表达式>`（重新解析还原，无损），近似值存 `A:<value>:<precision>`
  （BigFloat 内部十进制，无损），颜色存 `color:<类别>=<颜色名>`（`state::load` 只返回原始
  `(类别, 颜色名)` 字符串，**合法性由 lib.rs 用 `parse_color`/`set_category` 校验后应用**，非法项跳过）。
  **删除状态文件即恢复默认**——不要把默认值也写进文件逻辑里造成"删不掉"。
- 历史持久化：`history_path()`（`%USERPROFILE%/.hipercalc_history`），启动 `load_history`、退出 `save_history`，每行 `add_history_entry`。
- 新增模块要在对应 crate 的根声明：core 的写在 `crates/hipercalc-core/src/lib.rs`，上层的写在 `crates/hipercalc/src/lib.rs`。
- 改动任何功能/输出后同步更新 `README.md`（第一部分功能手册 + 第二部分代码架构）。

### 高等数学（src/calculus/）

- **架构：解析完成后、分类/求值前插一层 AST 重写**（`parse_and_eval` 里调 `expand_calculus`），
  把 `diff`/`lim`/`int`/`taylor`/`sum`/`prod` 节点**就地展开**成普通 `Expr`。好处：①不必给 `Parser`
  加生命周期；②不新增 `Expr` 变体（避开 12 处穷尽 match）；③展开结果天然能参与运算
  （`diff(x^2,x)+1`、`diff(x^2,x)=2` 直接解出 `x = 1`）。
- **零行为变化是硬要求**：入口先做字符串子串预扫描（函数名两侧标识符边界 + 后跟 `(`），未命中原样返回。
  改这段必须确认既有输入（`x+1` 仍报未定义变量、`1+2*3` 仍是 7）不受影响。
- **职责边界**：`expand_calculus` 递归替换普通节点，但**遇 `Factor`/`Sd` 不进入**、在递归前检查内部并报错；
  `parse_factor`/`parse_sd` 两个分支返回前也各走一次重写，好让 `fac(diff(x^3,x))` 给出真正原因
  （否则会报"无法因式分解"）。`eval_function` 里的同名拦截**只是兜底**。
- **顶层就是一次高数调用时不做整体化简**：这样 `taylor(...)` 能保留自己拼好的**升序**；
  与别的运算组合后再交给通用化简（次数降序，与 fac 一致）。
- **化简器（normalize.rs）四个坑**（每个都有回归测试守着）：
  1. 嵌套幂必须合并（`((1-x)^2)^2 → (1-x)^4`），否则两个底字面不同、同底指数永远合不上；
  2. **只有单项底才展开整数幂** —— 多顶底的 `(1-x)^4` 要保留为原子幂，展开会让反复求导的项数指数膨胀；
  3. **除数含多项时不拆分子**（`(a+b)/c` 保持一项），同理；
  4. 负幂只在**单项底**上"逐项取倒数"，多顶底必须保留指数（否则会算出 `1 - 1/x` 这种垃圾）。
- **数值复核是硬要求**：极限的结构解（**包括直接代入**）与积分求得的原函数都要过数值验证。
  `lim(abs(x)/x,x,0)` 若不复核就会采信洛必达给出的 0（实际左右极限是 ∓1、极限不存在）。
  教训：**"代入/求导能算出一个数" ≠ "它就是答案"**，不连续点只有数值证据才算数。
- **误差归因要准**：`lim(sin(x),x,inf)` 的数值探测若一路加大就会撞上 `check_trig_range` 的
  `10^(precision-2)` 护栏、把错误报成"三角函数参数过大"。探测指数封顶 `LIMIT_INF_K_MAX = 30` 就是为此。
- **数值积分用"计算节点"的 Gauss–Legendre**，不要硬编码 GK 常数：自适应 Simpson 在 1e-25 容差下
  需要上万次求值、直接撞穿预算。节点由 Newton 迭代现算，可用"对 2n-1 次多项式精确"验证。
- **泰勒系数的精确化**：求值器对 `cos(0)`/`sin(0)` 返回 `Approx`，必须经 `snap_exact` 还原成精确值
  （十进制还原 + 约分，分母 ≤ 10⁶ 才认），否则会输出 `1.0000000000…*x`。
- **新增一个高数函数要同步**：`CALCULUS_ARITIES`、`FUNCTIONS`、`lib.rs::FUNCTIONS_META`、`/help` **三份**、
  `i18n::TABLE`（`en.json` + `zh-TW.json`）、`docs/DOC.zh-CN.md` + `docs/DOC.en.md`。`CALCULUS_INF_ALLOWED` 决定 `inf` 能出现在哪个参数位
  —— 不登记就会落到求值路径报"未定义变量: inf"，误导用户。
