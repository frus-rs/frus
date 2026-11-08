use frus::{
    Align, Column, Container, FontWeight, GestureDetector, Justify, Row, State, StateContext,
    StatefulWidget, TextAlign, Widget,
};

use crate::{
    common::{
        chrome::{back, AppHeader, NavStyle, Page, Tab},
        colors::{black, green_tint, grey, light_gray, primary_color},
        icons::{icon, Glyph},
        routes::{project_board, search_results, task, SEARCH, SEARCH_FILTERS},
        shared::on,
        ui::{
            avatar, back_link, caption, card, chips, divider, filled, gap, green_tag, icon_button,
            outlined, primary_button, section_label, tag, tap, text, tracked, two_lines, Field,
            FULL,
        },
    },
    features::search::states::search_state::ResultsState,
    models::{
        member::Member,
        project::Project,
        store,
        task::{Task, TaskStatus},
    },
};

/// What a search found: projects, tasks and members, each type on its own or all together.
pub(crate) struct SearchResultsPage {
    pub query: String,
    pub kind: usize,
}

impl StatefulWidget for SearchResultsPage {
    type State = ResultsState;

    fn create_state(&self) -> ResultsState {
        ResultsState {
            query: self.query.clone(),
            kind: self.kind,
        }
    }
}

/// A result: a title over where it lives, tags and an avatar below.
fn result_card(
    title: &str,
    place: &str,
    tags: Vec<Box<dyn Widget>>,
    who: Option<&Member>,
    open: frus::Callback,
) -> Box<dyn Widget> {
    let mut bottom = Row::new()
        .main_axis_alignment(Justify::SpaceBetween)
        .cross_axis_alignment(Align::Center);
    let mut tag_row = Row::new().spacing(8.0).cross_axis_alignment(Align::Center);
    for t in tags {
        tag_row = tag_row.child(t);
    }
    bottom = bottom.child(tag_row);
    if let Some(m) = who {
        bottom = bottom.child(avatar(&m.letter(), 28.0, 12.0, m.is_you));
    }
    Box::new(
        GestureDetector::new(card(
            Column::new()
                .spacing(16.0)
                .cross_axis_alignment(Align::Stretch)
                .child(
                    Row::new()
                        .main_axis_alignment(Justify::SpaceBetween)
                        .cross_axis_alignment(Align::Start)
                        .child(two_lines(
                            text(title, 14.0, 20.0, black(), FontWeight::SemiBold),
                            caption(place),
                            4.0,
                        ))
                        .child(icon(Glyph::ChevronRight, 16.0, grey())),
                )
                .child(bottom),
        ))
        .on_tap(open),
    )
}

fn matches(query: &str, haystack: &[&str]) -> bool {
    query.is_empty() || haystack.iter().any(|h| h.to_lowercase().contains(query))
}

impl State for ResultsState {
    type Widget = SearchResultsPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let router = cx.router();
        let query = self.query.trim().to_lowercase();
        let kind = self.kind;

        type Found = (
            Vec<Project>,
            Vec<(Task, String, Option<Member>)>,
            Vec<Member>,
        );
        let (projects, tasks, members): Found = store::read(|s| {
            let filters = s.search_filters.clone();
            let projects = s
                .projects
                .iter()
                .filter(|p| matches(&query, &[&p.name, &p.description]))
                .cloned()
                .collect();
            let tasks = s
                .tasks
                .iter()
                .filter(|t| {
                    let labels = t.labels.iter().map(String::as_str).collect::<Vec<_>>();
                    let mut hay = vec![t.title.as_str(), t.description.as_str()];
                    hay.extend(labels);
                    matches(&query, &hay)
                })
                .filter(|t| match &filters {
                    Some(f) if f.statuses.iter().any(|b| *b) => {
                        f.statuses[TaskStatus::ALL
                            .iter()
                            .position(|s| *s == t.status)
                            .unwrap_or(0)]
                    }
                    _ => true,
                })
                .filter(|t| match &filters {
                    Some(f) if !f.labels.is_empty() => {
                        t.labels.iter().any(|l| f.labels.contains(l))
                    }
                    _ => true,
                })
                .map(|t| {
                    (
                        t.clone(),
                        s.project(t.project_id)
                            .map(|p| p.name.clone())
                            .unwrap_or_default(),
                        t.assignee.and_then(|m| s.member(m)).cloned(),
                    )
                })
                .collect();
            let members = s
                .members
                .iter()
                .filter(|m| matches(&query, &[&m.full_name, &m.title]))
                .cloned()
                .collect();
            (projects, tasks, members)
        });

