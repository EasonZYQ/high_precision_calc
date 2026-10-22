# Windows 7 兼容性调研

> 结论先行：**命令行（CLI）技术上可行但代价很大且无法用 edition 2024；图形界面（GUI）不可行。**
> 本文说清"为什么"和"要花多少"，供决策。

---

## 1. 三条硬事实（都可查证）

| # | 事实 | 出处 |
| --- | --- | --- |
| 1 | **Rust 1.78（2024-05-02）起，`*-pc-windows-*` 目标的最低系统要求提到 Windows 10**（既约束工具链本身，也约束产出的二进制） | [Rust 官方博客《Updated baseline standards for Windows targets》](https://blog.rust-lang.org/2024/02/26/Windows-7/) |
| 2 | Win7 专用目标 `x86_64-win7-windows-msvc` / `i686-win7-windows-msvc` 只到 **Tier 3**：**没有预编译的 std**，要用 nightly + `-Z build-std` 自己编标准库；官方不保证、不进 CI | 同上（Tier 3 定义见 Target Tier Policy） |
| 3 | **最后一个"装好就能在 Win7 上跑"的稳定工具链是 1.77.2** | [rust-lang/rust#143268](https://github.com/rust-lang/rust/issues/143268) |

而本项目的另一条硬约束是**edition 2024 ⇒ rustc ≥ 1.85**。把两条放一起就是结论：

> **`edition 2024` 与 "Win7 能跑" 互斥**（1.85 > 1.77）。

---

## 2. CLI：三条路，没有一条是"改个 cargo 版本"就完事

### 路线 A：整体降级到 rustc 1.77.2 + edition 2021（唯一"确定能用"的路）

要动的**不只是 cargo/rustc 版本**：

| 要改的 | 说明 |
| --- | --- |
| 三个 crate 的 `edition = "2024"` → `"2021"` | **全 workspace**，会丢 edition 2024 的语言特性（如 `let` chains —— 本项目 `fonts.rs`、`theme.rs` 里已经在用） |
| 依赖全部钉版本 | `rustyline` / `colored` / `num-*` / `futures` 等都要降到最后支持 1.77 的版本，`Cargo.lock` 要锁死 |
| MSRV 写进文档 + CI 加一条 1.77 的构建 | 否则以后谁写一行新语法就把 Win7 断了，而且是**静默**的 |
| 双轨维护 | main 走 edition 2024（新特性、新依赖），另开 `legacy-win7` 分支只修 bug |

**成本估计**：一次性 1–2 天（大部分时间在依赖降级与编译错误排查），之后每次进 main 的功能都要想"这个要不要同步到 legacy"。

### 路线 B：nightly + `-Z build-std` + `x86_64-win7-windows-msvc`

- 保留 edition 2024 与现代依赖；
- 但**必须 nightly**（CI 得锁一个 nightly 版本，否则某天编译不过）；
- Tier 3 = 官方不测、不保证；`build-std` 要连标准库一起编，CI 时间 +10~20 分钟；
- 仍可能踩到"某个依赖用了 Win10-only API"的地雷，且**没有官方支持**。

**成本估计**：0.5–1 天试探 + 每次 CI 更慢 + 长期不确定性。

### 路线 C：不管 Win7

- Win7 已于 2020-01 停止支持，StatCounter 份额已不足 0.3%；
- Rust 官方明确表示 Win7 目标"最多停在 Tier 3"，不会回到 Tier 1（#143268 里 maintainer 的原话）。

---

## 3. GUI：不只是工具链问题，是**图形后端**问题

| 阻塞点 | 说明 |
| --- | --- |
| `wgpu`（iced 的渲染后端） | 默认走 **DX12 / Vulkan / Metal**。Win7 **没有 DX12**；Vulkan 只能靠厂商驱动（老机器常常没有） |
| `winit` / `iced` 的 Windows 后端 | 依赖 Win10 的窗口与 DPI API，Win7 上没有 |
| Rust std 本身 | 1.78 之后 std 假定 Win10 API 存在，二进制在 Win7 上直接加载失败 |
| 液态玻璃抓屏 | 依赖 **DXGI Desktop Duplication**，Win7 只有 DXGI 1.1（没有桌面复制接口） |

⇒ **GUI 支持 Win7 等于换渲染后端 + 换 UI 框架**，不是本次能做的量级。

---

## 4. 建议

1. **主分支不追求 Win7**，README/文档明确写"Windows 10 及以上"；
2. 如果确实有 Win7 用户，**只做 CLI，走路线 A**，单独开 `legacy-win7` 分支：
   - 该分支：`edition = "2021"`、`rustc 1.77.2`、`Cargo.lock` 锁死、CI 只跑 CLI；
   - 主线发版时顺手 cherry-pick 关键修复；
3. **不要**走路线 B 给主线做 Win7 支持 —— nightly + Tier 3 + build-std 的维护成本会一直压在 CI 上。

---

## 5. 如果决定要做（路线 A 的具体动作清单）

```bash
# 1) 装老工具链
rustup toolchain install 1.77.2

# 2) 新分支
git switch -c legacy-win7

# 3) 改 edition（三个 Cargo.toml）
#    edition = "2024"  ->  "2021"
#    并把 edition 2024 才有的写法改回（let-chains 等，编译器会逐条报）

# 4) 钉依赖：让 cargo 自己降
cargo +1.77.2 build --release 2>&1 | grep "requires rustc"   # 逐个降版本
cargo +1.77.2 build --release                                  # 直到通过

# 5) 在 Win7 虚拟机上冒烟：hipercalc -e "1/3+1/6"
```

---

*本文只做技术调研与成本评估，未改动任何代码。*
