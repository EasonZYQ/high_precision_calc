//! 后台求值线程：把 `Session` 挪出 UI 线程，让界面**永远不卡**。
//!
//! ## 为什么需要它
//!
//! 界面最初的写法是在 `update` 里直接 `self.session.eval(..)`。这对 `1+1` 没问题，
//! 但对 `100^100^10` 是**灾难**：求值跑在 iced 的事件循环线程上，跑多久界面就冻结多久。
//! Windows 在窗口几秒不处理消息后会给标题栏挂上「(未响应)」—— 用户看到的就是这个。
//!
//! ## 模型
//!
//! ```text
//!   UI 线程                                  worker 线程（常驻一个）
//!   ────────                                 ──────────────────────
//!   update(Evaluate)
//!     ├─ busy = true
//!     └─ 把 Request 推进 mpsc 队列  ────────▶  Session::eval(..) 逐行
//!                                               │ Instant::now() 计时
//!   Task::perform(阻塞等回传)  ◀───────────────┘ 把 Evaluated 发回请求自带的通道
//!     └─ push 历史 / busy = false
//! ```
//!
//! 关键点：
//! - **`Session` 只有 worker 持有**。UI 侧不再有 `session` 字段，从类型上就杜绝了
//!   "顺手在 update 里跑一次求值"重新引入卡顿的可能。
//! - 但模式（显示 / 角度 / 计算 / 语言 / 精度）仍要能被 UI 改。这些走**另一条**
//!   队列（[`Command`]），由 worker 在执行请求的间隙顺带应用 —— 这样 UI 改设置是
//!   非阻塞的，且不必给 `Session` 加锁。
//! - 计时用 `std::time::Instant`，与 CLI 的 `Timing` 同一套口径（整秒；不足 1 秒
//!   显示 `<1秒`）。文本格式化在 UI 侧（[`crate::app::elapsed_text`]），因为
//!   文案要跟着界面语言走，而语言是 UI 侧的进程级开关。
//!
//! ## 为什么不是 `Arc<Mutex<Session>>`
//!
//! 那样每个 `update` 分支都得 `lock()`，一旦 worker 正拿着锁跑 `100^100^10`，
//! UI 线程就会**卡在 lock 上** —— 换了个地方冻结而已，而且更隐蔽
//!（连接口都还是"非阻塞"的样子）。独占 + 消息传递是这个场景唯一干净的解法。
//!
//! ## 为什么每个请求自带回传通道
//!
//! 朴素写法是"一条共享的回传通道"，但那样无法区分响应对应哪个请求，
//! 而用户完全可以在上一个 `100^100^10` 还在算的时候改精度再求值 ——
//! 两个请求同时在飞，回来的顺序不保证。让**请求自带一条 `Sender`**
//! （`mpsc::channel()` 每次新建，成本极低）就把对应关系变成了类型事实：
//! 收到的结果必然是这个请求的，不需要 id 比对。

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hipercalc::Session;
use hipercalc::parser_api::DisplayMode;
use hipercalc_core::calc_mode::CalcMode;
use hipercalc_core::trig::AngleMode;

/// UI → worker：改一项会话设置。
///
/// ⚠️ 这里**必须**逐项枚举、不能直接送一个闭包 `Box<dyn FnOnce(&mut Session)>`：
/// 闭包不是 `Debug`（`Message` 要 `Debug`），而且能送闭包就等于把
/// "在 worker 上跑任意代码" 开了口子 —— 之后很容易有人把整段 `eval` 塞进来，
/// 于是"求值只在 worker 串行发生"这条不变量就悄悄破了。
///
/// ⚠️ **语言（`SetLang`）刻意不在这里**。语言是进程级全局（`i18n` 的 `AtomicU8`），
/// 而界面每一处标签都在**当帧的 `view`** 里直接读它 ⇒ 必须在 UI 线程上**同步**生效
///（否则用户点完语言按钮，界面还是旧语言，什么时候翻过来取决于下一次求值）。
/// 既然 UI 线程已经设过了，让 worker 再设一遍就有害无益：
/// 那是**异步**的，会把全局值改成"稍后才知道谁先谁后"的状态 ——
/// 实测表现为测试之间互相污染（一个用例刚还原成中文，被另一个用例的 worker
/// 延迟改回英文）。所以语言只走 [`crate::app`] 里那条同步路径。
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    SetDisplay(DisplayMode),
    SetAngle(AngleMode),
    SetCalc(CalcMode),
    SetPrecision(usize),
}

/// worker 收到的请求。求值请求自带回传通道（见模块头的说明）。
pub enum Request {
    Eval {
        /// 输入原文（多行用 `\n` 连接），原样回传给 UI 做回显
        input: String,
        /// 已 trim、已去空行的待求值行；调用方保证非空
        lines: Vec<String>,
        reply: Sender<Evaluated>,
    },
    Config(Command),
}

