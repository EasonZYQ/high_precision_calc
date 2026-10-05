//! hipercalc 二进制入口。
//!
//! 具体逻辑全在库里的 `hipercalc::run`（见 lib.rs）；这里只负责把退出码交给系统。
//! 把逻辑放进 lib 而不是 main，是为了让 REPL/命令分派能被测试直接调用。

fn main() {
    std::process::exit(hipercalc::run());
}