        let show = |k: usize| kind == 0 || kind == k;
        let count = if show(1) { projects.len() } else { 0 }
            + if show(2) { tasks.len() } else { 0 }
            + if show(3) { members.len() } else { 0 };

        let clear = cx.callback(|s: &mut ResultsState| s.query.clear());
        let submit = {
            let router = router.clone();
            cx.callback(move |s: &mut ResultsState| router.replace(search_results(s.query.trim())))
        };
        let mut field = Field::new(
            &self.query,
            "Search Forge",
            cx.handler(ResultsState::set_query),
        )
        .radius(8.0)
        .lead(Glyph::Search)
        .on_submit(submit);
        if !self.query.is_empty() {
            field = field.trailing(icon_button(Glyph::X, 16.0, 24.0, grey(), clear));
        }

        let top = Column::new()
            .cross_axis_alignment(Align::Stretch)
            .child(back_link(
                "Search",
                20.0,
                8.0,
                back(&router, SEARCH.to_string()),
            ))
            .child(gap(24.0))
            .child(field.build());

        let content = if count == 0 {
            no_results(cx, &router, &self.query, top)
        } else {
            let filters = {
                let router = router.clone();
                on(move || router.push(SEARCH_FILTERS))
            };
            let mut list = Column::new()
                .spacing(24.0)
                .cross_axis_alignment(Align::Stretch);
            if show(1) && !projects.is_empty() {
                let mut section = Column::new()
                    .spacing(12.0)
                    .cross_axis_alignment(Align::Stretch)
                    .child(section_label("Projects"));
                for p in &projects {
                    let router = router.clone();
                    let id = p.id;
                    section = section.child(result_card(
                        &p.name,
                        &format!("{} tasks · {} members", p.task_count, p.member_count),
                        vec![Box::new(tag(p.status.label()))],
                        None,
                        on(move || router.push(project_board(id))),
                    ));
                }
                list = list.child(section);
            }
            if show(2) && !tasks.is_empty() {
                let mut section = Column::new()
                    .spacing(12.0)
                    .cross_axis_alignment(Align::Stretch)
                    .child(section_label("Tasks"));
                for (t, project, who) in &tasks {
                    let router = router.clone();
                    let id = t.id;
                    let status: Box<dyn Widget> = if t.status == TaskStatus::InProgress {
                        Box::new(green_tag(t.status.label()))
                    } else {
                        Box::new(tag(t.status.label()))
                    };
                    let mut tags = vec![status];
                    if let Some(label) = t.labels.first() {
                        tags.push(Box::new(tag(label.clone())));
                    }
                    section = section.child(result_card(
                        &t.title,
                        project,
                        tags,
                        who.as_ref(),
                        on(move || router.push(task(id))),
                    ));
                }
                list = list.child(section);
            }
            if show(3) && !members.is_empty() {
                let mut section = Column::new()
                    .spacing(12.0)
                    .cross_axis_alignment(Align::Stretch)
                    .child(section_label("Members"));
                for m in &members {
                    section = section.child(result_card(
                        &m.full_name,
                        &m.title,
                        vec![Box::new(tag(m.presence.label()))],
                        Some(m),
                        on(|| {}),
                    ));
                }
                list = list.child(section);
            }

            Column::new()
                .cross_axis_alignment(Align::Stretch)
                .child(top)
                .child(gap(24.0))
                .child(
                    Row::new()
                        .main_axis_alignment(Justify::SpaceBetween)
                        .cross_axis_alignment(Align::Center)
                        .child(text(
                            format!("{count} result{}", if count == 1 { "" } else { "s" }),
                            14.0,
                            20.0,
                            black(),
                            FontWeight::Medium,
                        ))
                        .child(tap(
                            Container::new()
                                .width(36.0)
                                .height(36.0)
                                .decoration(outlined(8.0))
                                .alignment(frus::Alignment::CENTER)
                                .child(icon(Glyph::SlidersHorizontal, 16.0, grey())),
                            filters,
                        )),
                )
                .child(gap(24.0))
                .child(chips(
                    &["All", "Projects", "Tasks", "Members"].map(String::from),
                    self.kind,
                    |i| cx.callback(move |s: &mut ResultsState| s.kind = i),
                ))
                .child(gap(32.0))
                .child(list)
        };

