// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The questionnaire's tests (milestone 815): validation refusing Next, skipping, going back,
//! the review, the answers kept, the progress at each width, and what a reader hears.

use frus_core::{Point, Primitive, Rect, Size};

use super::*;
use crate::form::Rule;
use crate::{build_ui, Runtime, Ui};

#[derive(Clone, Debug, PartialEq)]
enum Msg {
    Change(StepFormState),
    Done(StepFormState),
}

fn form(state: &StepFormState) -> StepForm<Msg> {
    StepForm::new(state, Msg::Change)
        .step(
            FormStep::new("Your name", |s: &StepFormState| {
                Text::new(format!("name={}", s.answer("name")))
            })
            .subtitle("As it should be printed")
            .validate(|s| {
                Form::new().field("name", s.answer("name"), Rule::required("Name is required"))
            }),
        )
        .step(FormStep::new("Notes", |_| Text::new("anything else")).optional())
        .step(
            FormStep::new("Contact", |s: &StepFormState| {
                Text::new(format!("email={}", s.answer("email")))
            })
            .validate(|s| {
                Form::new().field("email", s.answer("email"), Rule::email("Enter an email"))
            })
            .check(|s| {
                (s.answer("email") == "taken@example.com").then(|| "That address is taken".into())
            }),
        )
        .summary(|s: &StepFormState| Text::new(format!("Review: {}", s.answer("name"))))
        .on_complete(Msg::Done)
}

fn frame_at(widget: &StepForm<Msg>, width: f32) -> Ui<Msg> {
    build_ui(
        widget,
        Size::new(width, 700.0),
        &Runtime::default(),
        &Theme::light(),
    )
}

fn frame(widget: &StepForm<Msg>) -> Ui<Msg> {
    frame_at(widget, 900.0)
}