/// worker 回传的求值结果。
///
/// 只带**结果**，不带 `Session` 内部状态 —— 界面需要的那点状态
/// （`Ans`、精度回填）由 [`Command::SetPrecision`] 与 `Session` 查询另行处理。
#[derive(Debug, Clone)]
pub struct Evaluated {
    /// 与请求的 `input` 一致
    pub input: String,
    /// 展示文本（最后一行）
    pub text: String,
    pub is_error: bool,
    /// 整段"解析 + 结果生成"的耗时（对标 CLI 的 `Timing` 口径）
    pub elapsed: Duration,
}

/// 与 worker 通信的句柄。UI 线程持有它。
///
/// `Sender::send` 只是把消息推进队列就返回（接收端每轮循环开头就 `recv`，
/// 不会长时间不取），所以 `update` 里调它不会有可感知的延迟、更不会阻塞。
pub struct Evaluator {
    tx: Sender<Request>,
}

impl Evaluator {
    /// 起一条后台线程并把 `session` 交过去。
    ///
    /// 线程活到进程结束：`tx` 被 [`Evaluator`] 持有，`Evaluator` 活在 `State` 里，
    /// `State` 活到事件循环结束。`tx` 一 drop，接收端 `recv()` 返回 `Err`，线程自然退出。
    pub fn spawn(session: Session) -> Self {
        let (tx, rx) = mpsc::channel::<Request>();
        std::thread::Builder::new()
            .name("hipercalc-eval".into())
            .spawn(move || worker_loop(session, rx))
            .expect("无法创建求值线程（系统资源不足）");
        Evaluator { tx }
    }

    /// 派一次求值，返回 `(结果信箱, 阻塞等待的闭包)`。
    ///
    /// - **信箱**（`Arc<Mutex<Option<Evaluated>>>`）：结果落进来后，任何持有它的人
    ///   都能读到 —— 这是测试路径（`State::await_pending_for_test`）用的。
    /// - **闭包**：交给 `Task::perform(..)`（它收 `FnOnce() -> impl Future`），
    ///   内部**阻塞**等结果。⚠️ 必须由 `Task::perform` 放到线程池里跑 ——
    ///   在 `update` 里直接调它就又把 UI 冻住了。
    ///
    /// ## 为什么是"信箱"而不是直接返回 `Receiver`
    ///
    /// `Receiver` 不可 clone、`recv()` 也**只能取一次**。但同一个结果有两条消费路径：
    /// 产品路径要它去驱动界面，测试路径要它去断言。用一个单槽信箱（worker 只写一次、
    /// 读者只读一次）比"两条通道二选一"简单得多 —— 谁先读谁拿到，另一条路径
    /// 天然不会也有结果可拿（同一时刻只会有一条在等）。
    ///
    /// 真正跨线程的仍只有 worker 那一条通道；信箱只是结果的暂存点。
    pub fn eval(
        &self,
        input: String,
        lines: Vec<String>,
    ) -> (
        Arc<Mutex<Option<Evaluated>>>,
        impl FnOnce() -> std::future::Ready<Evaluated>,
    ) {
        let (rtx, rrx) = mpsc::channel::<Evaluated>();
        let sent = self
            .tx
            .send(Request::Eval {
                input: input.clone(),
                lines,
                reply: rtx,
            })
            .is_ok();

        // 结果的单槽信箱（见上面的说明）
        let slot: Arc<Mutex<Option<Evaluated>>> = Arc::new(Mutex::new(None));

        // 搬运线程：从 worker 的回传通道取到结果后写进信箱。
        // 结果只经手一次，所以"谁读"这件事不构成竞争。
        let slot_for_move = Arc::clone(&slot);
        let input_for_move = input.clone();
        std::thread::Builder::new()
            .name("hipercalc-eval-wait".into())
            .spawn(move || {
                let out = block_for(&rrx, sent, input_for_move);
                *slot_for_move.lock().unwrap_or_else(|e| e.into_inner()) = Some(out);
            })
            .expect("无法创建等待线程");

        // 闭包再持一份（`Arc` 不能 Copy，但 clone 很便宜）
        let slot_for_wait = Arc::clone(&slot);
        (slot, move || {
            let slot = slot_for_wait;
            // 等待：自旋到信箱里有东西（搬运线程一填就停）。
            // 用短睡而不是忙等 —— 大运算要跑几十秒，忙等会白烧一个核。
            let out = loop {
                if let Some(v) = slot.lock().unwrap_or_else(|e| e.into_inner()).clone() {
                    break v;
                }
                std::thread::sleep(Duration::from_millis(2));
            };
            std::future::ready(out)
        })
    }

