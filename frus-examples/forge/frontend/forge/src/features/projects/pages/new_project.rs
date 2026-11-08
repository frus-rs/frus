use frus::{
    Align, Column, Container, FontWeight, GestureDetector, KeyboardType, Row, State, StateContext,
    StatefulWidget, TextAlign, Widget,
};

use crate::{
    common::{
        chrome::{back, AppHeader, NavStyle, Page, Tab},
        colors::{black, green_tint, primary_color, surface},
        icons::{icon, Glyph},
        routes::{project_board, PROJECTS},
        ui::{
            back_link, cancel, caption, filled, gap, labelled, primary_button, segmented_with,
            text, title_block, Field, FULL,
        },
    },
    features::projects::states::new_project_state::NewProjectState,
    models::{
        project::{Project, ProjectColor, ProjectStatus},
        store,
    },
};

/// Create a project: its name, description, colour, visibility and first members.
pub(crate) struct NewProjectPage;

impl StatefulWidget for NewProjectPage {
    type State = NewProjectState;

    fn create_state(&self) -> NewProjectState {
        NewProjectState::default()
    }
}

/// A removable chip: "Kokou ×".
fn invitee_chip(name: &str, remove: frus::Callback) -> Box<dyn Widget> {
    Box::new(
        Container::new()
            .decoration(filled(green_tint(), FULL))
            .padding_each(8.0, 12.0, 8.0, 12.0)
            .child(
                Row::new()
                    .spacing(8.0)
                    .main_axis_size(frus::MainAxisSize::Min)
                    .cross_axis_alignment(Align::Center)
                    .child(text(name, 14.0, 20.0, primary_color(), FontWeight::Medium))
                    .child(
                        GestureDetector::new(icon(Glyph::X, 16.0, primary_color().with_alpha(0.7)))
                            .on_tap(remove),
                    ),
            ),
    )
}

impl State for NewProjectState {
    type Widget = NewProjectPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let router = cx.router();

        // The colours a project can take, the chosen one ringed.
        let mut swatches = Row::new().spacing(12.0).cross_axis_alignment(Align::Center);
        for (i, color) in ProjectColor::ALL.iter().enumerate() {
            let chosen = i == self.color;
            let swatch = Container::new()
                .width(32.0)
                .height(32.0)
                .decoration(if chosen {
                    filled(surface(), FULL).border(frus::Border::new(2.0, color.color()))
                } else {
                    filled(surface(), FULL)
                })
                .padding(4.0)
                .child(Container::new().decoration(filled(color.color(), FULL)));
            swatches = swatches.child(
                GestureDetector::new(swatch)
                    .on_tap(cx.callback(move |s: &mut NewProjectState| s.color = i)),
            );
        }

        let mut invitees = Row::new().spacing(8.0).cross_axis_alignment(Align::Center);
        for (i, name) in self.invitees.iter().enumerate() {
            invitees = invitees.child(invitee_chip(
                name,
                cx.callback(move |s: &mut NewProjectState| {
                    s.invitees.remove(i);
                }),
            ));
        }

        let error = (self.submitted && self.name.trim().is_empty()).then(|| {
            text(
                "Give the project a name.",
                12.0,
                16.0,
                crate::common::colors::red(),
                FontWeight::Regular,
            )
        });

        let create = {
            let router = router.clone();
            cx.callback(move |s: &mut NewProjectState| {
                s.submitted = true;
                if s.name.trim().is_empty() {
                    return;
                }
                let id = store::write(|store| {
                    let id = store.next_project_id();
                    let you = store.you().id;
                    store.projects.push(Project {
                        id,
                        name: s.name.trim().to_string(),
                        description: s.description.trim().to_string(),
                        color: ProjectColor::ALL[s.color],
                        status: ProjectStatus::Active,
                        task_count: 0,
                        member_count: s.invitees.len().max(1),
                        members: vec![you],
                        progress: 0,
                        in_progress: 0,
                        due_soon: 0,
                    });
                    id
                });
                router.replace(project_board(id));
            })
        };

        let mut name_field = Column::new()
            .spacing(8.0)
            .cross_axis_alignment(Align::Stretch)
            .child(text(
                "Project name",
                14.0,
                20.0,
                black(),
                FontWeight::SemiBold,
            ))
            .child(
                Field::new(
                    &self.name,
                    "e.g. Marketing website",
                    cx.handler(NewProjectState::set_name),
                )
                .radius(8.0)
                .build(),
            );
        if let Some(error) = error {
            name_field = name_field.child(error);
        }

        let form = Column::new()
            .spacing(24.0)
            .cross_axis_alignment(Align::Stretch)
            .child(name_field)
            .child(labelled(
                "Description",
                Field::new(
                    &self.description,
                    "What is this project about?",
                    cx.handler(NewProjectState::set_description),
                )
                .radius(8.0)
                .height(112.0)
                .lines(4)
                .build(),
            ))
            .child(
                Column::new()
                    .spacing(12.0)
                    .cross_axis_alignment(Align::Stretch)
                    .child(text(
                        "Project color",
                        14.0,
                        20.0,
                        black(),
                        FontWeight::SemiBold,
                    ))
                    .child(swatches),
            )
            .child(
                Column::new()
                    .spacing(12.0)
                    .cross_axis_alignment(Align::Stretch)
                    .child(text(
                        "Visibility",
                        14.0,
                        20.0,
                        black(),
                        FontWeight::SemiBold,
                    ))
                    .child(segmented_with(
                        &[
                            (Some(Glyph::Lock), "Private"),
                            (Some(Glyph::Users), "Workspace"),
                        ],
                        self.visibility,
                        14.0,
                        40.0,
                        |i| cx.callback(move |s: &mut NewProjectState| s.visibility = i),
                    )),
            )
            .child(
                Column::new()
                    .spacing(12.0)
                    .cross_axis_alignment(Align::Stretch)
                    .child(text(
                        "Invite members",
                        14.0,
                        20.0,
                        black(),
                        FontWeight::SemiBold,
                    ))
                    .child(
                        Field::new(
                            &self.email,
                            "Add by email",
                            cx.handler(NewProjectState::set_email),
                        )
                        .radius(8.0)
                        .keyboard(KeyboardType::Email)
                        .on_submit(cx.callback(NewProjectState::add_invitee))
                        .build(),
                    )
                    .child(invitees),
            );

        let content = Column::new()
            .cross_axis_alignment(Align::Stretch)
            .child(
                Container::new()
                    .height(44.0)
                    .alignment(frus::Alignment::CENTER_LEFT)
                    .child(back_link(
                        "Projects",
                        20.0,
                        8.0,
                        back(&router, PROJECTS.to_string()),
                    )),
            )
            .child(gap(24.0))
            .child(title_block(
                "Create a project",
                "Bring your team and work together.",
                8.0,
            ))
            .child(gap(32.0))
            .child(form)
            .child(gap(32.0))
            .child(primary_button("Create project", 48.0, 8.0, create))
            .child(gap(12.0))
            .child(cancel(back(&router, PROJECTS.to_string())))
            .child(gap(24.0))
            .child(caption("You can change these settings later.").align(TextAlign::Center));

        Page::new(AppHeader::new("New project"), content)
            .nav(Tab::Projects, NavStyle::Standard)
            .padding(24.0, 20.0, 40.0)
            .build()
    }
}
