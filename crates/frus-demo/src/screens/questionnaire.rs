// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! A questionnaire asked one step at a time (milestone 815): required answers that refuse
//! Next, an optional step that can be skipped, a review of every answer, and what was sent.

use frus_widgets::{FormStep, StepForm, StepFormState, Text};

use crate::prelude::*;

/// The screen.
pub(crate) struct QuestionnairePage;

/// What the screen keeps.
pub(crate) struct QuestionnaireState {
    form: StepFormState,
    sent: Option<StepFormState>,
}

impl StatefulWidget for QuestionnairePage {
    type State = QuestionnaireState;

    fn create_state(&self) -> QuestionnaireState {
        QuestionnaireState {
            form: StepFormState::new(),
            sent: None,
        }
    }
}

impl State for QuestionnaireState {
    type Widget = QuestionnairePage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let theme = cx.theme().clone();
        let router = cx.router();
        let app_bar = AppBar::new().title(Text::new("Questionnaire")).leading(
            IconButton::new(Icons::ARROW_BACK).on_press(on(move || {
                router.pop();
            })),
        );

        if let Some(sent) = &self.sent {
            let summary = sent
                .answers()
                .iter()
                .fold(Flex::column().gap(8.0).padding(24.0), |column, (k, v)| {
                    column.child(Text::new(format!("{k}: {v}")))
                })
                .child(button(
                    "Start again",
                    cx.callback(|s| {
                        s.sent = None;
                        s.form = StepFormState::new();
                    }),
                ));
            return Scaffold::new()
                .background(theme.background)
                .app_bar(app_bar)
                .body(summary)
                .build();
        }

        // Each step's content is built from the state the form hands it; the fields send their
        // answers back through this screen's handlers.
        let handle = cx.handle();
        let about_you = {
            let handle = handle.clone();
            move |s: &StepFormState| -> Box<dyn Widget> {
                Box::new(
                    Flex::column()
                        .gap(12.0)
                        .child(field_of(&handle, s, "name", "Full name"))
                        .child(field_of(&handle, s, "email", "Email")),
                )
            }
        };
        let notes = {
            let handle = handle.clone();
            move |s: &StepFormState| -> Box<dyn Widget> {
                Box::new(field_of(&handle, s, "notes", "Anything we should know?").max_lines(5))
            }
        };
        let rating = {
            let handle = handle.clone();
            move |s: &StepFormState| -> Box<dyn Widget> {
                Box::new(
                    Flex::column()
                        .gap(12.0)
                        .child(Text::new(
                            "How likely are you to recommend us, from 0 to 10?",
                        ))
                        .child(field_of(&handle, s, "score", "Score").digits_only()),
                )
            }
        };

        let form = StepForm::new(
            &self.form,
            cx.handler(|s, state: StepFormState| s.form = state),
        )
        .step(
            FormStep::new("About you", about_you)
                .subtitle("So that we can reply")
                .validate(|s| {
                    Form::new()
                        .field("name", s.answer("name"), Rule::required("Enter your name"))
                        .field(
                            "email",
                            s.answer("email"),
                            Rule::all([
                                Rule::required("Enter your email"),
                                Rule::email("Enter a valid email address"),
                            ]),
                        )
                }),
        )
        .step(FormStep::new("Notes", notes).optional())
        .step(FormStep::new("Your score", rating).check(
            |s| match s.answer("score").parse::<u32>() {
                Ok(n) if n <= 10 => None,
                _ => Some("Give a score from 0 to 10".into()),
            },
        ))
        .summary(|s: &StepFormState| {
            s.answers()
                .iter()
                .fold(Flex::column().gap(8.0), |column, (k, v)| {
                    column.child(Text::new(format!("{k}: {v}")))
                })
        })
        .on_complete(cx.handler(|s, done: StepFormState| s.sent = Some(done)));

        Scaffold::new()
            .background(theme.background)
            .app_bar(app_bar)
            .body(form)
            .build()
    }
}

/// A field bound to the answer `key` through `handle`, showing its error once Next has
/// refused it.
fn field_of(
    handle: &frus_widgets::StateHandle<QuestionnaireState>,
    state: &StepFormState,
    key: &'static str,
    label: &str,
) -> TextField {
    let handle = handle.clone();
    let field = TextField::new(state.answer(key))
        .label(label)
        .on_input(move |text: String| {
            let handle = handle.clone();
            Callback::new(move || {
                let text = text.clone();
                handle.set_state(|s| s.form = s.form.with_answer(key, text.clone()))
            })
        });
    match state.error(key) {
        Some(error) => field.error(error),
        None => field,
    }
}
