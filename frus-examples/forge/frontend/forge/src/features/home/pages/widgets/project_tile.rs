use frus::{
    Align, BuildContext, Callback, Color, Column, Container, Expanded, FontWeight, GestureDetector,
    Justify, Positioned, Row, Stack, StatelessWidget, Widget,
};

use crate::{
    common::{
        colors::{black, gray, grey, light_gray},
        ui::{accent_card, filled, text, FULL},
    },
    features::home::pages::widgets::member_avatar::MemberAvatar,
};

/// A project on the dashboard: its name, its figures, its progress and its members.
pub(crate) struct ProjectTile {
    pub title: String,
    pub tasks: usize,
    pub members: usize,
    pub percentage: u32,
    /// The figure's colour: green while the project leads, grey otherwise.
    pub per_color: Color,
    pub bar_color: Color,
    /// The members' letters, the signed-in member's flagged.
    pub avatars: Vec<(String, bool)>,
    pub on_tap: Callback,
}

/// A progress bar, `height` tall, `progress` of it filled.
pub(crate) fn progress_bar(progress: f32, height: f32, color: Color) -> Box<dyn Widget> {
    let progress = progress.clamp(0.0, 1.0);
    let mut row = Row::new().cross_axis_alignment(Align::Stretch);
    if progress > 0.0 {
        row = row
            .child(Expanded::new(Container::new().decoration(filled(color, FULL))).flex(progress));
    }
    if progress < 1.0 {
        row = row.child(Expanded::new(Container::new()).flex(1.0 - progress));
    }
    Box::new(
        Container::new()
            .height(height)
            .decoration(filled(light_gray(), FULL))
            .clip()
            .child(row),
    )
}

/// Avatars overlapping by 8.
pub(crate) fn avatar_stack(avatars: &[(String, bool)]) -> Box<dyn Widget> {
    let mut stack = Stack::new();
    for (i, (letter, you)) in avatars.iter().enumerate() {
        stack = stack.layer(
            Positioned::new(MemberAvatar::new(letter.clone(), *you).into_widget())
                .left(i as f32 * 20.0),
        );
    }
    let width = if avatars.is_empty() {
        0.0
    } else {
        28.0 + (avatars.len() - 1) as f32 * 20.0
    };
    Box::new(Container::new().width(width).height(28.0).child(stack))
}

impl StatelessWidget for ProjectTile {
    fn build(&self, _cx: &BuildContext) -> Box<dyn Widget> {
        let heading = Row::new()
            .main_axis_alignment(Justify::SpaceBetween)
            .cross_axis_alignment(Align::Start)
            .child(
                Column::new()
                    .spacing(4.0)
                    .cross_axis_alignment(Align::Start)
                    .child(text(
                        self.title.clone(),
                        16.0,
                        24.0,
                        black(),
                        FontWeight::SemiBold,
                    ))
                    .child(text(
                        format!("{} tasks · {} members", self.tasks, self.members),
                        12.0,
                        16.0,
                        grey(),
                        FontWeight::Regular,
                    )),
            )
            .child(text(
                format!("{}%", self.percentage),
                14.0,
                20.0,
                self.per_color,
                FontWeight::SemiBold,
            ));

        let card = accent_card(
            gray(),
            (17.0, 17.0, 17.0, 20.0),
            Column::new()
                .spacing(16.0)
                .cross_axis_alignment(Align::Stretch)
                .child(heading)
                .child(progress_bar(
                    self.percentage as f32 / 100.0,
                    8.0,
                    self.bar_color,
                ))
                .child(Row::new().child(avatar_stack(&self.avatars))),
        );
        Box::new(GestureDetector::new(card).on_tap(self.on_tap.clone()))
    }
}
