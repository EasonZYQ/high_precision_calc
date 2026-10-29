//! 界面骨架：顶层 view、工具条、输入区、虚拟键盘。
//!
//! 从 `app.rs` 拆出来的视图层：**只读 `State`，不发消息以外的事**。
//! 业务逻辑仍在 `mod.rs` 的 `update` 里 —— 拆分的目的是让 4600 行的单文件
//! 变成"按区域可定位"的几个文件，而不是把逻辑打散。

use iced::widget::{column, row};

use super::*;

impl State {
    pub fn view(&self) -> Element<'_, Message> {
        let t = std::time::Instant::now();
        let el = self.view_inner();
        fps::on_view(t.elapsed().as_nanos() as u64);
        el
    }

    /// [`State::view`] 的本体（外面那层只负责计时）
    pub(super) fn view_inner(&self) -> Element<'_, Message> {
        // 把最新背景同步给玻璃着色器（`liquid` 的管线在版本号变化时才重传
        // 纹理）。这里每帧调用是刻意的：质感切到"关闭"时 `bg_pixels` 会被
        // update 清成 None，只有从 view 这条每帧必经的路同步，着色器才能
        // 可靠地知道"现在没有背景可采样"。
        //
        // 同时算出**窗口→背景图**的 uv 映射：窗口挪动时原点差跟着变，
        // 着色器按它平移采样 —— 拖动全程零抓屏、零 CPU（用户要求 60fps）。
        //
        // ⚠️ `uv_map` 收的是**抓屏区域的物理尺寸**（元组后两位），
        // 纹理像素尺寸（中间两位）只喂 `write_texture` —— 混用会整体错位
        //（change_log81 的根因，见 [`BgPixels`] 的文档）。
        let (bw, bh) = self
            .bg_pixels
            .as_ref()
            .map_or((0, 0), |(_, _, _, w, h, _, _)| (*w, *h));
        let bg_map = self
            .bg_pixels
            .as_ref()
            .and_then(|(_, x, y, _, _, rw, rh)| crate::backdrop::uv_map((*x, *y, *rw, *rh)))
            .unwrap_or(([1.0, 1.0], [0.0, 0.0]));
        liquid::sync_backdrop(
            self.bg_pixels.as_ref().map(|(p, ..)| p),
            bw,
            bh,
            self.bg_version,
            bg_map,
        );

        // 内容区（原来的四块）保留 14px 边距；**标题栏单独一行、吃满窗口宽度**
        // —— 最小化/最大化/关闭要贴到窗口右上角（现代窗口的样子），
        // 如果跟内容一起被 padding 包住，按钮会离边缘 14px，看着像"浮在中间"。
        let content_column = column![
            self.toolbar(),
            self.results(),
            self.input_area(),
            self.keypad_area(),
        ]
        .spacing(10)
        .padding(iced::Padding {
            // 顶部只留 6px：标题栏自己占 34px，再加 14 会显得头重脚轻
            top: 6.0,
            right: 14.0,
            bottom: 14.0,
            left: 14.0,
        })
        .height(Length::Fill);

        // 函数弹窗用 stack 做成浮层盖在最上面
        let content: Element<'_, Message> = if self.show_funcs {
            let backdrop = iced::widget::mouse_area(
                container(Space::new().width(Length::Fill).height(Length::Fill)).style(|_t| {
                    container::Style {
                        // ⚠️ **全透明**（用户反馈："打开函数面板会变暗"）。
                        //
                        // 之前这里是一层压暗遮罩（先是写死的 `BLACK 0.42`，后来改成
                        // 随主题的 `scrim`）。但函数面板几乎占满窗口，遮罩除了把
                        // 露出的一圈压暗之外没有任何作用 —— 而它唯一的**功能**是
                        // "点空白处关闭"，那不需要可见。
                        //
                        // 所以底色直接给全透明：点击照样能收到，界面一点不变暗。
                        background: Some(Background::Color(Color::TRANSPARENT)),
                        ..container::Style::default()
                    }
                }),
            )
            .on_press(Message::CloseFuncs);

            // ⚠️ **Desmos 式锚定**（用户 2026-10-24 拍板的小面板方案）：
            // 面板不再是"居中的大卡"，而是**悬在 `ƒ` 按钮上方**的一块小浮层 ——
            // 右下对齐 + 底部留出整个键盘区的高度，看起来就像从 `ƒ` 键上弹出来的。
            let panel = container(self.func_panel())
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::End)
                .align_y(Alignment::End)
                // ⚠️ iced 的 `Padding` 只支持单值/二元组，四向要写字面量结构
                .padding(iced::Padding {
                    top: 0.0,
                    right: FN_PANEL_RIGHT_PAD,
                    bottom: FN_PANEL_BOTTOM_PAD,
                    left: 0.0,
                });

            // ⚠️ **必须把内容区垫在最底下**（用户："我的意思不是遮住整个界面，
            // 是设置这个小框在最顶层，不是全部遮住啊"）。
            //
            // 上一版写的是 `stack![backdrop, panel]` —— 浮层**替代**了内容区，
            // 于是打开设置时工具条/历史/输入区/键盘**全都不画了**，只剩一块空白，
            // 看起来就是把整个界面盖住。我当时只改了高度，没发现根因在这里。
            stack![content_column, backdrop, panel].into()
        } else if self.show_settings {
            // ── 设置浮层（用户要求）────────────────────────────────────────
            //
            // 两条要求：
            // 1. **盖在历史计算之上**，而不是把历史推下去 —— 推进去会让历史区被压得
            //    很矮（旧快照里最后一条的分母被滚出可视区，看起来像排版坏了）；
            // 2. **点面板外的任意位置就关闭**（不必再点一次齿轮）。
            //
            // 做法与函数弹窗同一套：`stack!` + 一层接收点击的遮罩。
            let backdrop = iced::widget::mouse_area(
                container(Space::new().width(Length::Fill).height(Length::Fill)).style(|_t| {
                    container::Style {
                        // ⚠️ **全透明**（用户要求："不要遮盖全部界面，显示在哪就遮哪里"）。
                        //
                        // 这一层的职责只有一个：接住"点面板外任意处"这个点击，
                        // 让设置关掉。界面本身一点不该被压暗 —— 它显示在历史区那块，
                        // 视觉上也就该只影响那块。
                        background: Some(Background::Color(Color::TRANSPARENT)),
                        ..container::Style::default()
                    }
                }),
            )
            .on_press(Message::CloseSettings);

            // ⚠️ **高度必须是 Shrink**（用户反馈："我的意思不是遮住整个界面，
            // 是设置这个小框在最顶层，不是全部遮住啊"）。
            // 上一版写的是 `height(Length::Fill)`，于是 `settings_panel()` 那层
            // **带背景色**的容器被拉伸到整屏高 —— 面板下面拖出一大片同色空白，
            // 看起来就是把整个界面盖住了。
            let panel = container(self.settings_panel())
                .width(Length::Fill)
                .height(Length::Shrink)
                .padding([56.0, 22.0]);

            // ⚠️ **必须把内容区垫在最底下**（用户："我的意思不是遮住整个界面，
            // 是设置这个小框在最顶层，不是全部遮住啊"）。
            //
            // 上一版写的是 `stack![backdrop, panel]` —— 浮层**替代**了内容区，
            // 于是打开设置时工具条/历史/输入区/键盘**全都不画了**，只剩一块空白，
            // 看起来就是把整个界面盖住。我当时只改了高度，没发现根因在这里。
            stack![content_column, backdrop, panel].into()
        } else {
            // ⚠️ **单独包一层 `stack!`，哪怕只有一个孩子**。
            //
            // 用户实测："历史计算较多时，打开设置就会拉到最早的历史"。
            // 根因：两个浮层分支返回的是 `stack![content_column, ..]`，而这里
            // 直接返回内容区 —— 于是它在 widget 树里的**位置不同**（一个在 stack
            // 的 `children[0]`，一个就是根），iced 按位置匹配状态 ⇒ 滚动位置被判为
            // "新容器"而重置到顶部。
            //
            // 统一成"内容区永远是 stack 的第 0 个孩子"，状态就能跨开合保留下来。
            stack![content_column].into()
        };

        // ⚠️ **标题栏在浮层之外**：函数/设置浮层的透明遮罩铺满整块内容区
        //（用来"点空白处关闭"），如果标题栏也被它盖住，浮层一打开窗口就拖不动了。
        // 把标题栏拼在 stack **之上**，它永远可点（三个按钮、拖动都正常），
        // 浮层只在内容区里浮。
        let content: Element<'_, Message> = column![self.titlebar_area(), content]
            .height(Length::Fill)
            .into();

        // ── 缩放热区：铺在**所有东西之上**（连函数面板的边也能拖）──────────
        //
        // 最大化时不给：那时系统本来就不让拖边，留着热区只会把"贴边点击"
        // 白白吃掉 5px。
        let content: Element<'_, Message> = if self.maximized {
            content
        } else {
            stack![content, crate::chrome::resize_overlay(Message::Chrome)].into()
        };

        // 根容器负责"透出桌面"：亚克力成功时背景全透，失败时不透明。
        // 这一步**必须有** —— 窗口开了 transparent，若根容器不给背景，
        // 没成功启用亚克力时就会露出一片黑/花屏。
        // 最底层：**自绘模糊**的背景图（抓屏 + 模糊，见 `crate::backdrop`）。
        // 它必须垫在 `content` **之下** —— 上面的面板是半透明的，
        // 透过它们看到的就是这张图。抓不到时退回氛围底色。
        // ⚠️ **玻璃关掉就不叠这层**（用户实测："关闭后并没有完全消失，
        // 而是还有残留的透明"）。否则背景图会一直透出来，面板再实也挡不住。
        // ⚠️ 只看 `bg_pixels`，**不再要求 `self.bg` 一定存在**：
        // 玻璃模式走 shader，根本用不到 `image::Handle`（见 `BgCaptured`），
        // 那里刻意把它留成 `None` 来省掉一次图集上传。
        let layered: Element<'_, Message> = match &self.bg_pixels {
            Some((px, _cx, _cy, w, h, _, _)) if theme::texture() != theme::Texture::Off => {
                // 两种质感看到的是**同一张**模糊背景，但画法不同：
                // - 亚克力：普通贴图（磨砂玻璃，不做形变）
                // - 玻璃：走**折射 shader** —— 边缘被"压弯"，像一块凸透镜
                // 扭曲强度：玻璃要"边缘被压弯"（0.85），亚克力是磨砂、**不做形变**（0.0）。
                // 强度与 `liquid.rs` 共用同一个常量（`glass_wgsl::WIN_STRENGTH`）——
                // 两边一旦不同，"按键里透出的背景"与"窗口边缘的背景"就会是
                // 两种弯曲，按键边界处会露馅。
                let strength = if theme::texture() == theme::Texture::Glass {
                    crate::glass_wgsl::WIN_STRENGTH
                } else {
                    0.0
                };
                // 窗口→背景图的 uv 映射（拖动时随原点差变化，零抓屏跟手）
                let layer: Element<'_, Message> =
                    iced::widget::shader(crate::refraction::Refraction {
                        pixels: Arc::clone(px),
                        width: *w,
                        height: *h,
                        strength,
                        version: self.bg_version,
                        uv_scale: bg_map.0,
                        uv_off: bg_map.1,
                    })
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into();
                stack![layer, content].into()
            }
            _ => content,
        };

        container(layered)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::root)
            .into()
    }

    // ── ① 工具条 ────────────────────────────────────────────────────────────

    /// 给一块**面板内容**垫上液态玻璃（只在玻璃质感下；其余模式原样返回）。
    ///
    /// ⚠️ **玻璃必须垫在 container 外面**（而不是塞进 container 里面）：
    /// 面板的 padding 是 container 的，垫在里面就只能盖住 padding 以内的区域，
    /// 面板最外圈会露出一条没盖到的直角 —— 而 **折射的精华全在最外 30px 那一圈**，
    /// 盖不到就等于没做（用户要求："做好四周的折射"）。
    ///
    /// 垫在外面时要 `push_under`：让 container（base）决定整块的尺寸，
    /// 玻璃是 `Fill`，绘制顺序在 container **下面** ——
    /// 于是观感是"折射过的背景 → 面板底色 → 面板内容"。
    pub(super) fn glass_under<'a>(
        content: Element<'a, Message>,
        g: liquid::Glass,
    ) -> Element<'a, Message> {
        if theme::liquid_on() {
            Stack::new()
                .push(content)
                .push_under(liquid::layer(g))
                .into()
        } else {
            content
        }
    }

    pub(super) fn toolbar(&self) -> Element<'_, Message> {
        // ⚠️ 从 UI 侧副本读，而不是问 `Session` —— 求值器在别的线程上（见 `State::modes`）
        let disp = self.modes.display;
        let ang = self.modes.angle;
        let deep = self.modes.calc == CalcMode::Deep;

        // 工具条上的文字都是"拉丁/数学"标签（MathIO、RAD…），走数学字体；
        // 中文标签（快速/死算/清空）走等线。`theme::label` 自动按内容挑。
        //
        // ⚠️ **padding 必须给内容、不给 button**：
        // 玻璃层是 `.width/height(Fill)`，它撑的是 **button 的内容盒** ——
        // 而内容盒 = button 边界减去 padding。把 padding 留在 button 上，
        // 玻璃就会比按键窄一圈（四周露出没盖到的直角）。
        // 所以 button 自己 `.padding(0)`，padding 由里面的 `container` 给。
        // ⚠️ **下面这个必须是 `fn`，不能写成闭包**：闭包参数的生命周期会被推断成
        // **不变的**，而 `Button<'_, Message>` 对生命周期也是不变的 ——
        // 于是"传入 `Element<'_>` → 包进 `button(..)` → 返回"会报
        // `lifetime may not live long enough`。写成 `fn` 就能显式标出 `'a`。
        //
        // ⚠️ **padding 在这里给，不给 button**：液态玻璃是 `.width/height(Fill)` 的
        // 着色器层，在 `Stack` 里它必须排在**文字之后**才能由文字决定尺寸（见下）。
        // 而文字外还得留一圈内边距 —— 所以 padding 包在文字外面、玻璃垫在整块下面，
        // 玻璃才盖得到 chip 的全部面积（含内边距）。
        fn chip<'a>(content: Element<'a, Message>, on: bool, msg: Message) -> Element<'a, Message> {
            let padded: Element<'a, Message> = container(content).padding([5, 12]).into();
            // ⚠️ **玻璃层必须 `push_under`，不能写成 `stack![glass, 文字]`**：
            // `Stack` 的 `base_layer` 默认是 **0**，尺寸取自**第一个**子元素 ——
            // 而玻璃是 `Fill`，放进第 0 位就等于"把 chip 撑满可用空间"，
            // 实际效果是工具条上的选项被拉成 180px 高的大板子（第一版快照的惨状）。
            // `push_under` 把玻璃插到第 0 位并把 `base_layer` 加 1 ⇒
            // 尺寸由文字决定、玻璃铺满文字那一圈、绘制顺序在文字**下面**。
            let body: Element<'a, Message> = if theme::liquid_on() {
                Stack::new()
                    .push(padded)
                    .push_under(liquid::layer(theme::chip_glass(
                        on,
                        theme::CHIP_RADIUS_GLASS,
                    )))
                    .into()
            } else {
                padded
            };
            button(body)
                .padding(0)
                .style(move |t, s| theme::mode_chip(t, s, on))
                .on_press(msg)
                .into()
        }
        // `seg` 收 `&'static str`（字面量，无需翻译的那些：MathIO / RAD / DEG）；
        // `seg_t` 收**已翻译的 `String`** —— 界面里会跟随语言变化的那几个标签用它。
        let seg = |label: &'static str, on: bool, msg: Message| {
            chip(theme::label(label).size(14).into(), on, msg)
        };
        let seg_t = |label: String, on: bool, msg: Message| {
            chip(theme::label(&label).size(14).into(), on, msg)
        };

        // 图标按钮：固定最小宽度，否则 `☾` / `⚙` 这类单字符按钮会缩成很小的方块，
        // 点在它旁边一点就落空（第一版快照里它们明显偏小偏挤）
        let icon = |label: &'static str, on: bool, msg: Message| {
            chip(
                text(label).size(15).font(theme::font_math()).into(),
                on,
                msg,
            )
        };

        // ⚠️ 这里**不再**画 "HiPerCalc" 大标题：自绘标题栏已经写了应用名
        //（用户 2026-10-26 加了窗口栏），工具条上再写一遍就是同一个名字出现两次。
        // 工具条只放"算之前随手切"的模式。
        let mut r = Row::new().spacing(6).align_y(Alignment::Center);
        // 显示模式
        r = r.push(seg(
            "MathIO",
            disp == DisplayMode::MathIO,
            Message::SetDisplay(DisplayMode::MathIO),
        ));
        r = r.push(seg(
            "LineIO",
            disp == DisplayMode::LineIO,
            Message::SetDisplay(DisplayMode::LineIO),
        ));
        r = r.push(Space::new().width(Pixels(8.0)));
        // 角度模式
        r = r.push(seg(
            "RAD",
            ang == AngleMode::Radian,
            Message::SetAngle(AngleMode::Radian),
        ));
        r = r.push(seg(
            "DEG",
            ang == AngleMode::Degree,
            Message::SetAngle(AngleMode::Degree),
        ));
        r = r.push(Space::new().width(Pixels(8.0)));
        // 计算模式。⚠️ 这两个是**会翻译的**界面标签（不带 `&'static str`）：
        // `seg` 的签名收 `&'static str`，所以这里改成用 `theme::label(t(..))`
        // 直接构造，见下面 `clear` 的写法。
        r = r.push(seg_t(t("快速"), !deep, Message::SetCalc(false)));
        r = r.push(seg_t(t("死算"), deep, Message::SetCalc(true)));
        r = r.push(Space::new().width(Length::Fill));
        // 设置 + 清空。
        //
        // ⚠️ 明暗切换按钮**不在这里**了 —— 用户要求"深色浅色的按钮放到设置里"。
        // 主题属于"偶尔改一次"的设置，和 MathIO/RAD/快速 这些"算之前随手切"的
        // 模式不是一类东西，混在工具条上会让一排 chip 越长越难扫。
        // 命令行版：起一个**独立进程**（GUI 关掉也不影响它）。
        // 放在设置旁边，`>_` 是终端提示符的通用记号，比文字更省宽度。
        r = r.push(icon(">_", false, Message::LaunchCli));
        r = r.push(icon("⚙", self.show_settings, Message::ToggleSettings));
        // 三角形求解（用户 2026-10-27：解三角形要独立界面，别让人对着
        // `triangle(...)` 猜参数）。点了就把结果区切成表单页。
        r = r.push(seg_t(
            t("三角形求解"),
            self.triangle_open,
            Message::ToggleTriangle,
        ));
        // 清空历史：这条**必须**翻译（工具条上最显眼的中文之一）。
        // 注意键用的是"清空历史"而不是"清空" —— 后者在 CLI 里可能被当成别的片段。
        r = r.push(seg_t(t("清空历史"), false, Message::ClearHistory));

        Self::glass_under(
            container(r)
                .padding([8, 12])
                .width(Length::Fill)
                .style(theme::panel)
                .into(),
            theme::panel_glass(),
        )
    }

    // ── ② 结果区 ────────────────────────────────────────────────────────────

    /// 输入区：编辑器 + C / = 按钮 + **实时数学预览**。
    ///
    /// ## 为什么是"编辑器 + 预览"两行，而不是把编辑器本身变成数学排版
    ///
    /// 用户的要求原话是"输入栏里根号应当渲染，类似 desmos"。这里有一个**硬约束**
    /// 必须先说清楚，因为它决定了做法：
    ///
    /// - iced 的 `text_editor` 自己排版自己那串文本，**没有**"在这里画一个根号"
    ///   的扩展点。它唯一开放的钩子是 `Highlighter`，而
    ///   `iced_core::text::highlighter::Format` **只有 `color` 与 `font` 两个字段**
    ///   （没有字号、没有自绘回调）⇒ 它能改颜色和字体，**画不出根号的顶线**。
    /// - 要实现 Desmos 那种"光标能落在根号里面"的真正内联排版，得自己写一个
    ///   完整的文本编辑器（排版、光标、选区、命中测试全都要自己算）。
    ///
    /// 所以这里采取**分层**做法，两层各司其职：
    ///
    /// 1. **上面一行**是真正的 `text_editor`：打字、光标、选区、多行都在这里，
    ///    行为完全可靠（光标永远在它该在的地方）。
    /// 2. **下面一行**是 [`mathrender`] 排出来的**实时预览**：输入一变化就重排，
    ///    `sqrt(2)/3` 立刻显示成"根号 2 分之 3"。这就是用户要的"根号要渲染"。
    ///
    /// 预览行只在**输入非空**时出现，且高度与主行分开 —— 不会挤动下面的键盘。
    pub(super) fn input_area(&self) -> Element<'_, Message> {
        // ⚠️ `text_editor` 的 `.width()` 只接受 `Pixels`（不像别的控件收 `Length`），
        // **没法声明 `Fill`**。而它的自然行为是"尽量占满"，在 `Row` 里会和右侧
        // 两个定宽按钮**争宽度并撑破容器** —— 实测症状是 `C` / `=` 被推到窗口外，
        // 只剩色块露在右边缘（截图上文字全看不见）。
        //
        // 解法：把编辑器放进一个 `FillPortion` 的槽位，让它有**明确的上限**
        // —— 槽位拿到 `Row` 分配的空间后，`text_editor` 在里面填满，不再向外膨胀。
        let src = self.input_text();

        // ── ② 用户真正**看得见**的数学排版 ─────────────────────────────────
        //
        // ⚠️ 这一层**就是输入栏本身**，不再是"预览"：结构化之后树既能排版、
        // 又能精确说出"每个可点目标 ⇒ 光标落在哪一位"，所以不需要
        // "文本编辑器 + 预览"那套两层结构了。
        let shown: Element<'_, Message> = if src.is_empty() {
            // ⚠️ **空输入也要画光标**（用户反馈："没有输入时点击输入栏不会出现
            // 光标"）。以前空着只画提示语，光标是 `Node` 树的一部分、树是空的，
            // 于是"看不见光标"—— 用户根本不知道可以开始打字。
            row![
                mathrender::caret(theme::text_size(18.0)),
                theme::label(t("输入表达式，例如 sqrt(2)/3、x^2-4=0、int(x^2,x,0,1)"))
                    .size(17)
                    .style(theme::hint),
            ]
            .spacing(3)
            .align_y(Alignment::Center)
            .into()
        } else {
            // 树 → 渲染 IR。光标作为一根**真实参与布局的竖线**插进去，
            // 位置由 iced 算 —— 不需要猜任何字体的字形宽度。
            //
            // 第二个返回值是"可点目标表"，`Node::Hit` / `Node::Empty` 里的
            // `at` 就是它的下标 ⇒ 点它就把光标送到那个位置。
            let (node, _targets) = self.input.to_node();
            mathrender::build(
                &node,
                mathrender::Style2 {
                    size: theme::text_size(18.0),
                    color: Some(theme::colors().text),
                    wrap: true,
                    // 输入栏的函数名用斜体（用户 2026-10-24："函数都用黑色斜体字，
                    // 与输入栏字体相同"—— 输入栏就是那个基准）
                    italic_names: true,
                },
                // 点空槽位框 / 点某个记号 ⇒ 光标落在那里（用户要求）
                Some(&|id| Message::PlaceCaret(id)),
            )
        };

        // `stack!` 把两层叠起来，高度取两者较大的 —— 所以分数/根号把排版撑高时
        // 输入区会跟着长高，而不是被 `INPUT_HEIGHT` 裁掉。
        //
        // ⚠️ **顺序很关键：后写的在上面**。必须把**排版层放在 editor 之上**。
        //
        // 反过来的话，透明的 editor 会把所有鼠标点击都吃掉 —— 它内部的定位是按
        // **纯文本字形**算的（`√2` 的宽度和 `sqrt(2)` 差得远），点出来的位置必然偏。
        // 用户的原话是"完全无法选中修改，一选中就变成了文本字母"。
        // 排版层在上之后，点击由我们自己的 `Node::At` 处理（位置精确到源字符偏移），
        // editor 只负责它真正不可替代的那件事：键盘、输入法、粘贴、框选。
        //
        // 外层的 `mouse_area` 是**兜底**：点在排版层的空白处（没有 `At` 覆盖的地方）
        // 时不移动光标，但至少要把焦点还给 editor，否则用户接着打字会没反应。
        // 排版层直接就是输入栏
        let editor_slot = container(shown)
            .width(Length::FillPortion(10))
            .padding([12, 14])
            .align_x(Alignment::Start);

        // `=` 与 `C` 给**固定宽高**：两者不等宽（56 / 46）是有意的，让 `=` 更醒目。
        //
        // ⚠️ **高度必须显式给**。第一版只给了宽度，`Row` 的交叉轴分配让按钮被压成
        // 一条 22px 高的细线（截图上只看得到色块，看不到文字）——
        // 而 editor 那边有 `Height::Fixed(56)` 撑着，落差全由按钮承担了。
        // 实测：宽度对、高度塌，是要单独看像素才能发现的一类问题。
        //
        // 计算中时给 `=` 去掉 `on_press`（等价于禁用）：不然用户会一直点，
        // 而 `evaluate` 里那条 `busy` 守卫会把后续点击全部静默丢掉 ——
        // 用户看到的是"点了没反应"。按钮画成不可按，意图就清楚了。
        // 同时把字形换成 `…`，与历史区那条「计算中…」互相印证。
        // 计算中：`=` 变成**取消**（用户要求 2026-10-23："输入栏的 = 按钮也变成取消"）。
        // 不再是"画成不可按"—— 那样用户只能干等。
        let go = if self.busy {
            button(text(t("取消")).size(15).font(crate::fonts::font_cjk()))
                .width(Length::Fixed(56.0))
                .height(Length::Fixed(INPUT_HEIGHT))
                .style(|t, s| theme::key(t, KeyKind::Control, s))
                .on_press(Message::Cancel)
        } else {
            button(text("=").size(22).font(theme::font_math()))
                .width(Length::Fixed(56.0))
                .height(Length::Fixed(INPUT_HEIGHT))
                .style(|t, s| theme::key(t, KeyKind::Equals, s))
                .on_press(Message::Evaluate)
        };

        let clear = button(text("C").size(18).font(theme::font_math()))
            .width(Length::Fixed(46.0))
            .height(Length::Fixed(INPUT_HEIGHT))
            .style(|t, s| theme::key(t, KeyKind::Control, s))
            .on_press(Message::ClearInput);

        let row1 = row![editor_slot, clear, go]
            .spacing(8)
            .align_y(Alignment::Center);

        // "式子没写完"之类的**行内提示**（不占历史、不染红整块）
        let hint: Element<'_, Message> = match &self.input_hint {
            Some(h) => container(theme::label(h.as_str()).size(13).style(theme::danger_text))
                .padding([2, 14])
                .into(),
            None => Space::new().height(Length::Shrink).into(),
        };

        // ⚠️ **点在空白处 = 光标到末尾**。
        //
        // 字符级点击目标只能表达"落在某个记号**之前**"，最后一个记号**之后**
        // 那个位置没有任何目标可点 —— 用户实测："光标在 4|5，无论鼠标点哪里
        // 都无法移到 5 后面"。所以整块输入栏再包一层 `mouse_area` 兜底：
        // 内层目标会先捕获事件（`mouse_area` 只在 `!is_event_captured` 时才动作），
        // 只有落在空白处才轮到这里。
        let body: Element<'_, Message> = iced::widget::mouse_area(
            container(column![row1, hint])
                .width(Length::Fill)
                .padding(8)
                .style(theme::panel),
        )
        .on_press(Message::PlaceCaretEnd)
        .into();

        // 输入区只剩一行：排版层本身就是"输入栏"（用户要求），
        // 不再另起一条"预览"—— 那是旧方案（编辑器可见 + 预览在下）的产物。
        Self::glass_under(body, theme::panel_glass())
    }

    // ── ④ 虚拟键盘区 ─────────────────────────────────────────────────────────

    /// 键盘上方那行小字："键盘" + 当前面板标签 + 函数弹窗按钮。
    ///
    /// Desmos 把 `functions` 放在键盘**网格内部**（右侧一列）。我们两种都保留了：
    /// 网格内的 `ƒ` 键是照 Desmos 的布局放的，这里这个按钮则是"一眼能看见"的入口
    /// —— 函数面板是本项目的主力功能，藏在一个小键里不好找。
    pub(super) fn keypad_header(&self) -> Element<'_, Message> {
        let func_btn = button(
            row![
                text("ƒ").size(17).font(theme::font_math()),
                theme::label(t("展开函数面板")).size(14),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
        )
        .padding([7, 14])
        .style(move |t, s| theme::mode_chip(t, s, self.show_funcs))
        .on_press(Message::ToggleFuncs);

        // ⚠️ 键用的是**更长的键名**（"主键盘" / "字母键盘"），不是裸的"键盘" / "字母"：
        // 裸键太短、又是常用词，放词典里容易在别处的输出文本里被误匹配
        //（`t()` 是"子串最长优先"，短键会在任意句子里到处替换）。
        let panel_name = match self.panel {
            Panel::Main => t("主键盘"),
            Panel::Abc => t("字母键盘"),
        };

        row![
            theme::label(t("虚拟键盘")).size(13).style(theme::hint),
            text("·").size(13).style(theme::hint),
            text(panel_name)
                .size(13)
                .font(theme::font_cjk())
                .style(theme::accent_text),
            Space::new().width(Length::Fill),
            func_btn,
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into()
    }

    /// 一个键面 widget。
    ///
    /// Desmos 的键面规律（照搬）：**记号用数学字体、字母数字用普通字体**，
    /// 且所有键面都居中、字号统一。数学记号交给 [`crate::theme::font_math`]，
    /// 否则 `π` / `√` / `≤` 用普通字体排出来又矮又瘦，与旁边的数字不搭。
    pub(super) fn key_face<'a>(&self, label: &'a str, kind: KeyKind) -> Element<'a, Message> {
        // `a²` / `aᵇ` 这种"字母 + 上标"要拆开排：上标字号更小、位置更高，
        // 直接当普通文本排会得到一个等高的 `a²`，完全不像幂。
        if let Some((base, sup)) = split_superscript(label) {
            return row![
                text(base.to_string())
                    .size(KEY_FACE_SIZE)
                    .font(theme::font_math_italic()),
                text(sup.to_string())
                    .size(KEY_FACE_SIZE * 0.68)
                    .font(theme::font_math()),
            ]
            .spacing(1)
            .align_y(Alignment::Start)
            .into();
        }

        // ⚠️ **所有键面统一用数学字体**（用户反馈："虚拟键盘和计算栏的字母和符号
        // 并没有使用数学字体，而是等线"）。
        //
        // 以前只给 `label_is_math` 认得的符号（π √ ≤ …）设数学字体，字母数字
        // （`x` `7` `ABC`）**不设** ⇒ 落到 iced 默认字体上，与旁边的 `π` 不是一套。
        //
        // ⚠️ **例外**：`⌫`（U+232B）在数学字体里**没有字形**，硬套会变成豆腐块
        //（实测）。这类"UI 图标符号"不是数学记号，保持默认字体即可。
        //
        // ⚠️ **键面与输入栏必须同一条规则**（用户反馈："输入栏的字母是斜体，
        // 但是虚拟键盘不是，请将虚拟键盘和输入栏字体同步"）：
        // 输入栏里单个字母是**变量**（斜体，见 `mathrender::is_variable`），
        // 所以键面上的 `x` / `e` / `a` 也必须斜体，否则同一个字母在两处是两种样子。
        // 多字母（`ABC`）与数字仍然直立。
        let _ = kind;
        let face = text(label.to_string()).size(theme::text_size(KEY_FACE_SIZE));
        let face = if UI_ICON_KEYS.contains(&label) {
            face
        } else if mathrender::looks_like_variable(label) {
            face.font(theme::font_math_italic())
        } else {
            face.font(theme::font_math())
        };
        face.into()
    }

    pub(super) fn keypad_area(&self) -> Element<'_, Message> {
        let layout = layout_for(self.panel);
        // 计算中要禁用「↵」—— 闭包里不能借用 `self`，先取出来
        let busy = self.busy;
        let busy_blocks = move |action: KeyAction| busy && matches!(action, KeyAction::Evaluate);

        let mut rows = Column::new().spacing(KEY_GAP).width(Length::Fill);
        for (ri, line) in layout.iter().enumerate() {
            let mut r = Row::new().spacing(KEY_GAP).align_y(Alignment::Center);
            for (ci, k) in line.iter().enumerate() {
                // 加宽的键：`↵`（求值）与 `⌫`（退格）—— Desmos 里这两个都是双宽。
                // 求值键尤其要点得准，它是最常用的键。
                let w = match k.action {
                    KeyAction::Evaluate | KeyAction::Backspace => Length::FillPortion(2),
                    // `⌫` 在字母面板的第 3 行也是双宽
                    KeyAction::ShowMain | KeyAction::ShowAbc => Length::FillPortion(2),
                    _ => Length::FillPortion(1),
                };
                let face = self.key_face(k.label, k.kind);
                // 键面四向居中（容器宽高都填满）—— 见下面 `.center_x/.center_y` 的注释
                let face = container(face)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill);
                // 玻璃质感下，键面底下垫一层**液态玻璃**（见 `crate::liquid`）。
                // 它只管外观；尺寸与 hover 由着色器自己从 bounds / cursor 取。
                let inner: Element<'_, Message> = if theme::liquid_on() {
                    stack![
                        liquid::layer(theme::key_glass(k.kind, theme::KEY_RADIUS_GLASS)),
                        face
                    ]
                    .into()
                } else {
                    face.into()
                };
                r = r.push(
                    // ⚠️ **`button` 的内容默认贴在左上角**，不是居中。
                    // `iced_widget::button` 的 `layout` 走 `layout::padded`，
                    // 而 `padded` 把内容 `move_to((padding.left, padding.top))` ——
                    // 完全没有垂直居中的步骤（button 也没有 `center_y` / `vertical_alignment`
                    // 这类 API，翻遍 `button.rs` 只有 `width` / `height` / `padding`）。
                    //
                    // 所以只看宽度的 `container(face).width(Fill).center_x(Fill)` 会让键面
                    // **偏上**：容器撑满宽度但高度只有文字高，button 把它钉在顶部。
                    // 修法：容器**宽高都填满**并四向居中，内容才会落在键面正中心。
                    button(inner)
                        .width(w)
                        .height(Length::Fixed(KEY_HEIGHT))
                        // ⚠️ **padding 必须是 0**：玻璃层撑的是 button 的**内容盒**
                        //（= 边界 - padding），留 padding 玻璃就盖不到按键边缘。
                        // 键面本来就是居中的，不需要靠 padding 定位。
                        .padding(0)
                        .style(move |t, s| theme::key(t, k.kind, s))
                        // 计算中把「↵」置灰（去掉 `on_press` ⇒ `Status::Disabled`
                        // ⇒ `theme::key` 自动降到 30% 透明）。用户要求："虚拟键盘的
                        // enter 变成灰色"。
                        .on_press_maybe(if busy_blocks(k.action) {
                            None
                        } else {
                            Some(Message::Key(ri, ci))
                        }),
                );
            }
            rows = rows.push(r);
        }

        Self::glass_under(
            container(
                column![
                    self.keypad_header(),
                    rule::horizontal(1.0).style(theme::divider),
                    rows
                ]
                .spacing(8),
            )
            .width(Length::Fill)
            .padding(10)
            .style(theme::panel_low)
            .into(),
            theme::panel_glass(),
        )
    }

    // ── 函数弹窗 ────────────────────────────────────────────────────────────
}
