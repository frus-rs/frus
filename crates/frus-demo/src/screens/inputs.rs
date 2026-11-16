// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! Inputs and lists (milestone 810): a page each for the one-time code boxes, the field
//! frame and the input group, the empty state, the swipe row and the wheel picker.

use frus_widgets::{
    Addon, EmptyState, Field, InputGroup, OtpInput, OtpKind, SwipeAction, SwipeActions, Text,
    WheelColumn, WheelPicker,
};

use crate::prelude::*;

/// A page with a back arrow, a title and a scrolling body.
fn page(router: GoRouter, title: &str, theme: &Theme, body: Flex) -> Box<dyn Widget> {
    Box::new(
        Scaffold::new()
            .background(theme.background)
            .app_bar(
                AppBar::new()
                    .title(Text::new(title))
                    .leading(IconButton::new(Icons::ARROW_BACK).on_press(on(move || {
                        router.pop();
                    })))
                    .build(),
            )
            .body(
                SingleChildScrollView::new()
                    .flex(1.0)
                    .child(body.padding(16.0).gap(20.0)),
            )
            .build(),
    )
}

fn heading(words: &str, theme: &Theme) -> Text {
    text(words)
        .size(16.0)
        .weight(FontWeight::SemiBold)
        .color(theme.on_surface)
}

// ---------------------------------------------------------------------------------------
// One-time code
// ---------------------------------------------------------------------------------------

/// The one-time code page.
pub(crate) struct OtpPage;

/// What the page keeps: the three codes and what the check said.
#[derive(Default)]
pub(crate) struct OtpState {
    sms: String,
    pin: String,
    letters: String,
    verdict: Option<bool>,
}

impl StatefulWidget for OtpPage {
    type State = OtpState;
    fn create_state(&self) -> OtpState {
        OtpState::default()
    }
}

impl State for OtpState {
    type Widget = OtpPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let theme = cx.theme().clone();
        let typed = cx.handler(|s: &mut OtpState, code: String| {
            s.sms = code;
            s.verdict = None;
        });
        let done = cx.handler(|s: &mut OtpState, code: String| {
            s.verdict = Some(code == "123456");
            s.sms = code;
        });
        let error = self.verdict == Some(false);
        let body = Flex::column()
            .child(heading("SMS code (6 digits, two groups)", &theme))
            .child(
                Field::new(
                    OtpInput::new(&self.sms, 6, typed)
                        .on_completed(done)
                        .groups([3, 3])
                        .error(error)
                        .label("Verification code"),
                )
                .label("Verification code")
                .description("Type 123456, paste it, or let the phone fill it from the SMS.")
                .error(error.then(|| "That code is not right.".to_string()))
                .required(true),
            )
            .child(
                text(match self.verdict {
                    Some(true) => "Code accepted.",
                    Some(false) => "Code refused.",
                    None => "Waiting for the code.",
                })
                .size(14.0)
                .color(theme.muted),
            )
            .child(heading("Masked PIN", &theme))
            .child(
                OtpInput::new(
                    &self.pin,
                    4,
                    cx.handler(|s: &mut OtpState, c: String| s.pin = c),
                )
                .masked(true)
                .label("PIN"),
            )
            .child(heading("Letters and digits", &theme))
            .child(
                OtpInput::new(
                    &self.letters,
                    5,
                    cx.handler(|s: &mut OtpState, c: String| s.letters = c),
                )
                .kind(OtpKind::Alphanumeric)
                .box_size(44.0, 52.0)
                .label("Invite code"),
            )
            .child(heading("Disabled", &theme))
            .child(OtpInput::new("42", 6, |_| ()).enabled(false));
        page(cx.router(), "One-time code", &theme, body)
    }
}

// ---------------------------------------------------------------------------------------
// Field and input group
// ---------------------------------------------------------------------------------------

/// The field and input group page.
pub(crate) struct FieldsPage;

/// What the page keeps.
#[derive(Default)]
pub(crate) struct FieldsState {
    email: String,
    site: String,
    query: String,
    amount: String,
    searched: String,
}

impl StatefulWidget for FieldsPage {
    type State = FieldsState;
    fn create_state(&self) -> FieldsState {
        FieldsState::default()
    }
}

