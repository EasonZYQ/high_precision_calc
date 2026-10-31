//! **自绘窗口栏**：标题栏（拖动 / 双击最大化）+ 最小化·最大化·关闭 + 8 向缩放边。
//!
//! ## 为什么自己画
//!
//! 用户 2026-10-26："程序窗口与程序的现代化界面严重割裂"。系统标题栏是 Win32
//! 的灰条 + 直角边框，跟这套玻璃界面是两种语言。所以窗口改成
//! `decorations: false`，标题栏、按钮、缩放边全部由这里提供。
//!
//! ## 三件系统行为怎么补回来
//!
//! | 行为 | 手段 |
//! | --- | --- |
//! | 拖动窗口 | `iced::window::drag`（按下标题栏就进入系统移动循环） |
//! | 缩放窗口 | 8 条边/角 → `iced::window::drag_resize(direction)` |
//! | 最小化 / 最大化 / 关闭 | `window::minimize` / `maximize` / `close` |
//!
//! 缩放边用自定义 widget 而不是 `mouse_area`：**光标形状**要靠
//! `Widget::mouse_interaction` 上报（`ResizingHorizontally` 等），`mouse_area`
//! 不上报光标，鼠标贴到边上还是箭头 —— 用户根本不知道能拖。
//!
//! ## 结构
//!
//! - `titlebar`：一行标题（logo + 名字 + 副标题）+ 三个按钮，整行可拖动；
//! - `resize_overlay`：铺在整窗之上的 8 条热区（只占边缘几像素，中间不挡事件）；
//! - `ChromeMessage`：本模块只产这几种意图，转成应用消息由 `app.rs` 决定。

use iced::widget::{button, canvas, container, mouse_area, stack};
use iced::{
    Alignment, Element, Length, Point, Rectangle, Renderer, Theme, mouse, widget::canvas::Frame,
    widget::canvas::Path, widget::canvas::Stroke, window,
};

/// 窗口栏产生的意图（`app.rs` 负责转成自己的消息）。
#[derive(Debug, Clone, Copy)]
pub enum ChromeMessage {
    /// 开始拖动窗口（标题栏按下）
    Drag,
    /// 开始缩放窗口（方向由 8 条边/角决定）
    Resize(window::Direction),
    /// 最小化
    Minimize,
    /// 最大化 / 还原（按钮与双击标题栏都走这条）
    ToggleMax,
    /// 关闭
    Close,
}

/// 标题栏高度（逻辑像素）。缩放边与 iced 的布局都按它对齐。
pub const TITLEBAR_H: f32 = 34.0;

/// 三个标题栏按钮的宽度。
const CAPTION_W: f32 = 46.0;

/// 缩放热区厚度（px）。太薄不好抓，太厚会挡住面板边缘的点击。
const EDGE: f32 = 5.0;

/// 角落热区边长（比边厚一倍，斜向拖动更好抓）。
const CORNER: f32 = 12.0;

