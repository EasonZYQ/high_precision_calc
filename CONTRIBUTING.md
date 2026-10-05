# 贡献指南

[English](CONTRIBUTING.en.md) · **简体中文**

感谢你愿意为 HiPerCalc 出一份力！无论报告问题、改进文档还是提交代码，都非常欢迎。
（Issue / PR 用中文或英文都可以。）

## 可以贡献什么

- **报告问题**：附上输入、期望结果与实际结果，最好带上运行环境（系统、构建方式）。
  如果输出很长，贴关键片段即可。
- **改进文档**：`docs/DOC.zh-CN.md`（完整中文文档）与 `docs/DOC.en.md`（英文）都需要同步维护。
- **提交代码**：修 bug、加函数、优化性能都欢迎。动手前建议先开个 Issue 说明思路，避免方向不一致。

## 开发环境

只需要 [Rust 工具链](https://rustup.rs)（stable），项目是单 crate、无额外系统依赖：

```bash
cargo build            # 调试构建
cargo build --release  # 发布构建
cargo run              # 启动交互式 REPL
cargo test --workspace # 跑全部单元测试（144 项：core 11 + hipercalc 133）
cargo test -p hipercalc-core   # 只跑数值底座（迭代大数/精度相关改动时快得多）
```

> Windows 上如果提示 `failed to remove ... hipercalc.exe (os error 5)`，说明程序还在运行——
> 先 `taskkill /f /im hipercalc.exe` 再构建。

## 提交前请自查

1. **`cargo build` 与 `cargo test --workspace` 都通过，且不引入新的编译警告**；
   **顺手记下耗时**（`cargo test` 结尾的 `finished in …`，要连编译一起看就用 `time cargo test`）——
   不看时间就发现不了改动是否把求解拖慢了；
2. **新增行为要有单元测试**（本项目测试都写成 `#[cfg(test)] mod tests`，直接在源文件末尾追加即可）；
3. **多语言同步**：词条表已经独立成 `src/language/<语言代码>.json`（编译期内嵌；**新增一门语言只需
   加一个 JSON 文件**并在 `language::FILES` 里登记，详见该目录的模块注释）。
   新增用户可见文案时，在 `zh-TW.json` 与 `en.json` 里补上对应键——**简体原文就是键**，
   两边的键集必须完全一致（漏一条就是那个语言的界面里残留中文）；
   带占位符的长词条要小心：词条的**字面部分不能横跨两个着色 token**
   （例如 `{0} = {1}，k 为整数` 里的 ` = ` 夹在变量与数字之间），否则替换会把该处的颜色码一起丢掉；
   这类情况应改写成连续纯文本的短词条（如只保留尾段 `，k 为整数`）；
4. **`/help` 三语正文同步**（`src/main.rs` 的 `HELP_TEXT` / `HELP_TEXT_TW` / `HELP_TEXT_EN` 三份）；
5. **文档同步**：改了功能就顺手更新 `docs/DOC.zh-CN.md`、`docs/DOC.en.md`；
   踩过的坑、改错容易犯的约定，请补进 `AGENTS.md`；
6. **别把不该提交的东西加进来**：`target/`、`.workbuddy/`、`.trae/`、`change_logs/`、`*.log`
   都在 `.gitignore` 里——`git add` 之后请确认暂存区没有它们。

## 代码约定

- **注释与提交信息用中文或英文都可以**（保持同一处前后一致即可，不必强求统一语言）；
- 用户可见的输出走 `lprint!` / `leprint!` 宏，**不要直接用 `println!`**，否则会绕过 i18n；
- 底层数值函数的**规模护栏不要删**（`exp` 参数上限、幂结果位数上限、试除预算、牛顿发散保护……）：
  它们的存在是为了避免"静默给出错误结果"或界面卡死，`/mode deep` 才是放开限制的开关；
- **凡是拿 `BigFloat::div` 做除法的除数，都要先显式 `is_zero()` 判零**——它内部是 `assert!`，除数为零会直接 panic；
- 用 `#[cfg(windows)]` / `#[cfg(not(windows))]` 写两份平台实现时，**两边的类型与函数签名必须保持一致**
  （共享调用方会同时编译两份，签名缺口只有非 Windows 平台才会暴露）；
- 改动某个模块前，先看 `AGENTS.md` 里对应小节的"容易改错的地方"，多数坑都记在那边。

## 提交流程

1. Fork 本仓库并从 `main` 拉一个分支（`fix/xxx`、`feat/xxx`）；
2. 按上面的自查清单过一遍；
3. **提交信息尽量简短**：一两句话说明做了什么即可，细节留给 PR 描述；
4. 开 Pull Request，描述里说清动机与验证方式。

## CI

每次 push / PR 会触发 [CI](.github/workflows/ci.yml)：

- **test**：Ubuntu / Windows / macOS 三平台跑 `cargo build` + `cargo test`；
- **cross**：把 Release 要产出的全部目标（Linux gnu/musl/aarch64、macOS x86_64/aarch64、Windows msvc）
  都编译一遍。

> **特别注意跨平台可编译性**：曾经因为非 Windows 桩里少声明了几个枚举变体
> （`rawkey::Key` 只有 `Other`），导致**除 Windows 外全部目标编译失败**，而且只在打 tag 时才暴露。
> 现在 `cross` 这一层会在 PR 阶段就拦住这类问题。

打 `v*` 标签会自动触发 [Release](.github/workflows/release.yml) 构建各平台二进制并发布到 Releases。