impl State for FieldsState {
    type Widget = FieldsPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let theme = cx.theme().clone();
        let email_error = (!self.email.is_empty() && !self.email.contains('@'))
            .then(|| "An email address has an @ in it.".to_string());
        let body = Flex::column()
            .child(heading("A field around a text field", &theme))
            .child(
                Field::new(
                    TextField::new(&self.email)
                        .placeholder("ada@example.com")
                        .on_input(cx.handler(|s: &mut FieldsState, v: String| s.email = v)),
                )
                .label("Email")
                .description("We send the receipt here.")
                .error(email_error)
                .required(true),
            )
            .child(heading("Input groups", &theme))
            .child(
                Field::new(
                    InputGroup::new(
                        TextField::new(&self.site)
                            .placeholder("example")
                            .on_input(cx.handler(|s: &mut FieldsState, v: String| s.site = v)),
                    )
                    .leading(Addon::text("https://"))
                    .trailing(Addon::text(".com")),
                )
                .label("Website"),
            )
            .child(
                InputGroup::new(
                    TextField::new(&self.query)
                        .placeholder("Search")
                        .on_input(cx.handler(|s: &mut FieldsState, v: String| s.query = v)),
                )
                .leading(Addon::icon(Icons::SEARCH))
                .trailing(Addon::widget(
                    button(
                        "Go",
                        cx.callback(|s: &mut FieldsState| s.searched = s.query.clone()),
                    )
                    .variant(Variant::Filled)
                    .size(14.0),
                )),
            )
            .child(
                text(format!("Searched for: {}", self.searched))
                    .size(14.0)
                    .color(theme.muted),
            )
            .child(
                Field::new(
                    InputGroup::new(
                        TextField::new(&self.amount)
                            .digits_only()
                            .on_input(cx.handler(|s: &mut FieldsState, v: String| s.amount = v)),
                    )
                    .leading(Addon::text("€"))
                    .trailing(Addon::widget(
                        IconButton::new(Icons::CLEAR)
                            .on_press(cx.callback(|s: &mut FieldsState| s.amount.clear())),
                    ))
                    .error(self.amount.len() > 6),
                )
                .label("Amount")
                .error((self.amount.len() > 6).then(|| "At most six digits.".to_string())),
            );
        page(cx.router(), "Fields and groups", &theme, body)
    }
}

// ---------------------------------------------------------------------------------------
// Empty state
// ---------------------------------------------------------------------------------------

/// The empty state page.
pub(crate) struct EmptyPage;

/// How many projects there are: none, to begin with.
#[derive(Default)]
pub(crate) struct EmptyStateState {
    projects: usize,
}

impl StatefulWidget for EmptyPage {
    type State = EmptyStateState;
    fn create_state(&self) -> EmptyStateState {
        EmptyStateState::default()
    }
}

impl State for EmptyStateState {
    type Widget = EmptyPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let theme = cx.theme().clone();
        let body = if self.projects == 0 {
            Flex::column().child(
                EmptyState::new("No projects yet")
                    .icon(Icons::FOLDER_OPEN)
                    .description("Projects you create show up here. Start one, or import one.")
                    .action(
                        button(
                            "New project",
                            cx.callback(|s: &mut EmptyStateState| s.projects = 1),
                        )
                        .variant(Variant::Filled),
                    )
                    .action(button("Import", on(|| {})).variant(Variant::Outlined)),
            )
        } else {
            Flex::column()
                .child(text(format!("{} project(s)", self.projects)).color(theme.on_surface))
                .child(button(
                    "Clear them",
                    cx.callback(|s: &mut EmptyStateState| s.projects = 0),
                ))
        };
        page(cx.router(), "Empty state", &theme, body)
    }
}

// ---------------------------------------------------------------------------------------
// Swipe row
// ---------------------------------------------------------------------------------------

/// The swipe row page.
pub(crate) struct SwipePage;

