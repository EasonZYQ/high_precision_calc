//! **三角形求解的独立面板页**（用户 2026-10-27 要求）。
//!
//! 为什么单开一页：命令行里它是 `triangle(a=3 b=4 c=5)`，但在界面上"函数式输入"
//! 会让"三角形"这三个字变得含糊（是求面积？求某个角？）。这里给一张明确的表：
//! 三条边 + 三个角，填任意三个已知量（至少一条边）就能解。
//!
//! ⚠️ **不改键盘区**：用户明确"不占用键盘区"，所以这一页显示在**结果区**，
//! 由工具条上的「三角形」开关切换；关掉就回到历史列表。

use iced::widget::column;

use super::*;

/// 六个字段的键：小写 = 边、大写 = 角（与 `triangle(a=3, A=30)` 的约定一致）
pub(super) const TRI_KEYS: [&str; 6] = ["a", "b", "c", "A", "B", "C"];

impl State {
    /// 结果区里的「三角形求解」页
    pub(super) fn triangle_page(&self) -> Element<'_, Message> {
        let mut body = Column::new().spacing(10).width(Length::Fill);

        body = body.push(
            theme::label(t("填任意三个已知量（至少一条边），点求解"))
                .size(13)
                .style(theme::hint),
        );

        // 两行三列：第一行三条边、第二行三个角
        for (row_i, title) in [t("边"), t("角")].iter().enumerate() {
            let mut r = Row::new().spacing(10).align_y(Alignment::Center);
            for col in 0..3 {
                let i = row_i * 3 + col;
                r = r.push(
                    Row::new()
                        .spacing(4)
                        .align_y(Alignment::Center)
                        .push(theme::label(TRI_KEYS[i].to_string()).size(14))
                        .push(
                            text_input("", &self.tri_vals[i])
                                .on_input(move |v| Message::TriChanged(i, v))
                                .width(Length::Fixed(96.0))
                                .size(13)
                                .padding([5, 8])
                                .font(theme::font_math())
                                .style(theme::field),
                        ),
                );
            }
            body = body.push(
                column![theme::label(title.clone()).size(12).style(theme::hint), r,].spacing(4),
            );
        }

        body = body.push(
            Row::new()
                .spacing(8)
                .align_y(Alignment::Center)
                .push(
                    button(theme::label(t("求解")).size(13))
                        .padding([5, 14])
                        .style(|t, s| theme::key(t, KeyKind::Equals, s))
                        .on_press(Message::TriSolve),
                )
                .push(
                    button(theme::label(t("清空")).size(12))
                        .padding([4, 10])
                        .style(|t, s| theme::mode_chip(t, s, false))
                        .on_press(Message::TriClear),
                ),
        );

        let inner = scrollable(container(body).padding([12, 12]))
            .height(Length::Fill)
            .width(Length::Fill);

        Self::glass_under(
            container(inner)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(6)
                .style(theme::panel_card)
                .into(),
            theme::panel_glass(),
        )
    }
}
