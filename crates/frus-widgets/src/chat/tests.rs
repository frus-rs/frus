// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The conversation's tests (milestone 815): rows, runs and days, the list's anchoring and
//! its jump button, the composer, the dots, right to left and what a reader hears.

use std::cell::Cell;
use std::rc::Rc;

use frus_core::{Point, Primitive, Rect, Size};

use super::*;
use crate::interaction::{Key, WidgetId};
use crate::runtime::Edit;
use crate::{build_ui, Runtime, Ui};

#[derive(Clone, Debug, PartialEq)]
enum Msg {
    State(ChatListState),
    Retry(String),
    Draft(String),
    Send,
}

/// Noon on 2026-11-16, local time.
const NOON: i64 = 20_773 * 86_400 + 12 * 3600;
/// That day.
fn today() -> CalendarDate {
    CalendarDate::from_days(20_773)
}

fn conversation() -> Rc<Vec<ChatMessage>> {
    Rc::new(vec![
        ChatMessage::incoming("1", "ada", "Yesterday's note", NOON - 86_400).author_name("Ada"),
        ChatMessage::incoming("2", "ada", "Did the build pass?", NOON).author_name("Ada"),
        ChatMessage::incoming("3", "ada", "And the tests?", NOON + 30).author_name("Ada"),
        ChatMessage::outgoing("4", "Both green.", NOON + 90).status(MessageStatus::Read),
        ChatMessage::outgoing("5", "Pushing now.", NOON + 95).status(MessageStatus::Failed),
    ])
}

fn chat(messages: Rc<Vec<ChatMessage>>, state: &ChatListState) -> ChatList<Msg> {
    ChatList::new(messages, state, Msg::State)
        .today(today())
        .on_retry(Msg::Retry)
}

fn frame_with(widget: &dyn Widget<Msg>, size: Size, runtime: &Runtime, theme: &Theme) -> Ui<Msg> {
    build_ui(widget, size, runtime, theme)
}

fn frame(widget: &dyn Widget<Msg>) -> Ui<Msg> {
    frame_with(
        widget,
        Size::new(400.0, 600.0),
        &Runtime::default(),
        &Theme::light(),
    )
}

/// Every text drawn, with where.
fn texts(ui: &Ui<Msg>) -> Vec<(String, Rect)> {
    ui.scene()
        .primitives()
        .iter()
        .filter_map(|p| match p {
            Primitive::Text { text, position, .. } => {
                Some((text.clone(), Rect::new(position.x, position.y, 0.0, 0.0)))
            }
            _ => None,
        })
        .collect()
}

fn text_at(ui: &Ui<Msg>, words: &str) -> Option<Rect> {
    texts(ui)
        .into_iter()
        .find(|(t, _)| t == words)
        .map(|(_, at)| at)
}

/// The messages every clickable thing on the screen sends.
fn clicks(ui: &Ui<Msg>) -> Vec<Msg> {
    let rects: Vec<Rect> = ui.hit_rects().collect();
    rects
        .into_iter()
        .filter_map(|r| {
            let centre = Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0);
            ui.hit(centre).and_then(|id| ui.msg_for(id))
        })
        .collect()
}

fn rect_fill(ui: &Ui<Msg>, colour: Color) -> Vec<Rect> {
    ui.scene()
        .primitives()
        .iter()
        .filter_map(|p| match p {
            Primitive::Rect { rect, color, .. } if *color == colour => Some(*rect),
            _ => None,
        })
        .collect()
}

#[test]
fn the_newest_message_rests_at_the_bottom() {
    let ui = frame(&chat(conversation(), &ChatListState::new()));
    let newest = text_at(&ui, "Pushing now.").expect("the newest message is drawn");
    let older = text_at(&ui, "Did the build pass?").expect("an older one too");
    assert!(newest.y > older.y, "newest under the older ones");
    assert!(newest.y > 450.0, "near the bottom of 600: {newest:?}");
}

