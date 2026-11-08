use frus::{
    Align, Column, Container, Expanded, FontWeight, Row, State, StateContext, StatefulWidget,
    Widget,
};

use crate::{
    common::{
        chrome::{AppHeader, NavStyle, Page, Tab},
        colors::{black, grey, light_gray},
        icons::{icon, Glyph},
        routes::{browse, search_results},
        shared::on,
        ui::{body, filled, gap, group, muted, page_title, tap, text, Field, FULL},
    },
    features::search::states::search_state::SearchState,
    models::store,
};

/// Search across Forge: recent searches and what there is to browse.
pub(crate) struct SearchPage;

impl StatefulWidget for SearchPage {
    type State = SearchState;

    fn create_state(&self) -> SearchState {
        SearchState::default()
    }
}

impl State for SearchState {
    type Widget = SearchPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let router = cx.router();
        let (recent, counts) = store::read(|s| {
            (
                s.recent_searches.clone(),
                [
                    ("Projects", 12.max(s.projects.len()), 1),
                    ("Tasks", 48.max(s.tasks.len()), 2),
                    ("Members", s.members.len(), 3),
                ],
            )
        });

        let submit = {
            let router = router.clone();
            cx.callback(move |s: &mut SearchState| {
                let query = s.query.trim().to_string();
                if query.is_empty() {
                    return;
                }
                store::write(|store| {
                    store.recent_searches.retain(|r| *r != query);
                    store.recent_searches.insert(0, query.clone());
                    store.recent_searches.truncate(5);
                });
                router.push(search_results(&query));
            })
        };

        let rows = recent
            .iter()
            .map(|r| {
                let router = router.clone();
                let query = r.clone();
                tap(
                    Container::new()
                        .height(56.0)
                        .padding_each(0.0, 16.0, 0.0, 16.0)
                        .child(
                            Row::new()
                                .spacing(12.0)
                                .cross_axis_alignment(Align::Center)
                                .child(icon(Glyph::Clock, 16.0, grey()))
                                .child(Expanded::new(body(r.clone())))
                                .child(icon(Glyph::ChevronRight, 16.0, grey())),
                        ),
                    on(move || router.push(search_results(&query))),
                )
            })
            .collect();

        let mut types = Row::new().spacing(8.0).cross_axis_alignment(Align::Center);
        for (label, count, kind) in counts {
            let router = router.clone();
            types = types.child(tap(
                Container::new()
                    .height(36.0)
                    .decoration(filled(light_gray(), FULL))
                    .padding_each(0.0, 14.0, 0.0, 14.0)
                    .alignment(frus::Alignment::CENTER)
                    .child(
                        Row::new()
                            .spacing(6.0)
                            .main_axis_size(frus::MainAxisSize::Min)
                            .cross_axis_alignment(Align::Center)
                            .child(text(label, 14.0, 20.0, black(), FontWeight::Medium))
                            .child(text(
                                count.to_string(),
                                14.0,
                                20.0,
                                grey(),
                                FontWeight::Regular,
                            )),
                    ),
                on(move || router.push(browse(kind))),
            ));
        }

        let content = Column::new()
            .cross_axis_alignment(Align::Stretch)
            .child(page_title("Search"))
            .child(gap(8.0))
            .child(muted("Find projects, tasks, and teammates."))
            .child(gap(32.0))
            .child(
                Field::new(
                    &self.query,
                    "Search Forge",
                    cx.handler(SearchState::set_query),
                )
                .radius(12.0)
                .font(15.0)
                .lead(Glyph::Search)
                .on_submit(submit)
                .build(),
            )
            .child(gap(32.0))
            .child(text(
                "Recent searches",
                16.0,
                24.0,
                black(),
                FontWeight::SemiBold,
            ))
            .child(gap(16.0))
            .child(group(rows))
            .child(gap(32.0))
            .child(text(
                "Browse by type",
                16.0,
                24.0,
                black(),
                FontWeight::SemiBold,
            ))
            .child(gap(16.0))
            .child(types);

        Page::new(AppHeader::new("Search"), content)
            .nav(Tab::Projects, NavStyle::Standard)
            .padding(32.0, 20.0, 32.0)
            .build()
    }
}
