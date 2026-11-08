//! What frames every signed-in page: the header, the bottom navigation and the scaffold
//! that holds them around a page's body.

use frus::{
    Align, BuildContext, Callback, Column, Container, Expanded, FontWeight, GestureDetector,
    Justify, Positioned, Row, Scaffold, SingleChildScrollView, Stack, StatelessWidget, Widget,
};

use crate::common::{
    colors::{black, gray, grey, light_gray, primary_color, surface, white},
    icons::{icon, Glyph},
    routes::{HOME, NOTIFICATIONS, PROFILE, PROJECTS, SETTINGS},
    shared::on,
    ui::{bottom_border, filled, text, top_border, tracked, FULL, W},
};

/// The three headers the design draws.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum HeaderStyle {
    /// 64 tall, "Forge" and the page's name side by side, a dot on the bell, a grey avatar.
    Standard,
    /// 56 tall, a rule between "Forge" and the page's name, a plain bell, a green avatar:
    /// the account pages.
    Account,
    /// 64 tall, a dark bell with its dot and a grey avatar with a dark letter: the
    /// workspace pages.
    Workspace,
}

/// The header at the top of a signed-in page.
pub(crate) struct AppHeader {
    pub subtitle: String,
    pub style: HeaderStyle,
}

impl AppHeader {
    pub(crate) fn new(subtitle: impl Into<String>) -> Self {
        Self {
            subtitle: subtitle.into(),
            style: HeaderStyle::Standard,
        }
    }

    pub(crate) fn style(mut self, style: HeaderStyle) -> Self {
        self.style = style;
        self
    }
}

impl StatelessWidget for AppHeader {
    fn build(&self, cx: &BuildContext) -> Box<dyn Widget> {
        let router = cx.router();
        let (to_notifications, to_profile) = (router.clone(), router);
        let style = self.style;

        let brand: W = match style {
            HeaderStyle::Account => Box::new(tracked(
                "Forge",
                18.0,
                28.0,
                -0.45,
                primary_color(),
                FontWeight::Bold,
            )),
            _ => Box::new(tracked(
                "Forge",
                20.0,
                28.0,
                -0.5,
                primary_color(),
                FontWeight::Bold,
            )),
        };
        let subtitle = text(
            self.subtitle.clone(),
            14.0,
            20.0,
            grey(),
            FontWeight::Regular,
        );
        let title: W = match style {
            HeaderStyle::Standard => Box::new(
                Row::new()
                    .spacing(12.0)
                    .cross_axis_alignment(Align::Baseline)
                    .child(brand)
                    .child(subtitle),
            ),
            HeaderStyle::Account => Box::new(
                Row::new()
                    .spacing(12.0)
                    .cross_axis_alignment(Align::Center)
                    .child(brand)
                    .child(
                        Container::new()
                            .box_border(frus::Border {
                                left: frus::BorderSide::new(gray(), 1.0),
                                ..frus::Border::NONE
                            })
                            .padding_each(0.0, 0.0, 0.0, 13.0)
                            .child(subtitle),
                    ),
            ),
            HeaderStyle::Workspace => Box::new(
                Row::new()
                    .spacing(8.0)
                    .cross_axis_alignment(Align::Baseline)
                    .child(brand)
                    .child(subtitle),
            ),
        };

        let bell_color = if style == HeaderStyle::Workspace {
            black()
        } else {
            grey()
        };
        let mut bell = Stack::new().layer(Container::new().width(20.0).height(20.0).child(icon(
            Glyph::Bell,
            20.0,
            bell_color,
        )));
        if style != HeaderStyle::Account {
            bell = bell.layer(
                Positioned::new(Container::new().width(8.0).height(8.0).decoration(
                    filled(primary_color(), FULL).border(frus::Border::new(1.5, surface())),
                ))
                .top(-2.0)
                .right(-2.0),
            );
        }
        let bell = GestureDetector::new(Container::new().width(20.0).height(20.0).child(bell))
            .on_tap(on(move || to_notifications.go(NOTIFICATIONS)));

        let (fill, ink, size) = match style {
            HeaderStyle::Standard => (light_gray(), grey(), 12.0),
            HeaderStyle::Account => (primary_color(), white(), 14.0),
            HeaderStyle::Workspace => (light_gray(), black(), 14.0),
        };
        let avatar = GestureDetector::new(
            Container::new()
                .width(32.0)
                .height(32.0)
                .decoration(filled(fill, FULL))
                .alignment(frus::Alignment::CENTER)
                .child(text("K", size, size + 4.0, ink, FontWeight::Medium)),
        )
        .on_tap(on(move || to_profile.push(PROFILE)));

        let (height, pad) = if style == HeaderStyle::Account {
            (56.0, 24.0)
        } else {
            (64.0, 20.0)
        };
        // The header starts under the status bar, and its white runs behind it.
        let top = frus::MediaQuery::of().padding.top;
        Box::new(
            Container::new()
                .height(height + top)
                .color(surface())
                .box_border(bottom_border())
                .padding_each(top, pad, 0.0, pad)
                .child(
                    Row::new()
                        .main_axis_alignment(Justify::SpaceBetween)
                        .cross_axis_alignment(Align::Center)
                        .child(title)
                        .child(
                            Row::new()
                                .spacing(16.0)
                                .cross_axis_alignment(Align::Center)
                                .child(bell)
                                .child(avatar),
                        ),
                ),
        )
    }
}