#[test]
fn days_are_separated_and_named() {
    let ui = frame(&chat(conversation(), &ChatListState::new()));
    let yesterday = text_at(&ui, "Yesterday").expect("a date over yesterday's message");
    let today = text_at(&ui, "Today").expect("and over today's");
    assert!(yesterday.y < today.y);
    // Older days are dated, with a year when it is not this one.
    let strings = ChatStrings::default();
    assert!(day_label(20_773 - 3, 20_773, &strings).contains("13"));
    assert!(day_label(20_773 - 400, 20_773, &strings).ends_with("2025"));
}

#[test]
fn a_run_names_its_author_once_and_times_its_last_message() {
    let ui = frame(&chat(conversation(), &ChatListState::new()));
    let names = texts(&ui).iter().filter(|(t, _)| t == "Ada").count();
    // Yesterday's message is a run of its own, today's two are one run: two names.
    assert_eq!(names, 2, "{:?}", texts(&ui));
    // The time goes under the last of a run only: 12:00:30 ends it, 12:00 does not show.
    assert!(
        text_at(&ui, "12:00").is_some(),
        "the run of two ends at 12:00:30"
    );
    let times = texts(&ui).iter().filter(|(t, _)| t == "12:00").count();
    assert_eq!(times, 2, "yesterday's run and today's, each timed once");
    // Messages further apart than the window are two runs.
    let a = ChatMessage::incoming("a", "ada", "", NOON);
    let b = ChatMessage::incoming("b", "ada", "", NOON + 301);
    assert!(!same_run(&a, &b, GROUP_WINDOW));
    let c = ChatMessage::incoming("c", "ada", "", NOON + 300);
    assert!(same_run(&a, &c, GROUP_WINDOW));
    let d = ChatMessage::incoming("d", "bob", "", NOON + 1);
    assert!(!same_run(&a, &d, GROUP_WINDOW), "another sender");
    let e = ChatMessage::outgoing("e", "", NOON + 1);
    assert!(!same_run(&a, &e, GROUP_WINDOW), "the reader");
}

#[test]
fn outgoing_bubbles_hang_from_the_end_and_mirror_right_to_left() {
    let place = |theme: Theme| {
        let ui = frame_with(
            &chat(conversation(), &ChatListState::new()),
            Size::new(400.0, 600.0),
            &Runtime::default(),
            &theme,
        );
        let mine = text_at(&ui, "Both green.").expect("drawn");
        let theirs = text_at(&ui, "And the tests?").expect("drawn");
        (mine.x, theirs.x)
    };
    let (mine, theirs) = place(Theme::light());
    assert!(
        mine > theirs && mine > 200.0,
        "left to right, mine on the right: {mine} {theirs}"
    );
    let (mine, theirs) = place(Theme::light().rtl());
    assert!(
        mine < theirs && mine < 200.0,
        "right to left, mine on the left: {mine} {theirs}"
    );
}

#[test]
fn the_bubbles_take_the_theme_and_the_caller_s_colours() {
    let theme = Theme::light();
    let ui = frame(&chat(conversation(), &ChatListState::new()));
    assert!(
        !rect_fill(&ui, theme.primary).is_empty(),
        "mine in the accent"
    );
    assert!(
        !rect_fill(&ui, theme.scheme.surface_container_high).is_empty(),
        "theirs on a container"
    );
    let pink = Color::rgb(1.0, 0.4, 0.7);
    let styled = chat(conversation(), &ChatListState::new()).style(ChatTheme {
        outgoing_color: Some(pink),
        ..ChatTheme::default()
    });
    let ui = frame(&styled);
    assert!(!rect_fill(&ui, pink).is_empty(), "the caller's colour wins");
    let mut themed = Theme::light();
    themed.widgets.chat.incoming_color = Some(pink);
    let ui = frame_with(
        &chat(conversation(), &ChatListState::new()),
        Size::new(400.0, 600.0),
        &Runtime::default(),
        &themed,
    );
    assert!(!rect_fill(&ui, pink).is_empty(), "then the theme's");
}

