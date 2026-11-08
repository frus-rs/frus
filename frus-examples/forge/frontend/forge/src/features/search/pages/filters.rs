use frus::{
    Align, Column, Container, Expanded, FontWeight, GestureDetector, Row, State, StateContext,
    StatefulWidget, Widget,
};

use crate::{
    common::{
        chrome::{back, AppHeader, NavStyle, Page, Tab},
        colors::{black, gray, green_tint, grey, primary_color},
        icons::{icon, Glyph},
        routes::SEARCH_RESULTS,
        shared::on,
        ui::{
            back_link, body, checkbox, filled, group_with, outlined, primary_button, section_label,
            segmented, tap, text, title_block, FULL,
        },
    },
    features::search::states::search_state::FiltersState,
    models::store::{self, SearchFilters},
};

/// Narrow a search: by type, status, project, assignee, labels and date.
pub(crate) struct FiltersPage;

impl StatefulWidget for FiltersPage {
    type State = FiltersState;

    fn create_state(&self) -> FiltersState {
        FiltersState::default()
    }
}

/// A hug-width box that opens a choice: "All projects ›".
fn dropdown(label: &str) -> Box<dyn Widget> {
    Box::new(
        Row::new().child(tap(
            Container::new()
                .decoration(outlined(8.0))
                .padding_each(12.0, 16.0, 12.0, 16.0)
                .child(
                    Row::new()
                        .spacing(4.0)
                        .main_axis_size(frus::MainAxisSize::Min)
                        .cross_axis_alignment(Align::Center)
                        .child(body(label))
                        .child(icon(Glyph::ChevronRight, 16.0, grey())),
                ),
            on(|| {}),
        )),
    )
}

fn section(title: &str, control: Box<dyn Widget>) -> Box<dyn Widget> {
    Box::new(
        Column::new()
            .spacing(12.0)
            .cross_axis_alignment(Align::Stretch)
            .child(section_label(title))
            .child(control),
    )
}

/// A row of a list with a mark on its right: a checkbox, a check.
fn mark_row(label: &str, mark: Box<dyn Widget>, action: frus::Callback) -> Box<dyn Widget> {
    Box::new(
        GestureDetector::new(
            Container::new().padding_each(12.0, 16.0, 12.0, 16.0).child(
                Row::new()
                    .cross_axis_alignment(Align::Center)
                    .child(Expanded::new(body(label)))
                    .child(mark),
            ),
        )
        .on_tap(action),
    )
}

impl State for FiltersState {
    type Widget = FiltersPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let router = cx.router();

        let statuses = ["Todo", "In progress", "Review", "Done"]
            .iter()
            .enumerate()
            .map(|(i, label)| {
                mark_row(
                    label,
                    checkbox(self.statuses[i]),
                    cx.callback(move |s: &mut FiltersState| s.statuses[i] = !s.statuses[i]),
                )
            })
            .collect();

        let ranges = ["Any time", "Due this week", "Overdue"]
            .iter()
            .enumerate()
            .map(|(i, label)| {
                let mark: Box<dyn Widget> = if self.range == i {
                    Box::new(icon(Glyph::Check, 16.0, primary_color()))
                } else {
                    Box::new(Container::new().width(16.0).height(16.0))
                };
                mark_row(
                    label,
                    mark,
                    cx.callback(move |s: &mut FiltersState| s.range = i),
                )
            })
            .collect();

        let mut labels = Row::new().spacing(8.0).cross_axis_alignment(Align::Center);
        for (i, (label, on_)) in self.labels.iter().enumerate() {
            let chip = Container::new()
                .decoration(if *on_ {
                    filled(green_tint(), FULL)
                } else {
                    filled(crate::common::colors::surface(), FULL)
                        .border(frus::Border::new(1.0, gray()))
                })
                .padding_each(6.0, 12.0, 6.0, 12.0)
                .child(text(
                    label.clone(),
                    12.0,
                    16.0,
                    if *on_ { primary_color() } else { black() },
                    FontWeight::Medium,
                ));
            labels =
                labels.child(GestureDetector::new(chip).on_tap(
                    cx.callback(move |s: &mut FiltersState| s.labels[i].1 = !s.labels[i].1),
                ));
        }
        labels = labels.child(
            Container::new()
                .decoration(
                    filled(crate::common::colors::surface(), FULL)
                        .border(frus::Border::new(1.0, gray())),
                )
                .padding_each(6.0, 12.0, 6.0, 12.0)
                .child(text("Add label", 12.0, 16.0, grey(), FontWeight::Regular)),
        );

        let content = Column::new()
            .spacing(24.0)
            .cross_axis_alignment(Align::Stretch)
            .child(back_link(
                "Search results",
                16.0,
                8.0,
                back(&router, SEARCH_RESULTS.to_string()),
            ))
            .child(title_block(
                "Filters",
                "Narrow down what you’re looking for.",
                4.0,
            ))
            .child(section(
                "Type",
                segmented(
                    &["All", "Projects", "Tasks", "Members"],
                    self.kind,
                    14.0,
                    |i| cx.callback(move |s: &mut FiltersState| s.kind = i),
                ),
            ))
            .child(section("Status", Box::new(group_with(8.0, statuses))))
            .child(section("Project", dropdown("All projects")))
            .child(section("Assignee", dropdown("Anyone")))
            .child(section("Labels", Box::new(labels)))
            .child(section("Date range", Box::new(group_with(8.0, ranges))));

        let reset = cx.callback(|s: &mut FiltersState| {
            *s = FiltersState {
                statuses: [false; 4],
                ..FiltersState::default()
            };
            store::write(|store| store.search_filters = None);
        });
        let apply = {
            let router = router.clone();
            cx.callback(move |s: &mut FiltersState| {
                let filters = SearchFilters {
                    statuses: s.statuses,
                    labels: s
                        .labels
                        .iter()
                        .filter(|(_, on_)| *on_)
                        .map(|(l, _)| l.clone())
                        .collect(),
                };
                store::write(|store| store.search_filters = Some(filters));
                if !router.pop() {
                    router.go(SEARCH_RESULTS);
                }
            })
        };
        let actions = Row::new()
            .spacing(12.0)
            .cross_axis_alignment(Align::Center)
            .child(
                Expanded::new(tap(
                    Container::new()
                        .height(46.0)
                        .decoration(outlined(8.0))
                        .alignment(frus::Alignment::CENTER)
                        .child(text(
                            "Reset",
                            14.0,
                            20.0,
                            primary_color(),
                            FontWeight::SemiBold,
                        )),
                    reset,
                ))
                .flex(1.0),
            )
            .child(Expanded::new(primary_button("Apply filters", 46.0, 8.0, apply)).flex(2.0));

        Page::new(AppHeader::new("Search"), content)
            .nav(Tab::Projects, NavStyle::Standard)
            .footer(actions)
            .padding(20.0, 20.0, 24.0)
            .build()
    }
}