/// The four destinations of the bottom navigation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tab {
    Dashboard,
    Projects,
    Notifications,
    Settings,
}

impl Tab {
    const ALL: [Tab; 4] = [
        Tab::Dashboard,
        Tab::Projects,
        Tab::Notifications,
        Tab::Settings,
    ];

    fn label(self) -> &'static str {
        match self {
            Tab::Dashboard => "Dashboard",
            Tab::Projects => "Projects",
            Tab::Notifications => "Notifications",
            Tab::Settings => "Settings",
        }
    }

    fn glyph(self) -> Glyph {
        match self {
            Tab::Dashboard => Glyph::LayoutDashboard,
            Tab::Projects => Glyph::FolderKanban,
            Tab::Notifications => Glyph::Bell,
            Tab::Settings => Glyph::Settings,
        }
    }

    fn route(self) -> &'static str {
        match self {
            Tab::Dashboard => HOME,
            Tab::Projects => PROJECTS,
            Tab::Notifications => NOTIFICATIONS,
            Tab::Settings => SETTINGS,
        }
    }
}

/// The three bottom navigations the design draws.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum NavStyle {
    /// 64 tall, 20 icons, 11 labels.
    Standard,
    /// 44 tall, 16 icons, 10 labels: the account pages.
    Compact,
    /// 80 tall, 20 icons, 12 labels: the workspace pages.
    Tall,
}

/// The bottom navigation.
pub(crate) struct BottomNav {
    pub current: Tab,
    pub style: NavStyle,
}

impl StatelessWidget for BottomNav {
    fn build(&self, cx: &BuildContext) -> Box<dyn Widget> {
        let router = cx.router();
        let (height, glyph, label, line, gap, bottom) = match self.style {
            NavStyle::Standard => (64.0, 20.0, 11.0, 24.0, 4.0, 0.0),
            NavStyle::Compact => (44.0, 16.0, 10.0, 12.0, 2.0, 0.0),
            NavStyle::Tall => (80.0, 20.0, 12.0, 16.0, 4.0, 8.0),
        };
        let mut row = Row::new().cross_axis_alignment(Align::Center);
        for tab in Tab::ALL {
            let color = if tab == self.current {
                primary_color()
            } else {
                grey()
            };
            let go = router.clone();
            let current = self.current;
            let item = GestureDetector::new(
                Column::new()
                    .spacing(gap)
                    .main_axis_alignment(Justify::Center)
                    .cross_axis_alignment(Align::Center)
                    .child(icon(tab.glyph(), glyph, color))
                    .child(text(tab.label(), label, line, color, FontWeight::Regular)),
            )
            .on_tap(on(move || {
                if tab != current {
                    go.go(tab.route());
                }
            }));
            row = row.child(Expanded::new(item));
        }
        // The scaffold tells its bottom slot how much of the screen's bottom edge the system
        // takes (the navigation buttons, the home indicator) without padding it: the bar
        // keeps clear of it itself, its background running down behind the system bar.
        let inset = frus::MediaQuery::of().padding.bottom;
        Box::new(
            Container::new()
                .height(height + inset)
                .color(surface())
                .box_border(top_border())
                .padding_each(0.0, 8.0, bottom + inset, 8.0)
                .child(row),
        )
    }
}