/// 标题栏。
///
/// - `title` / `subtitle`：名字与副标题（副标题用 hint 色，可留空）；
/// - `maximized`：决定第三个按钮画"最大化"还是"还原"；
/// - `to`：把 `ChromeMessage` 映射成应用自己的消息类型。
///
/// 整行包在 `mouse_area` 里：按下 = 拖动，双击 = 最大化/还原。
/// ⚠️ 按钮是它的**子 widget**，点击会被按钮先捕获，所以点按钮不会误触发拖动
/// （iced 的事件分发是"子先父后"，捕获了就停）。
pub fn titlebar<'a, M>(
    title: String,
    subtitle: String,
    maximized: bool,
    to: impl Fn(ChromeMessage) -> M + Copy + 'a,
) -> Element<'a, M>
where
    M: Clone + 'a,
{
    let name = crate::theme::font_math();
    let brand = iced::widget::row![
        iced::widget::text("ƒ")
            .size(17)
            .font(name)
            .style(crate::theme::accent_text),
        iced::widget::text(title).size(13).font(name),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    let mut bar = iced::widget::row![brand]
        .spacing(10)
        .align_y(Alignment::Center)
        .padding(iced::Padding {
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
            left: 14.0,
        });

    if !subtitle.is_empty() {
        bar = bar.push(
            iced::widget::text(subtitle)
                .size(11)
                .font(crate::theme::font_cjk())
                .style(crate::theme::hint),
        );
    }

    // 中间留白：既是视觉呼吸，也是拖动的主体区域
    bar = bar.push(iced::widget::Space::new().width(Length::Fill));
    // 三个按钮：最小化 / 最大化·还原 / 关闭
    bar = bar
        .push(caption_button(
            CaptionIcon::Minimize,
            false,
            to(ChromeMessage::Minimize),
            to,
        ))
        .push(caption_button(
            if maximized {
                CaptionIcon::Restore
            } else {
                CaptionIcon::Maximize
            },
            false,
            to(ChromeMessage::ToggleMax),
            to,
        ))
        .push(caption_button(
            CaptionIcon::Close,
            true,
            to(ChromeMessage::Close),
            to,
        ));

    // 整行：按下拖动 + 双击最大化
    mouse_area(container(bar).height(Length::Fixed(TITLEBAR_H)))
        .on_press(to(ChromeMessage::Drag))
        .on_double_click(to(ChromeMessage::ToggleMax))
        .into()
}

/// 一个标题栏按钮（背景 hover 由 `theme::caption` 给）。
fn caption_button<'a, M>(
    icon: CaptionIcon,
    close: bool,
    on_press: M,
    _to: impl Fn(ChromeMessage) -> M + Copy + 'a,
) -> Element<'a, M>
where
    M: Clone + 'a,
{
    button(
        canvas(icon)
            .width(Length::Fixed(CAPTION_W))
            .height(Length::Fixed(TITLEBAR_H)),
    )
    .width(Length::Fixed(CAPTION_W))
    .height(Length::Fixed(TITLEBAR_H))
    .padding(0)
    // 关闭按钮的 hover 底色要跟着窗口圆角拐一下，否则右上角会戳出直角
    .style(move |t, s| crate::theme::caption(t, s, close))
    .on_press(on_press)
    .into()
}

/// 8 条缩放热区，铺满整窗（**只占边缘**，中间完全透明、不挡事件）。
///
/// 用法：`stack![内容, 浮层, resize_overlay(..)]` —— 必须放在**最后一个**
/// （iced 的 `stack` 事件是**从后往前**分发的，最后一个先拿到）。
/// 最大化时调用方不要把它塞进 `stack`：那时不该能拖边。
pub fn resize_overlay<'a, M>(to: impl Fn(ChromeMessage) -> M + Copy + 'a) -> Element<'a, M>
where
    M: Clone + 'a,
{
    use window::Direction as D;

    // 顺序：四条边 → 四个角（后面的在上层，角优先命中）。
    // 热区自己的尺寸由 `Handle::lengths` 给（边是 Fill、角是固定方块），
    // 这里只需要"贴到哪一边"。
    let mut layers: Vec<Element<'a, M>> = Vec::with_capacity(8);
    for (dir, ax, ay) in [
        (D::North, Alignment::Center, Alignment::Start),
        (D::South, Alignment::Center, Alignment::End),
        (D::West, Alignment::Start, Alignment::Center),
        (D::East, Alignment::End, Alignment::Center),
        (D::NorthWest, Alignment::Start, Alignment::Start),
        (D::NorthEast, Alignment::End, Alignment::Start),
        (D::SouthWest, Alignment::Start, Alignment::End),
        (D::SouthEast, Alignment::End, Alignment::End),
    ] {
        layers.push(
            container(Element::new(Handle::new(
                dir,
                to(ChromeMessage::Resize(dir)),
            )))
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(ax)
            .align_y(ay)
            .into(),
        );
    }
    stack(layers).into()
}

/// 窗口缩放热区：**不画任何东西**，只做两件事 —— 按下发消息、上报光标形状。
struct Handle<M> {
    dir: window::Direction,
    msg: M,
}

impl<M> Handle<M> {
    fn new(dir: window::Direction, msg: M) -> Self {
        Self { dir, msg }
    }