fn texts(ui: &Ui<Msg>) -> Vec<String> {
    ui.scene()
        .primitives()
        .iter()
        .filter_map(|p| match p {
            Primitive::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn has(ui: &Ui<Msg>, words: &str) -> bool {
    texts(ui).iter().any(|t| t == words)
}

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

/// The state the button labelled `label` sends, found by its position in the footer: the
/// buttons' messages in the order the footer holds them.
fn changes(ui: &Ui<Msg>) -> Vec<StepFormState> {
    clicks(ui)
        .into_iter()
        .filter_map(|m| match m {
            Msg::Change(s) => Some(s),
            Msg::Done(_) => None,
        })
        .collect()
}

#[test]
fn the_first_step_shows_its_title_subtitle_and_place() {
    let ui = frame(&form(&StepFormState::new()));
    assert!(has(&ui, "Your name"));
    assert!(has(&ui, "As it should be printed"));
    assert!(
        has(&ui, "Step 1 of 4"),
        "three steps and the review: {:?}",
        texts(&ui)
    );
    assert!(has(&ui, "Next"));
    assert!(!has(&ui, "Back"), "nothing to go back to");
}

/// **Next is refused while the step is invalid**: it sends the same step with its errors.
#[test]
fn an_invalid_step_refuses_next_and_shows_why() {
    let ui = frame(&form(&StepFormState::new()));
    let next = changes(&ui);
    assert_eq!(next.len(), 1, "only Next: {next:?}");
    let refused = &next[0];
    assert_eq!(refused.current(), 0, "still on the first step");
    assert_eq!(refused.error("name"), Some("Name is required"));
    // Shown: listed over the step.
    let ui = frame(&form(refused));
    assert!(
        texts(&ui).iter().any(|t| t.contains("Name is required")),
        "{:?}",
        texts(&ui)
    );
    // Typing the answer clears that error.
    let typed = refused.with_answer("name", "Ada");
    assert_eq!(typed.error("name"), None);
    assert_eq!(typed.answer("name"), "Ada");
}

#[test]
fn a_valid_step_goes_on_and_is_marked_done() {
    let state = StepFormState::new().with_answer("name", "Ada");
    let ui = frame(&form(&state));
    let next = changes(&ui);
    let on = next
        .iter()
        .find(|s| s.current() == 1)
        .expect("Next goes on");
    assert!(on.is_done(0));
    assert!(!on.went_back());
    assert_eq!(on.answer("name"), "Ada", "the answers travel with it");
}

#[test]
fn an_optional_step_offers_skip_and_back() {
    let state = StepFormState::new().with_answer("name", "Ada").go_to(1);
    let ui = frame(&form(&state));
    assert!(has(&ui, "Skip"));
    assert!(
        texts(&ui).iter().any(|t| t.contains("Optional")),
        "{:?}",
        texts(&ui)
    );
    let moves = changes(&ui);
    let back = moves.iter().find(|s| s.current() == 0).expect("Back");
    assert!(back.went_back());
    assert_eq!(back.answer("name"), "Ada", "going back keeps the answers");
    let skipped = moves
        .iter()
        .find(|s| s.current() == 2 && s.is_skipped(1))
        .expect("Skip");
    assert!(!skipped.is_done(1));
}

#[test]
fn a_step_wide_check_refuses_too() {
    let state = StepFormState::new()
        .with_answer("email", "taken@example.com")
        .go_to(2);
    let ui = frame(&form(&state));
    let refused = changes(&ui)
        .into_iter()
        .find(|s| s.current() == 2)
        .expect("Next refused");
    assert_eq!(refused.error(""), Some("That address is taken"));
}

#[test]
fn the_review_sends_every_answer_on_submit() {
    let state = StepFormState::new()
        .with_answer("name", "Ada")
        .with_answer("email", "ada@example.com")
        .go_to(3);
    let ui = frame(&form(&state));
    assert!(has(&ui, "Review: Ada"), "{:?}", texts(&ui));
    assert!(has(&ui, "Submit"));
    let done = clicks(&ui)
        .into_iter()
        .find_map(|m| match m {
            Msg::Done(s) => Some(s),
            _ => None,
        })
        .expect("Submit completes");
    assert_eq!(done.answer("email"), "ada@example.com");
    assert_eq!(done.answer("name"), "Ada");
}

#[test]
fn without_a_review_the_last_step_completes() {
    let state = StepFormState::new().with_answer("x", "1");
    let single = StepForm::new(&state, Msg::Change)
        .step(FormStep::new("Only", |_| Text::new("…")))
        .on_complete(Msg::Done);
    let ui = frame(&single);
    assert!(has(&ui, "Submit"));
    let done = clicks(&ui)
        .into_iter()
        .find_map(|m| match m {
            Msg::Done(s) => Some(s),
            _ => None,
        })
        .expect("completes");
    assert!(done.is_done(0));
}

/// A wide window shows the numbered steps, a phone a bar; either can be asked for.
#[test]
fn the_progress_follows_the_width() {
    let wide = frame_at(&form(&StepFormState::new()), 900.0);
    let narrow = frame_at(&form(&StepFormState::new()), 360.0);
    let roles = |ui: &Ui<Msg>| -> Vec<frus_core::Role> {
        ui.semantics().iter().map(|(_, _, s)| s.role).collect()
    };
    // The numbered steps draw their titles under the markers; the bar does not.
    assert!(has(&wide, "Notes"), "the steps' labels: {:?}", texts(&wide));
    assert!(!has(&narrow, "Notes"), "a bar has no labels");
    assert!(roles(&wide).contains(&frus_core::Role::ProgressBar));
    assert!(roles(&narrow).contains(&frus_core::Role::ProgressBar));
    let forced = frame_at(
        &form(&StepFormState::new()).progress(StepProgress::Steps),
        360.0,
    );
    assert!(has(&forced, "Notes"));
}

/// The form stays no wider than it reads well, centred in a wide window.
#[test]
fn a_wide_window_centres_the_form() {
    let ui = frame_at(&form(&StepFormState::new()), 1600.0);
    let title_x = ui
        .scene()
        .primitives()
        .iter()
        .find_map(|p| match p {
            Primitive::Text { text, position, .. } if text == "Your name" => Some(position.x),
            _ => None,
        })
        .expect("the title");
    assert!(title_x > 400.0, "centred, not at the left edge: {title_x}");
}

/// The step's title is a heading a reader can jump to.
#[test]
fn the_title_is_a_heading() {
    let ui = frame(&form(&StepFormState::new()));
    assert!(ui.semantics().iter().any(|(_, _, s)| {
        s.role == frus_core::Role::Heading && s.label.as_deref() == Some("Your name")
    }));
}

/// The steps already done can be gone back to by their numbers; the ones ahead cannot.
#[test]
fn done_steps_can_be_revisited_from_the_progress() {
    let state = StepFormState::new().with_answer("name", "Ada").go_to(2);
    let mut state = state;
    state.done.insert(0);
    state.skipped.insert(1);
    let ui = frame(&form(&state));
    let targets: Vec<usize> = changes(&ui).iter().map(|s| s.current()).collect();
    assert!(targets.contains(&0), "step 1, done: {targets:?}");
    assert!(!targets.contains(&3), "the review is ahead: {targets:?}");
}