/// A signed-in page: its header, its body scrolling between the bars, the bottom
/// navigation and, when it has one, a bar of actions pinned above the navigation.
pub(crate) struct Page {
    header: W,
    nav: Option<BottomNav>,
    body: W,
    footer: Option<W>,
    padding: (f32, f32, f32),
}

impl Page {
    pub(crate) fn new(header: AppHeader, body: impl Widget + 'static) -> Self {
        Self::with_bar(Box::new(header.into_widget()), body)
    }

    /// A page whose top bar is its own rather than the application's header.
    pub(crate) fn with_bar(bar: W, body: impl Widget + 'static) -> Self {
        Page {
            header: bar,
            nav: None,
            body: Box::new(body),
            footer: None,
            padding: (24.0, 20.0, 32.0),
        }
    }

    /// The bottom navigation, `tab` lit.
    pub(crate) fn nav(mut self, tab: Tab, style: NavStyle) -> Self {
        self.nav = Some(BottomNav {
            current: tab,
            style,
        });
        self
    }

    /// A bar of actions above the navigation: white, a hairline above it.
    pub(crate) fn footer(mut self, footer: impl Widget + 'static) -> Self {
        self.footer = Some(Box::new(
            Container::new()
                .color(surface())
                .box_border(top_border())
                .padding_each(17.0, 20.0, 16.0, 20.0)
                .child(footer),
        ));
        self
    }

    /// Something pinned under the body with no hairline: the comment field of a task.
    pub(crate) fn plain_footer(mut self, footer: impl Widget + 'static) -> Self {
        self.footer = Some(Box::new(
            Container::new()
                .color(surface())
                .padding_each(8.0, 20.0, 12.0, 20.0)
                .child(footer),
        ));
        self
    }

    /// The room above the content, at its sides and below it.
    pub(crate) fn padding(mut self, top: f32, side: f32, bottom: f32) -> Self {
        self.padding = (top, side, bottom);
        self
    }

    pub(crate) fn build(self) -> Box<dyn Widget> {
        let (top, side, bottom) = self.padding;
        let content: W = Box::new(
            SingleChildScrollView::new()
                .padding_each(top, side, bottom, side)
                .child(self.body),
        );
        // With no navigation under it, a footer keeps clear of the home indicator itself.
        let inset = if self.nav.is_none() {
            frus::MediaQuery::of().padding.bottom
        } else {
            0.0
        };
        let body: W = match self.footer {
            Some(footer) => Box::new(
                Column::new()
                    .cross_axis_alignment(Align::Stretch)
                    .child(Expanded::new(Container::new().child(content)))
                    .child(
                        Container::new()
                            .color(surface())
                            .padding_each(0.0, 0.0, inset, 0.0)
                            .child(footer),
                    ),
            ),
            None => content,
        };
        let mut scaffold = Scaffold::new()
            .background(surface())
            .app_bar(self.header)
            .body(body);
        if let Some(nav) = self.nav {
            scaffold = scaffold.bottom_navigation_bar(nav.into_widget());
        }
        scaffold.build()
    }
}

/// Goes back, or to `fallback` when there is nothing to go back to (a page opened by its
/// address).
pub(crate) fn back(router: &frus::GoRouter, fallback: String) -> Callback {
    let router = router.clone();
    on(move || {
        if !router.pop() {
            router.go(&fallback);
        }
    })
}