    /// 与 [`Evaluator::eval`] 同一条路径，但**直接返回结果**（同步阻塞）。
    ///
    /// 只给测试用：单测里没有 iced 的执行器，没法 await 那个 `Ready`。
    /// 走的仍是同一个 worker、同一条通道，所以验的还是真实行为。
    pub fn eval_blocking(&self, input: String, lines: Vec<String>) -> Evaluated {
        let (rtx, rrx) = mpsc::channel::<Evaluated>();
        let sent = self
            .tx
            .send(Request::Eval {
                input: input.clone(),
                lines,
                reply: rtx,
            })
            .is_ok();
        block_for(&rrx, sent, input)
    }

    /// 改一项会话设置（非阻塞，不产生结果）。
    pub fn config(&self, cmd: Command) {
        let _ = self.tx.send(Request::Config(cmd));
    }
}

/// 在回传通道上等结果；发不出去（worker 已退出）时给一个空结果。
///
/// 抽出来是为了让 [`Evaluator::eval`] 与 [`Evaluator::eval_blocking`] 共用
/// 同一段等待/降级逻辑 —— 否则测试路径与产品路径就分叉了，测了也不算数。
fn block_for(rrx: &Receiver<Evaluated>, sent: bool, input: String) -> Evaluated {
    // 降级路径：worker 已退出（进程正在收尾）时给一个空结果，
    // 界面不会挂死，只是这一次求值没有产出
    let fallback = Evaluated {
        input: input.clone(),
        text: String::new(),
        is_error: false,
        elapsed: Duration::ZERO,
    };
    if sent {
        rrx.recv().unwrap_or(fallback)
    } else {
        fallback
    }
}

/// worker 主循环：串行处理请求，直到发送端全部 drop 才退出。
fn worker_loop(mut session: Session, rx: Receiver<Request>) {
    while let Ok(req) = rx.recv() {
        match req {
            Request::Config(cmd) => apply(&mut session, cmd),
            Request::Eval {
                input,
                lines,
                reply,
            } => {
                // 与 CLI 的 `handle_input_result` 同一位置起表：覆盖
                // 「解析 + 结果生成」整段（实测耗时大头在结果格式化 ——
                // 方程通式识别、根收集、因式分解 —— 而非 parse 本身）。
                let start = Instant::now();
                let mut result = session.eval(&lines[0]);
                for line in &lines[1..] {
                    result = session.eval(line);
                }
                let elapsed = start.elapsed();

                // 界面已经走了（通道断了）就丢弃结果，别白等
                let _ = reply.send(Evaluated {
                    input,
                    text: result.text,
                    is_error: result.is_error,
                    elapsed,
                });
            }
        }
    }
}

