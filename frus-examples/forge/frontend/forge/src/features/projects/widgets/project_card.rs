use frus::{
    Align, BuildContext, Callback, Column, Container, Expanded, FontWeight, GestureDetector,
    Justify, Positioned, Row, Stack, StatelessWidget, Widget,
};

use crate::{
    common::{
        colors::{black, grey, light_gray, surface},
        icons::{icon, Glyph},
        ui::{accent_card, caption, filled, green_tag, tag, text, FULL},
    },
    features::home::pages::widgets::project_tile::progress_bar,
    models::project::{Project, ProjectStatus},
};

/// A project in the list: its coloured edge and icon, its figures, its progress, its status
/// and who works on it.
pub(crate) struct ProjectCard {
    pub project: Project,
    pub letters: Vec<String>,
    pub on_tap: Callback,
}

/// The 24 px avatars at the end of a project card, overlapping by 4.
fn letters_stack(letters: &[String]) -> Box<dyn Widget> {
    let mut stack = Stack::new();
    for (i, letter) in letters.iter().enumerate() {
        stack = stack.layer(
            Positioned::new(
                Container::new()
                    .width(24.0)
                    .height(24.0)
                    .decoration(filled(surface(), FULL))
                    .padding(2.0)
                    .child(
                        Container::new()
                            .decoration(filled(light_gray(), FULL))
                            .alignment(frus::Alignment::CENTER)
                            .child(text(
                                letter.clone(),
                                10.0,
                                12.0,
                                black(),
                                FontWeight::Medium,
                            )),
                    ),
            )
            .left(i as f32 * 20.0),
        );
    }
    let width = if letters.is_empty() {
        0.0
    } else {
        24.0 + (letters.len() - 1) as f32 * 20.0
    };
    Box::new(Container::new().width(width).height(24.0).child(stack))
}

impl StatelessWidget for ProjectCard {
    fn build(&self, _cx: &BuildContext) -> Box<dyn Widget> {
        let p = &self.project;
        let accent = p.color.color();

        let badge = Container::new()
            .width(40.0)
            .height(40.0)
            .decoration(filled(p.color.tint(), 8.0))
            .alignment(frus::Alignment::CENTER)
            .child(icon(Glyph::FolderKanban, 20.0, accent));

        let figures = Column::new()
            .spacing(4.0)
            .cross_axis_alignment(Align::Start)
            .child(text(
                p.name.clone(),
                14.0,
                20.0,
                black(),
                FontWeight::SemiBold,
            ))
            .child(caption(format!(
                "{} tasks · {} members",
                p.task_count, p.member_count
            )))
            .child(Container::new().height(4.0))
            .child(
                Row::new()
                    .spacing(12.0)
                    .cross_axis_alignment(Align::Center)
                    .child(Container::new().width(82.0).child(progress_bar(
                        p.progress as f32 / 100.0,
                        6.0,
                        accent,
                    )))
                    .child(caption(format!("{}%", p.progress))),
            );

        let top = Row::new()
            .spacing(12.0)
            .cross_axis_alignment(Align::Start)
            .child(badge)
            .child(Expanded::new(figures))
            .child(icon(Glyph::Ellipsis, 20.0, grey()));

        let status = match p.status {
            ProjectStatus::Active => green_tag(p.status.label()).padding_each(4.0, 10.0, 4.0, 10.0),
            _ => tag(p.status.label()).padding_each(4.0, 10.0, 4.0, 10.0),
        };
        let bottom = Row::new()
            .main_axis_alignment(Justify::SpaceBetween)
            .cross_axis_alignment(Align::Center)
            .child(status)
            .child(letters_stack(&self.letters));

        let card = accent_card(
            accent,
            (17.0, 17.0, 17.0, 20.0),
            Column::new()
                .spacing(20.0)
                .cross_axis_alignment(Align::Stretch)
                .child(top)
                .child(bottom),
        );
        Box::new(GestureDetector::new(card).on_tap(self.on_tap.clone()))
    }
}