        Page::new(AppHeader::new("Search"), content)
            .nav(Tab::Projects, NavStyle::Standard)
            .build()
    }
}

/// Nothing found: say so, offer to clear the filters or search again, suggest searches.
fn no_results(
    cx: &StateContext<ResultsState>,
    router: &frus::GoRouter,
    query: &str,
    top: Column,
) -> Column {
    let badge = Container::new()
        .width(64.0)
        .height(64.0)
        .decoration(filled(green_tint(), FULL))
        .alignment(frus::Alignment::CENTER)
        .child(icon(Glyph::SearchX, 32.0, primary_color()));

    let clear_filters = cx.callback(|s: &mut ResultsState| {
        store::write(|store| store.search_filters = None);
        s.kind = 0;
    });
    let again = {
        let router = router.clone();
        on(move || {
            if !router.pop() {
                router.go(SEARCH);
            }
        })
    };

    let mut suggestions = Row::new().spacing(8.0).cross_axis_alignment(Align::Center);
    for word in ["roadmap", "planning", "Q2 goals"] {
        let router = router.clone();
        suggestions = suggestions.child(tap(
            Container::new()
                .decoration(filled(light_gray(), FULL))
                .padding_each(8.0, 16.0, 8.0, 16.0)
                .child(text(word, 14.0, 20.0, grey(), FontWeight::Regular)),
            on(move || router.replace(search_results(word))),
        ));
    }

    let empty = Column::new()
        .cross_axis_alignment(Align::Center)
        .child(badge)
        .child(gap(24.0))
        .child(tracked(
            "No results found",
            20.0,
            28.0,
            -0.5,
            black(),
            FontWeight::Bold,
        ))
        .child(gap(8.0))
        .child(
            text(
                format!("We couldn’t find anything matching {}.", query.trim()),
                14.0,
                24.0,
                grey(),
                FontWeight::Regular,
            )
            .align(TextAlign::Center),
        )
        .child(gap(4.0))
        .child(
            text(
                "Try a different keyword or clear your filters.",
                14.0,
                24.0,
                grey(),
                FontWeight::Regular,
            )
            .align(TextAlign::Center),
        )
        .child(gap(24.0))
        .child(Container::new().width(144.0).child(primary_button(
            "Clear filters",
            44.0,
            8.0,
            clear_filters,
        )))
        .child(gap(12.0))
        .child(tap(
            Container::new()
                .height(40.0)
                .padding_each(0.0, 16.0, 0.0, 16.0)
                .alignment(frus::Alignment::CENTER)
                .child(text(
                    "Search again",
                    14.0,
                    20.0,
                    primary_color(),
                    FontWeight::SemiBold,
                )),
            again,
        ));

    top.child(gap(44.0))
        .child(
            Container::new()
                .padding_each(0.0, 20.0, 0.0, 20.0)
                .child(empty),
        )
        .child(gap(56.0))
        .child(divider())
        .child(gap(16.0))
        .child(section_label("Suggested searches"))
        .child(gap(12.0))
        .child(suggestions)
}