/// The mail in the list, and what was done last.
pub(crate) struct SwipeState {
    mail: Vec<(u32, &'static str, bool)>,
    said: String,
}

impl StatefulWidget for SwipePage {
    type State = SwipeState;
    fn create_state(&self) -> SwipeState {
        SwipeState {
            mail: [
                "Lunch on Friday?",
                "Your order has shipped",
                "Minutes of the meeting",
                "Photos from the trip",
                "Invoice 2026-114",
                "Welcome aboard",
                "The garden is in bloom",
                "Reminder: dentist",
            ]
            .into_iter()
            .enumerate()
            .map(|(i, subject)| (i as u32, subject, false))
            .collect(),
            said: "Swipe a row either way.".into(),
        }
    }
}

impl State for SwipeState {
    type Widget = SwipePage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let theme = cx.theme().clone();
        let mut list = Flex::column().gap(1.0);
        for &(id, subject, pinned) in &self.mail {
            let pin = cx.callback(move |s: &mut SwipeState| {
                if let Some(m) = s.mail.iter_mut().find(|m| m.0 == id) {
                    m.2 = !m.2;
                    s.said = format!("{} \"{}\"", if m.2 { "Pinned" } else { "Unpinned" }, m.1);
                }
            });
            let archive = cx.callback(move |s: &mut SwipeState| {
                s.said = format!("Archived message {id}");
                s.mail.retain(|m| m.0 != id);
            });
            let delete = cx.callback(move |s: &mut SwipeState| {
                s.said = format!("Deleted message {id}");
                s.mail.retain(|m| m.0 != id);
            });
            let content = Container::new()
                .color(theme.surface)
                .padding_each(0.0, 16.0, 0.0, 16.0)
                .child(
                    Flex::row()
                        .gap(12.0)
                        .height(72.0)
                        .align(Align::Center)
                        .child(
                            Icon::new(if pinned { Icons::PUSH_PIN } else { Icons::MAIL })
                                .color(theme.muted),
                        )
                        .child(Expanded::new(
                            text(subject).size(16.0).color(theme.on_surface),
                        )),
                );
            list = list.child(keyed(
                u64::from(id),
                SwipeActions::new(content)
                    .leading(
                        SwipeAction::new(if pinned { "Unpin" } else { "Pin" }, pin)
                            .icon(Icons::PUSH_PIN)
                            .colors(
                                theme.scheme.tertiary_container,
                                theme.scheme.on_tertiary_container,
                            ),
                    )
                    .trailing(
                        SwipeAction::new("Delete", delete)
                            .icon(Icons::DELETE)
                            .colors(theme.scheme.error, theme.scheme.on_error),
                    )
                    .trailing(SwipeAction::new("Archive", archive).icon(Icons::ARCHIVE))
                    .group(1)
                    .height(72.0),
            ));
        }
        let body = Flex::column()
            .child(text(self.said.clone()).size(14.0).color(theme.muted))
            .child(list);
        page(cx.router(), "Swipe actions", &theme, body)
    }
}

// ---------------------------------------------------------------------------------------
// Wheel picker
// ---------------------------------------------------------------------------------------

/// The wheel picker page.
pub(crate) struct WheelPage;

/// The time and the date picked.
pub(crate) struct WheelState {
    hour: usize,
    minute: usize,
    day: usize,
    month: usize,
    year: i32,
}

impl StatefulWidget for WheelPage {
    type State = WheelState;
    fn create_state(&self) -> WheelState {
        WheelState {
            hour: 9,
            minute: 30,
            day: 12,
            month: 10,
            year: 2026,
        }
    }
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

impl State for WheelState {
    type Widget = WheelPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let theme = cx.theme().clone();
        let time = WheelPicker::new()
            .column(
                WheelColumn::new(
                    24,
                    |h| format!("{h:02}"),
                    cx.handler(|s: &mut WheelState, h: usize| s.hour = h),
                )
                .selected(self.hour)
                .looping(true)
                .semantic_label(|h| format!("{h} hours")),
            )
            .column(
                WheelColumn::new(
                    60,
                    |m| format!("{m:02}"),
                    cx.handler(|s: &mut WheelState, m: usize| s.minute = m),
                )
                .selected(self.minute)
                .looping(true)
                .semantic_label(|m| format!("{m} minutes")),
            )
            .width(240.0);
        let days = days_in(self.month, self.year);
        let date = WheelPicker::new()
            .column(
                WheelColumn::new(
                    days,
                    |d| format!("{}", d + 1),
                    cx.handler(|s: &mut WheelState, d: usize| s.day = d),
                )
                .selected(self.day.min(days - 1))
                .looping(true)
                .flex(1.0),
            )
            .column(
                WheelColumn::new(
                    12,
                    |m| MONTHS[m].to_string(),
                    cx.handler(|s: &mut WheelState, m: usize| s.month = m),
                )
                .selected(self.month)
                .looping(true)
                .flex(2.0),
            )
            .column(
                WheelColumn::numbers(
                    1950..=2050,
                    self.year,
                    cx.handler(|s: &mut WheelState, y: i32| s.year = y),
                )
                .flex(1.5),
            );
        let body = Flex::column()
            .child(heading("A time, from two looping wheels", &theme))
            .child(time)
            .child(
                text(format!("{:02}:{:02}", self.hour, self.minute))
                    .size(18.0)
                    .color(theme.on_surface),
            )
            .child(heading("A date, from three", &theme))
            .child(date)
            .child(
                text(format!(
                    "{} {} {}",
                    self.day.min(days - 1) + 1,
                    MONTHS[self.month],
                    self.year
                ))
                .size(18.0)
                .color(theme.on_surface),
            );
        page(cx.router(), "Wheel picker", &theme, body)
    }
}

/// The days in month `month` (from 0) of `year`.
fn days_in(month: usize, year: i32) -> usize {
    match month {
        1 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        1 => 28,
        3 | 5 | 8 | 10 => 30,
        _ => 31,
    }
}