#[test]
fn a_failed_message_offers_a_retry_with_its_id() {
    let ui = frame(&chat(conversation(), &ChatListState::new()));
    assert!(text_at(&ui, "Not sent").is_some());
    assert!(clicks(&ui).contains(&Msg::Retry("5".into())));
}

#[test]
fn a_bubble_reads_as_one_item_with_who_when_and_where() {
    let ui = frame(&chat(conversation(), &ChatListState::new()));
    let labels: Vec<String> = ui
        .semantics()
        .iter()
        .filter_map(|(_, _, s)| s.label.clone())
        .collect();
    assert!(
        labels
            .iter()
            .any(|l| l.contains("Ada, 12:00: Did the build pass?")),
        "{labels:?}"
    );
    assert!(
        labels.iter().any(|l| l == "You, 12:01, Read: Both green."),
        "{labels:?}"
    );
    assert!(labels.iter().any(|l| l == "Today"), "the date is a heading");
    // Said once, by the item: the words inside are not read again on their own.
    for inside in ["Both green.", "And the tests?", "Ada", "A", "12:00"] {
        assert!(
            !labels
                .iter()
                .any(|l| l == inside || l.ends_with(&format!("\n{inside}"))),
            "{inside:?} read twice: {labels:?}"
        );
    }
    // A failed message is the exception that keeps a control: its retry is its own node,
    // and "Not sent" is still said once, by the item.
    assert!(
        labels
            .iter()
            .any(|l| l == "You, 12:01, Not sent: Pushing now."),
        "{labels:?}"
    );
    assert!(labels.iter().any(|l| l == "Retry"), "{labels:?}");
    assert!(
        !labels
            .iter()
            .any(|l| l == "Pushing now." || l == "Not sent"),
        "{labels:?}"
    );
}

#[test]
fn only_the_rows_in_view_are_built() {
    let built = Rc::new(Cell::new(0usize));
    let messages: Vec<ChatMessage> = (0..10_000)
        .map(|i| ChatMessage::incoming(i.to_string(), "ada", format!("Message {i}"), NOON + i))
        .collect();
    let counter = built.clone();
    let list = chat(Rc::new(messages), &ChatListState::new()).attachment(move |_m| {
        counter.set(counter.get() + 1);
        None::<Container<Msg>>
    });
    let ui = frame(&list);
    let rows = built.get();
    assert!(rows > 3 && rows < 60, "{rows} of 10 000 rows built");
    assert!(
        text_at(&ui, "Message 9999").is_some(),
        "the newest is shown"
    );
    assert!(text_at(&ui, "Message 0").is_none(), "the oldest is not");
}

#[test]
fn the_typing_dots_sit_under_the_newest_message_and_say_who() {
    let list = chat(conversation(), &ChatListState::new()).typing(["Ada"]);
    let ui = frame(&list);
    let labels: Vec<String> = ui
        .semantics()
        .iter()
        .filter_map(|(_, _, s)| s.label.clone())
        .collect();
    assert!(labels.iter().any(|l| l == "Ada is typing"), "{labels:?}");
    assert!(ui.wants_animation(), "the dots keep the frames coming");
    let two = ChatStrings::default().typing(&["Ada".into(), "Bob".into()]);
    assert_eq!(two, "Ada, Bob are typing");
}

#[test]
fn the_dots_rise_in_turn() {
    let (a, _) = TypingIndicator::pose(0, 0.3);
    let (b, _) = TypingIndicator::pose(1, 0.3);
    let (c, _) = TypingIndicator::pose(2, 0.3);
    assert!(a > b && b > c, "{a} {b} {c}: the first leads");
    let (rest, strength) = TypingIndicator::pose(0, 1.0);
    assert_eq!(rest, 0.0, "resting late in the cycle");
    assert!((strength - 0.4).abs() < 1e-5);
}