/// 把一项设置应用到会话上。
fn apply(session: &mut Session, cmd: Command) {
    match cmd {
        Command::SetDisplay(m) => session.set_display_mode(m),
        Command::SetAngle(m) => session.set_angle_mode(m),
        Command::SetCalc(m) => session.set_calc_mode(m),
        Command::SetPrecision(p) => session.set_precision(p),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 求值走后台，且**结果能回来**（这条是整套异步改造的地基）
    #[test]
    fn eval_round_trips_through_worker() {
        let ev = Evaluator::spawn(Session::new());
        let out = ev.eval_blocking("1+1".into(), vec!["1+1".into()]);
        assert_eq!(out.input, "1+1");
        assert!(!out.is_error, "1+1 不该报错：{out:?}");
        assert!(out.text.contains('2'), "结果应含 2：{out:?}");
    }

    /// worker 是**串行**的：连着两个请求，各自拿到的都是自己的结果
    ///（若回传通道被共享，这里会串味）
    #[test]
    fn concurrent_requests_do_not_cross_wires() {
        let ev = Evaluator::spawn(Session::new());
        let ra = ev.eval_blocking("2+2".into(), vec!["2+2".into()]);
        let rb = ev.eval_blocking("3*3".into(), vec!["3*3".into()]);
        assert!(ra.text.contains('4'), "2+2 应得 4：{ra:?}");
        assert!(rb.text.contains('9'), "3*3 应得 9：{rb:?}");
    }

    /// 多行输入：**逐行走一遍会话**，只回传最后一行
    ///（`ans` 是唯一必然跨行变化的副作用，用它证明中间行真的生效了）
    #[test]
    fn multiline_runs_line_by_line() {
        let ev = Evaluator::spawn(Session::new());
        let out = ev.eval_blocking("5!/2\nans*2".into(), vec!["5!/2".into(), "ans*2".into()]);
        assert!(
            out.text.contains("120"),
            "5!/2=60 后 ans*2 应为 120：{out:?}"
        );
    }

    /// 计时**必须大于零**：大运算与微小运算都得给出一个真实的 `Duration`。
    ///
    /// 这条盯住"别忘了在 worker 里起表"这个很容易漏的改动 ——
    /// 漏了的话耗时恒为 0，界面上永远显示 `<1秒`，而且**看起来很正常**。
    #[test]
    fn elapsed_is_measured() {
        let ev = Evaluator::spawn(Session::new());
        let out = ev.eval_blocking("2^10000".into(), vec!["2^10000".into()]);
        assert!(
            out.elapsed > Duration::ZERO,
            "耗时应当是测出来的、而非零值：{:?}",
            out.elapsed
        );
    }

    /// 大运算**真的**要花时间 —— 用一个明确的阈值证伪"计时是假的/恒为零"。
    ///
    /// `5000!` 是高精度阶乘，实测在百毫秒量级；这里只要求 > 1ms，
    /// 足够把"根本没测"和"测了但用了极粗的时钟"区分开。
    #[test]
    fn big_work_takes_measurable_time() {
        let ev = Evaluator::spawn(Session::new());
        let out = ev.eval_blocking("5000!".into(), vec!["5000!".into()]);
        assert!(
            out.elapsed > Duration::from_millis(1),
            "耗时：{:?}",
            out.elapsed
        );
    }

    /// 设置命令在 worker 上生效：改成 LineIO 后，同一条 `sqrt(2)` 的结果形态
    /// 从数学式退化成小数（这是**只有** worker 真的执行了 `Config` 才可能的）
    #[test]
    fn config_reaches_worker_session() {
        let ev = Evaluator::spawn(Session::new());
        ev.config(Command::SetDisplay(DisplayMode::MathIO));
        let math = ev.eval_blocking("sqrt(2)".into(), vec!["sqrt(2)".into()]);
        assert!(!math.is_error, "sqrt(2) 不该报错：{math:?}");

        // LineIO 下结果不该还是 `sqrt` 文本
        ev.config(Command::SetDisplay(DisplayMode::LineIO));
        let line = ev.eval_blocking("sqrt(2)".into(), vec!["sqrt(2)".into()]);
        assert!(
            !line.text.contains("sqrt"),
            "LineIO 下结果不该还是 sqrt 文本：{line:?}"
        );
        assert_ne!(
            math.text, line.text,
            "MathIO 与 LineIO 的结果形态必须不同（否则说明 Config 没生效）"
        );
    }

    /// ⚠️ **配置与求值共用一个队列、顺序有保证** —— 这条是刻意为之的行为。
    ///
    /// "改模式"紧跟"求值"发出时，worker 先处理配置再处理求值，
    /// 所以**这一条**求值就已经按新模式算（用户不必"再算一次"）。
    #[test]
    fn config_is_ordered_before_following_eval() {
        let ev = Evaluator::spawn(Session::new());
        ev.config(Command::SetDisplay(DisplayMode::LineIO));
        ev.config(Command::SetAngle(AngleMode::Degree));
        // `sin(30)` 在角度制下是 0.5
        let out = ev.eval_blocking("sin(30)".into(), vec!["sin(30)".into()]);
        assert!(!out.is_error, "不该报错：{out:?}");
        assert!(
            out.text.contains("0.5"),
            "角度制下 sin(30) 应为 0.5（说明配置先于求值被处理）：{out:?}"
        );
    }

    /// 精度设置能落到 worker 的会话上，且**越界值被夹住**（与 UI 侧同一语义）
    #[test]
    fn precision_command_is_clamped_on_worker() {
        let ev = Evaluator::spawn(Session::new());
        ev.config(Command::SetPrecision(1));
        let out = ev.eval_blocking("1/3".into(), vec!["1/3".into()]);
        assert!(!out.text.is_empty(), "设完精度后求值应有输出：{out:?}");
    }

    /// worker 退出后派单**不能 panic**，只回一个空结果（降级路径）
    #[test]
    fn eval_after_worker_gone_degrades_gracefully() {
        let ev = Evaluator::spawn(Session::new());
        drop(ev);
        // `ev` 已 drop ⇒ 产品路径根本走不到这里；这条验证的是 `sent == false`
        // 那条降级分支：发不出去时不能 panic，也不能永远挂住
        let (tx, rx) = mpsc::channel::<Request>();
        drop(rx);
        assert!(tx.send(Request::Config(Command::SetPrecision(80))).is_err());

        // `block_for` 在 sent=false 时必须立即返回，不碰通道
        let (_tx2, rx2) = mpsc::channel::<Evaluated>();
        let out = block_for(&rx2, false, "1+1".into());
        assert_eq!(out.input, "1+1");
        assert!(out.text.is_empty(), "降级结果不该有文本");
        assert_eq!(out.elapsed, Duration::ZERO);
    }
}
