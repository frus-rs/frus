use frus::{
    Align, Column, Container, Expanded, FontWeight, Positioned, RichText, Row, Stack, State,
    StateContext, StatefulWidget, TextSpan, Widget,
};

use crate::{
    common::{
        chrome::{AppHeader, NavStyle, Page, Tab},
        colors::{black, gray, grey, primary_color},
        icons::{icon, Glyph},
        ui::{avatar, banner, caption, chips, muted, text, title_block},
    },
    features::projects::{
        pages::board::missing, states::activity_state::ProjectActivityState,
        widgets::project_nav::back_to_project,
    },
    models::{
        activity::{Activity, ActivityKind},
        member::Member,
        store,
    },
};

/// Everything that happened in a project, day by day, as a timeline.
pub(crate) struct ProjectActivityPage {
    pub project: u32,
}

impl StatefulWidget for ProjectActivityPage {
    type State = ProjectActivityState;

    fn create_state(&self) -> ProjectActivityState {
        ProjectActivityState::default()
    }
}

/// An entry of the timeline; `last` has no line running down to the next.
fn entry(activity: &Activity, member: &Member, last: bool) -> Box<dyn Widget> {
    let mut words = TextSpan::new(format!("{} ", member.first_name()))
        .size(14.0)
        .color(black())
        .weight(FontWeight::SemiBold);
    let mut rest = activity.action.clone();
    if !activity.subject.is_empty() {
        rest = format!("{rest} {}", activity.subject);
    }
    if !activity.tail.is_empty() {
        rest = format!("{rest} {}", activity.tail);
    }
    if member.is_you && activity.action == "assigned" {
        rest.push_str(" to himself");
    }
    words = words.child(TextSpan::new(rest).weight(FontWeight::SemiBold));

    let row = Container::new()
        .padding_each(0.0, 0.0, if last { 0.0 } else { 24.0 }, 0.0)
        .child(
            Row::new()
                .spacing(12.0)
                .cross_axis_alignment(Align::Start)
                .child(avatar(&member.letter(), 32.0, 11.0, member.is_you))
                .child(Expanded::new(
                    Container::new().padding_each(2.0, 0.0, 0.0, 0.0).child(
                        RichText::new(words)
                            .base_style(frus::TextStyle::new(14.0).height(20.0 / 14.0)),
                    ),
                ))
                .child(
                    Container::new()
                        .padding_each(4.0, 0.0, 0.0, 0.0)
                        .child(caption(activity.time.clone())),
                ),
        );
    if last {
        return Box::new(row);
    }
    Box::new(
        Stack::new()
            .layer(
                Positioned::new(Container::new().width(1.0).color(gray()))
                    .left(15.5)
                    .top(32.0)
                    .bottom(0.0),
            )
            .layer(row),
    )
}

impl State for ProjectActivityState {
    type Widget = ProjectActivityPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let router = cx.router();
        let project_id = cx.widget().project;
        let filter = self.filter;
        let Some((project, entries)) = store::read(|s| {
            let project = s.project(project_id)?.clone();
            let mut entries = s
                .activity_of(project.id)
                .into_iter()
                .filter(|a| match filter {
                    1 => a.kind == ActivityKind::Task,
                    2 => a.kind == ActivityKind::Comment,
                    _ => true,
                })
                .filter_map(|a| s.member(a.member).map(|m| (a, m.clone())))
                .collect::<Vec<_>>();
            // The newest first within a day.
            entries.sort_by(|a, b| {
                (a.0.day != "Today")
                    .cmp(&(b.0.day != "Today"))
                    .then(b.0.time.cmp(&a.0.time))
            });
            Some((project, entries))
        }) else {
            return missing("Project");
        };

        let mut timeline = Column::new()
            .spacing(20.0)
            .cross_axis_alignment(Align::Stretch);
        for day in ["Today", "Yesterday"] {
            let of_day = entries
                .iter()
                .filter(|(a, _)| a.day == day)
                .collect::<Vec<_>>();
            if of_day.is_empty() {
                continue;
            }
            let mut section = Column::new()
                .spacing(12.0)
                .cross_axis_alignment(Align::Stretch)
                .child(text(day, 14.0, 20.0, grey(), FontWeight::Medium));
            let mut items = Column::new().cross_axis_alignment(Align::Stretch);
            for (i, (activity, member)) in of_day.iter().enumerate() {
                items = items.child(entry(activity, member, i + 1 == of_day.len()));
            }
            section = section.child(items);
            timeline = timeline.child(section);
        }
        if entries.is_empty() {
            timeline = timeline.child(muted("Nothing here yet."));
        }

        let content = Column::new()
            .spacing(20.0)
            .cross_axis_alignment(Align::Stretch)
            .child(back_to_project(&router, project.id, &project.name))
            .child(title_block(
                "Activity",
                "Everything happening in this project.",
                4.0,
            ))
            .child(chips(
                &[
                    "All activity".to_string(),
                    "Tasks".to_string(),
                    "Comments".to_string(),
                ],
                self.filter,
                |i| cx.callback(move |s: &mut ProjectActivityState| s.filter = i),
            ))
            .child(banner(
                Box::new(icon(Glyph::CircleCheck, 16.0, primary_color())),
                "All changes synced",
                8.0,
                12.0,
                12.0,
            ))
            .child(timeline);

        Page::new(AppHeader::new(project.name.clone()), content)
            .nav(Tab::Projects, NavStyle::Standard)
            .padding(20.0, 20.0, 32.0)
            .build()
    }
}