/// The state says "away" only when it changes, and coming back forgets what was seen.
#[test]
fn the_list_reports_leaving_and_coming_back_only() {
    let here = ChatListState::new();
    assert_eq!(here.report(10.0, Some("9")), None, "still following");
    let away = here.report(200.0, Some("9")).expect("left");
    assert!(away.is_away());
    assert_eq!(away.seen(), Some("9"));
    assert_eq!(
        away.report(400.0, Some("10")),
        None,
        "still away: no report"
    );
    let back = away.report(0.0, Some("10")).expect("came back");
    assert!(!back.is_away() && back.seen().is_none());
}

/// The jump glides back; the reports on the way back are not a reader leaving again.
#[test]
fn jumping_back_is_not_mistaken_for_leaving() {
    let away = ChatListState::new().report(500.0, Some("1")).unwrap();
    let jumping = away.jump_to_newest();
    assert!(!jumping.is_away());
    assert_eq!(jumping.report(300.0, Some("1")), None, "on the way");
    let arrived = jumping.report(0.0, Some("1")).expect("arrived");
    assert!(!arrived.is_away());
    assert!(
        arrived.report(200.0, Some("1")).unwrap().is_away(),
        "and can leave again"
    );
}

#[test]
fn away_from_the_newest_a_button_says_how_many_came_in() {
    let messages = conversation();
    // Away since message 2; 3 (Ada's) and 4, 5 (mine) came after: one new.
    let state = ChatListState::new().report(300.0, Some("2")).unwrap();
    let ui = frame(&chat(messages.clone(), &state));
    assert!(text_at(&ui, "1 new").is_some(), "{:?}", texts(&ui));
    assert!(clicks(&ui)
        .iter()
        .any(|m| matches!(m, Msg::State(s) if !s.is_away())));
    // Following, no button.
    let ui = frame(&chat(messages, &ChatListState::new()));
    assert!(text_at(&ui, "1 new").is_none());
    assert!(!clicks(&ui).iter().any(|m| matches!(m, Msg::State(_))));
}

/// A list sent back to its start glides there from the next frame, and the first generation
/// it is built with asks nothing.
#[test]
fn a_new_jump_generation_glides_the_list_to_its_start() {
    let mut runtime = Runtime::default();
    let id = WidgetId::ROOT;
    runtime.list_to_start(id, 0);
    assert!(
        runtime.to_start_pending.borrow().is_empty(),
        "the first is a count"
    );
    runtime.scroll.insert(id, (0.0, 700.0));
    runtime.list_to_start(id, 0);
    assert!(
        runtime.to_start_pending.borrow().is_empty(),
        "the same is no request"
    );
    runtime.list_to_start(id, 1);
    runtime.apply_scroll_corrections();
    assert_eq!(runtime.scroll_target.get(&id), Some(&(0.0, 0.0)));
}

/// **The anchoring, end to end through a real list**: scrolled up, a message arriving moves the
/// offset by its own measured height; resting at the newest, nothing moves.
#[test]
fn a_message_arriving_keeps_the_reader_s_place_unless_they_follow() {
    let tall = "A message long enough to wrap onto several lines, ".repeat(4);
    let mut messages: Vec<ChatMessage> = (0..40)
        .map(|i| {
            ChatMessage::incoming(
                format!("m{i}"),
                "ada",
                format!("Message {i}"),
                NOON + i * 600,
            )
        })
        .collect();
    let theme = Theme::light();
    let size = Size::new(400.0, 600.0);
    let list_id = |runtime: &Runtime, messages: &[ChatMessage]| {
        let ui = frame_with(
            &chat(Rc::new(messages.to_vec()), &ChatListState::new()),
            size,
            runtime,
            &theme,
        );
        ui.scroll_regions()[0].id
    };

    for (offset, holds) in [(400.0, true), (0.0, false)] {
        let mut runtime = Runtime::default();
        let id = list_id(&runtime, &messages);
        runtime.scroll.insert(id, (0.0, offset));
        let _ = list_id(&runtime, &messages);
        let _ = list_id(&runtime, &messages);
        runtime.apply_scroll_corrections();
        let before = runtime.scroll[&id].1;

        let mut more = messages.clone();
        more.push(ChatMessage::incoming(
            "new",
            "bob",
            tall.clone(),
            NOON + 99_000,
        ));
        let _ = list_id(&runtime, &more);
        let corrections = runtime.scroll_corrections.borrow().clone();
        runtime.apply_scroll_corrections();
        let after = runtime.scroll[&id].1;
        if holds {
            assert!(
                after - before > 60.0,
                "scrolled up, the offset grew by the new message: {before} → {after}, {corrections:?}"
            );
        } else {
            assert_eq!(after, before, "following, nothing moved");
        }
    }
    messages.clear();
}