    /// 热区在"沿边方向"要不要拉满（边是 Fill，角是固定方块）
    fn lengths(&self) -> iced::Size<Length> {
        use window::Direction as D;
        match self.dir {
            D::North | D::South => iced::Size::new(Length::Fill, Length::Fixed(EDGE)),
            D::East | D::West => iced::Size::new(Length::Fixed(EDGE), Length::Fill),
            _ => iced::Size::new(Length::Fixed(CORNER), Length::Fixed(CORNER)),
        }
    }
}

impl<M, Theme, Renderer> iced::advanced::Widget<M, Theme, Renderer> for Handle<M>
where
    M: Clone,
    Renderer: iced::advanced::Renderer,
{
    fn size(&self) -> iced::Size<Length> {
        self.lengths()
    }

    fn layout(
        &mut self,
        _tree: &mut iced::advanced::widget::Tree,
        _renderer: &Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        let l = self.lengths();
        iced::advanced::layout::Node::new(limits.resolve(l.width, l.height, iced::Size::ZERO))
    }

    fn draw(
        &self,
        _tree: &iced::advanced::widget::Tree,
        _renderer: &mut Renderer,
        _theme: &Theme,
        _style: &iced::advanced::renderer::Style,
        _layout: iced::advanced::Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        // 完全透明：热区不该有任何视觉痕迹
    }

    fn update(
        &mut self,
        _tree: &mut iced::advanced::widget::Tree,
        event: &iced::Event,
        layout: iced::advanced::Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut iced::advanced::Shell<'_, M>,
        _viewport: &Rectangle,
    ) {
        // 只在**左键按下**且光标在热区内时发一条：`app` 侧收到后调
        // `window::drag_resize`，系统接管后续拖动（我们不用跟踪 mousemove）。
        if let iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
            && cursor.is_over(layout.bounds())
        {
            shell.publish(self.msg.clone());
            shell.capture_event();
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        use window::Direction as D;
        if !cursor.is_over(layout.bounds()) {
            return mouse::Interaction::None;
        }
        match self.dir {
            D::North | D::South => mouse::Interaction::ResizingVertically,
            D::East | D::West => mouse::Interaction::ResizingHorizontally,
            // ↖↘ 一根斜线、↗↙ 另一根 —— 与系统缩放光标的方向一致
            D::NorthWest | D::SouthEast => mouse::Interaction::ResizingDiagonallyDown,
            D::NorthEast | D::SouthWest => mouse::Interaction::ResizingDiagonallyUp,
        }
    }
}

/// 三个按钮的图标（矢量绘制，不依赖字体里有没有 `⧉` 这种冷门字形）。
#[derive(Debug, Clone, Copy)]
pub enum CaptionIcon {
    Minimize,
    Maximize,
    Restore,
    Close,
}

impl<M> canvas::Program<M> for CaptionIcon {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let c = crate::theme::colors().text_dim;
        let stroke = Stroke::default().with_color(c).with_width(1.15);
        // 图标边长 10px，居中
        let s = 10.0;
        let (cx, cy) = (bounds.width / 2.0, bounds.height / 2.0);
        let (l, t) = (cx - s / 2.0, cy - s / 2.0);
        match self {
            CaptionIcon::Minimize => {
                frame.stroke(
                    &Path::line(Point::new(l, cy), Point::new(l + s, cy)),
                    stroke,
                );
            }
            CaptionIcon::Maximize => {
                let rect = Path::rectangle(Point::new(l, t), iced::Size::new(s, s));
                frame.stroke(&rect, stroke);
            }
            CaptionIcon::Restore => {
                // 两个错开的小方块：前面一个实边框、后面一个只画露出来的两条边
                let a = Path::rectangle(Point::new(l, t + 2.0), iced::Size::new(s - 2.0, s - 2.0));
                frame.stroke(&a, stroke);
                let back = Path::new(|p| {
                    p.move_to(Point::new(l + 2.0, t));
                    p.line_to(Point::new(l + s, t));
                    p.line_to(Point::new(l + s, t + s - 2.0));
                });
                frame.stroke(&back, stroke);
            }
            CaptionIcon::Close => {
                let a = Path::line(Point::new(l, t), Point::new(l + s, t + s));
                let b = Path::line(Point::new(l + s, t), Point::new(l, t + s));
                frame.stroke(&a, stroke);
                frame.stroke(&b, stroke);
            }
        }
        vec![frame.into_geometry()]
    }
}
