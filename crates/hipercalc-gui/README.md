# hipercalc-gui

`hipercalc` 的**图形界面**，基于 [Iced](https://iced.rs)（Elm 架构的跨平台 GUI 库）。

> **当前状态：只有环境自检骨架，正式界面尚未开始。**
> `src/main.rs` 是一个最小的 iced 程序，唯一目的是证明「工具链可用 + 能链接数值底座」。
> 它**不是**本项目的界面设计，正式开工时会被替换掉。

## 为什么单独一个 crate

- **依赖方向仍然是单向的**：`hipercalc-core ← hipercalc ← hipercalc-gui`。数值底座不碰 GUI，
  GUI 也不往数值层里塞东西。
- 二进制名 `hipercalc` 已经被 CI 冒烟与 release 产物占用，GUI 必须另起一个包名，
  否则会撞产物名（这也是本 crate 叫 `hipercalc-gui`、而不是把 GUI 塞进 `hipercalc` 的原因）。

## 环境要求

| 项 | 值 | 说明 |
|---|---|---|
| Rust | **≥ 1.88** | iced 0.14 的 MSRV；本机 1.97 可用 |
| 工具链 | `stable-x86_64-pc-windows-msvc` | iced 在 Windows 上要 MSVC + Windows SDK |
| 图形 | wgpu（默认）/ tiny-skia（软件兜底） | 默认 feature 同时启用两者 |

**版本选择**：`iced` 取 **0.14**（当前 crates.io 稳定版）。注意 docs 站点的差异——
`docs.iced.rs` 上的首页文档跟的是 **master（0.15.0-dev）**，与 0.14 的 API 有出入
（例如 0.15-dev 里 `update` 返回 `Task<Message>` 的写法更普遍）；查 API 请以
**`docs.rs/iced/0.14.0`** 或本地 `cargo doc -p iced --open` 为准。

## 运行

```powershell
cargo run -p hipercalc-gui     # 开一个窗口，显示 hipercalc-core 现场算出的 √2
cargo build -p hipercalc-gui   # 只编译
```

> 首次编译要拉取并编译 **354 个新依赖**（wgpu / naga / winit …；整个 workspace 的包数从 44 涨到 399），
> 耗时较长（实测 5m34s）；之后增量编译就快了。仓库根 `Cargo.toml` 的 `[profile.dev.package."*"] opt-level = 3`
> 对本 crate 同样生效——第三方依赖在 debug 下也带优化，渲染才不会慢得离谱。

## Iced 核心概念速查（0.14）

```rust
// 入口：State::default 造状态，update 改状态，view 画界面
fn main() -> iced::Result {
    iced::application(State::default, State::update, State::view)
        .title("标题")
        .window_size((760.0, 420.0))
        .run()
}

struct State { clicks: u64 }

#[derive(Debug, Clone, Copy)]          // Message 必须 Clone
enum Message { Increment }

impl State {
    // 只改状态（0.14 里返回 () 即可；要跑异步任务才返回 Task<Message>）
    fn update(&mut self, message: Message) {
        match message { Message::Increment => self.clicks += 1 }
    }

    // 只读状态；返回任何 `Into<Element>` 的类型
    fn view(&self) -> Column<'_, Message> {
        column![
            text(self.clicks),
            button("+").on_press(Message::Increment),
        ]
        .spacing(10)
        .padding(20)
    }
}
```

三个要点：

1. **`view` 里所有 widget 的 `Message` 类型必须与 `update` 一致**——这是编译器强制的，写错了不会跑偏。
2. **文本不会自动换行/滚动**，长结果要用 `scrollable` 包一层。
3. 组件是**构建器模式**（`.size(20)`、`.padding(10)`、`.style(...)`），不是配置对象。

## 正式开工前的前置任务

GUI 要真正能算，必须让 `hipercalc`（应用 crate）把求值入口暴露成公开 API——
它现在**只导出了 `pub fn run() -> i32`**（整个 REPL），GUI 没法复用。计划：

1. 在 `hipercalc` 里把 `run_line` 一类「输入字符串 → 结果字符串」的纯函数整理成公开 API
   （顺带把 `AppState` 的必要部分公开，供 GUI 保存显示/角度/计算模式）。
2. 在 `crates/hipercalc-gui/Cargo.toml` 里加上 `hipercalc = { path = "../hipercalc" }`。
3. 再动手做真正的界面。

## 参考资料

原始文档与源码已下载到仓库根的 `.iced-ref/`（**已加入 `.gitignore`，仅本地保留**）：

```text
.iced-ref/site/         docs.iced.rs / book.iced.rs / docs.rs 的离线快照 + widget 索引
.iced-ref/src/          iced 0.14.0 完整源码（含 src/application.rs —— 入口 API 的权威来源）
.iced-ref/examples/     官方示例 counter / todos / tour / integration / component
```

重新获取：

```bash
# 文档（curl 可直连）
curl -sL https://docs.iced.rs/iced/ -o .iced-ref/site/docs.iced.rs.html
curl -sL https://docs.rs/iced/0.14.0/iced/ -o .iced-ref/site/docs.rs-0.14.0.html
# 源码 / 示例（GitHub 直连被墙，走 gh）
gh api repos/iced-rs/iced/contents/examples/counter/src/main.rs?ref=0.14.0 \
   -H "Accept: application/vnd.github.raw" > .iced-ref/examples/counter/main.rs
curl -sL -H "User-Agent: hipercalc" https://crates.io/api/v1/crates/iced/0.14.0/download \
   | tar xz -C .iced-ref/src
```