#[test]
fn the_composer_sends_only_something() {
    let find = |draft: &str| {
        let composer: ChatComposer<Msg> = ChatComposer::new(draft, Msg::Draft).on_send(Msg::Send);
        clicks(&frame(&composer))
    };
    assert!(!find("").contains(&Msg::Send), "nothing to send");
    assert!(!find("   ").contains(&Msg::Send), "spaces are nothing");
    assert!(find("Hi").contains(&Msg::Send));
}

/// Enter sends where the field submits on Enter, and Shift+Enter breaks the line there.
#[test]
fn enter_sends_and_shift_enter_breaks_the_line() {
    let field = |submit: bool| {
        TextField::<Msg>::new("Hi")
            .max_lines(6)
            .submit_on_enter(submit)
            .on_input(Msg::Draft)
            .on_submit(Msg::Send)
    };
    let mut edit = Edit {
        cursor: 2,
        ..Edit::default()
    };
    assert_eq!(field(true).on_edit(&mut edit, &Key::Enter), Some(Msg::Send));
    let mut edit = Edit {
        cursor: 2,
        ..Edit::default()
    };
    assert_eq!(
        field(true).on_edit(&mut edit, &Key::LineBreak),
        Some(Msg::Draft("Hi\n".into()))
    );
    let mut edit = Edit {
        cursor: 2,
        ..Edit::default()
    };
    assert_eq!(
        field(false).on_edit(&mut edit, &Key::Enter),
        Some(Msg::Draft("Hi\n".into())),
        "a plain multi-line field still breaks the line"
    );
    // A single-line field takes Shift+Enter as Enter.
    let single = TextField::<Msg>::new("Hi").on_submit(Msg::Send);
    let mut edit = Edit::default();
    assert_eq!(single.on_edit(&mut edit, &Key::LineBreak), Some(Msg::Send));
}

/// On a desktop theme the composer's field sends on Enter; on a phone's it does not.
#[test]
fn the_composer_sends_on_enter_on_a_desktop_only() {
    let submits = |platform: frus_core::TargetPlatform| {
        let mut theme = Theme::light();
        theme.platform = platform;
        let composer: ChatComposer<Msg> = ChatComposer::new("Hi", Msg::Draft).on_send(Msg::Send);
        let built = composer.inner.into_widget();
        crate::build_deferred(built.as_ref(), &theme, &Runtime::default());
        let mut found = None;
        visit(built.as_ref(), &mut |w| {
            if w.takes_typing() {
                let mut edit = Edit {
                    cursor: 2,
                    ..Edit::default()
                };
                found = w.on_edit(&mut edit, &Key::Enter);
            }
        });
        found
    };
    assert_eq!(submits(frus_core::TargetPlatform::Windows), Some(Msg::Send));
    assert_eq!(
        submits(frus_core::TargetPlatform::Android),
        Some(Msg::Draft("Hi\n".into()))
    );
}

fn visit(w: &dyn Widget<Msg>, f: &mut dyn FnMut(&dyn Widget<Msg>)) {
    f(w);
    for child in w.children() {
        visit(child.as_ref(), f);
    }
}
