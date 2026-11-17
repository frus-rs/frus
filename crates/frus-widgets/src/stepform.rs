// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`StepForm`], [`FormStep`] and [`StepFormState`]: a questionnaire asked one step at a time
//! (milestone 815).
//!
//! # Where the answers live
//!
//! In the application, as a [`StepFormState`]: which step is showing, every answer given so
//! far (kept across the steps, whichever way the reader goes), which steps were done or
//! skipped, and the errors of the step the reader last tried to leave. The form is sent the
//! next state on every move, and the application keeps it.
//!
//! # Validation
//!
//! A step says what it requires with a [`Form`] built from the answers — the same rules a form
//! on one page uses — and with [`FormStep::check`] for what involves several answers at once.
//! **Next is refused while a step is invalid**: pressing it keeps the step and shows its errors,
//! listed over the step and handed to each field through [`StepFormState::error`]. Going back
//! never asks anything.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use frus_core::{Color, Insets, Size, TextStyle};
use frus_layout::{Align, Justify};

use crate::button::{Button, Variant};
use crate::composite::composite_widget;
use crate::container::Container;
use crate::expanded::Expanded;
use crate::flex::Flex;
use crate::form::{ErrorSummary, Form};
use crate::layoutbuilder::LayoutBuilder;
use crate::lazy::Lazy;
use crate::motionpatterns::{MotionPattern, SharedAxis};
use crate::progressbar::LinearProgressIndicator;
use crate::scroll::SingleChildScrollView;
use crate::semantics::Semantics;
use crate::steps::Steps;
use crate::switcher::AnimatedSwitcher;
use crate::text::Text;
use crate::theme::Theme;
use crate::themebuilder::ThemeBuilder;
use crate::widget::Widget;

/// Under this width the progress is a bar rather than the numbered steps, unless told.
const COMPACT: f32 = 600.0;

/// **Where a [`StepForm`] is**: the step showing, the answers, and what the reader has done
/// (milestone 815). Kept by the application, sent anew by the form on every move.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StepFormState {
    current: usize,
    answers: BTreeMap<String, String>,
    /// The errors of the step showing, by field; `""` for the step's own.
    errors: Vec<(String, String)>,
    done: BTreeSet<usize>,
    skipped: BTreeSet<usize>,
    /// Whether the last move went back, which the step transition plays backwards.
    back: bool,
}

impl StepFormState {
    /// A form on its first step, with no answers.
    pub fn new() -> Self {
        Self::default()
    }

    /// The step showing; the number of steps is the review, when there is one.
    pub fn current(&self) -> usize {
        self.current
    }

    /// The answer kept under `key`, or `""`.
    pub fn answer(&self, key: &str) -> &str {
        self.answers.get(key).map_or("", String::as_str)
    }

    /// Every answer, by key.
    pub fn answers(&self) -> &BTreeMap<String, String> {
        &self.answers
    }

    /// The state with `value` kept under `key`. That answer's error goes, since what it was
    /// about has changed.
    #[must_use]
    pub fn with_answer(&self, key: impl Into<String>, value: impl Into<String>) -> Self {
        let key = key.into();
        let mut next = self.clone();
        next.errors.retain(|(field, _)| *field != key);
        next.answers.insert(key, value.into());
        next
    }

    /// The error shown for `field` on the step showing, if Next found one.
    pub fn error(&self, field: &str) -> Option<&str> {
        self.errors
            .iter()
            .find(|(f, _)| f == field)
            .map(|(_, message)| message.as_str())
    }

    /// Every error shown, as `(field, message)`; the step's own under `""`.
    pub fn errors(&self) -> &[(String, String)] {
        &self.errors
    }

    /// Whether step `i` was completed: left with Next, valid.
    pub fn is_done(&self, i: usize) -> bool {
        self.done.contains(&i)
    }

    /// Whether step `i` was skipped.
    pub fn is_skipped(&self, i: usize) -> bool {
        self.skipped.contains(&i)
    }

    /// Whether the last move went back.
    pub fn went_back(&self) -> bool {
        self.back
    }

    /// The state on step `i`, its errors forgotten. A step the reader has not reached yet is
    /// for the application to allow.
    #[must_use]
    pub fn go_to(&self, i: usize) -> Self {
        Self {
            back: i < self.current,
            current: i,
            errors: Vec::new(),
            ..self.clone()
        }
    }

    /// Step `from` done, on to `to`.
    fn advanced(&self, from: usize, to: usize) -> Self {
        let mut next = self.go_to(to);
        next.done.insert(from);
        next.skipped.remove(&from);
        next
    }

    /// Step `from` skipped, on to `to`.
    fn skipped_to(&self, from: usize, to: usize) -> Self {
        let mut next = self.go_to(to);
        next.skipped.insert(from);
        next.done.remove(&from);
        next
    }

    /// The step kept, with `errors` shown.
    fn with_errors(&self, errors: Vec<(String, String)>) -> Self {
        Self {
            errors,
            back: false,
            ..self.clone()
        }
    }
}

/// A step's content, built from the state.
type Build<Msg> = Rc<dyn Fn(&StepFormState) -> Box<dyn Widget<Msg>>>;

/// The rules a step's answers must meet, built from the state.
type Rules = Rc<dyn Fn(&StepFormState) -> Form>;

/// A check across a step's answers: the message to show when it fails.
type Check = Rc<dyn Fn(&StepFormState) -> Option<String>>;

/// **One step of a [`StepForm`]**: a title, an optional subtitle, what it asks, and what it
/// requires (milestone 815).
pub struct FormStep<Msg = crate::callback::Callback> {
    title: String,
    subtitle: Option<String>,
    optional: bool,
    content: Build<Msg>,
    rules: Option<Rules>,
    check: Option<Check>,
}

impl<Msg: 'static> FormStep<Msg> {
    /// A step called `title`, showing what `content` builds from the state — its fields read
    /// their answers with [`StepFormState::answer`] and their errors with
    /// [`StepFormState::error`].
    pub fn new<W: Widget<Msg> + 'static>(
        title: impl Into<String>,
        content: impl Fn(&StepFormState) -> W + 'static,
    ) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            optional: false,
            content: Rc::new(move |state| Box::new(content(state)) as Box<dyn Widget<Msg>>),
            rules: None,
            check: None,
        }
    }

    /// A line under the title.
    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Makes the step **optional**: it says so, and offers Skip beside Next. Skipped, it is
    /// not validated.
    #[must_use]
    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }

    /// What the step requires, as the rules of a [`Form`] over the answers: Next is refused
    /// while any rule fails, and the errors are shown under their fields' keys.
    ///
    /// ```
    /// use frus_widgets::form::{Form, Rule};
    /// use frus_widgets::{FormStep, Text};
    ///
    /// let _step: FormStep<()> = FormStep::new("Contact", |_| Text::new("…"))
    ///     .validate(|state| {
    ///         Form::new().field("email", state.answer("email"), Rule::email("Enter an email"))
    ///     });
    /// ```
    #[must_use]
    pub fn validate(mut self, rules: impl Fn(&StepFormState) -> Form + 'static) -> Self {
        self.rules = Some(Rc::new(rules));
        self
    }

    /// A requirement of the step as a whole — one that involves several answers: `Some`
    /// message refuses Next, and the message is shown over the step.
    #[must_use]
    pub fn check(mut self, check: impl Fn(&StepFormState) -> Option<String> + 'static) -> Self {
        self.check = Some(Rc::new(check));
        self
    }

    /// What is wrong with the step in `state`, as `(field, message)`.
    fn errors(&self, state: &StepFormState) -> Vec<(String, String)> {
        let mut errors: Vec<(String, String)> = Vec::new();
        if let Some(rules) = &self.rules {
            let form = rules(state);
            errors.extend(
                form.errors()
                    .into_iter()
                    .map(|(field, message)| (field.to_owned(), message.to_owned())),
            );
        }
        if let Some(message) = self.check.as_ref().and_then(|check| check(state)) {
            errors.push((String::new(), message));
        }
        errors
    }
}

/// How a [`StepForm`] shows how far along the reader is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StepProgress {
    /// The numbered steps on a wide window, a bar on a narrow one.
    #[default]
    Auto,
    /// The numbered steps with their titles: [`Steps`].
    Steps,
    /// A bar filling as the steps are done.
    Bar,
    /// A dot per step, the current one drawn long.
    Dots,
    /// Nothing but the "Step 2 of 5" over the title.
    None,
}

/// The words a [`StepForm`] says, English unless told. `{n}` and `{total}` are replaced.
#[derive(Clone, Debug, PartialEq)]
pub struct StepFormStrings {
    /// The button back.
    pub back: String,
    /// The button on.
    pub next: String,
    /// The button past an optional step.
    pub skip: String,
    /// The button on the last step, or on the review.
    pub finish: String,
    /// The review step's title.
    pub review: String,
    /// Over the title: step `{n}` of `{total}`.
    pub step_of: String,
    /// Beside it, on an optional step.
    pub optional: String,
}

impl Default for StepFormStrings {
    fn default() -> Self {
        Self {
            back: "Back".into(),
            next: "Next".into(),
            skip: "Skip".into(),
            finish: "Submit".into(),
            review: "Review".into(),
            step_of: "Step {n} of {total}".into(),
            optional: "Optional".into(),
        }
    }
}

/// Defaults for [`StepForm`] (milestone 815). Every field unset is the framework's.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StepFormTheme {
    /// A step's title. Unset, `headline_small`.
    pub title_text_style: Option<TextStyle>,
    /// Its subtitle. Unset, `body_medium` in the muted ink.
    pub subtitle_text_style: Option<TextStyle>,
    /// "Step 2 of 5". Unset, `label_medium` in the muted ink.
    pub caption_text_style: Option<TextStyle>,
    /// The progress's colour. Unset, `primary`.
    pub accent: Option<Color>,
    /// The widest the form grows; it is centred in a wider window. Unset, 720.
    pub max_width: Option<f32>,
    /// The room round it. Unset, sixteen.
    pub padding: Option<Insets>,
}

impl StepFormTheme {
    fn over(self, under: StepFormTheme) -> StepFormTheme {
        StepFormTheme {
            title_text_style: self.title_text_style.or(under.title_text_style),
            subtitle_text_style: self.subtitle_text_style.or(under.subtitle_text_style),
            caption_text_style: self.caption_text_style.or(under.caption_text_style),
            accent: self.accent.or(under.accent),
            max_width: self.max_width.or(under.max_width),
            padding: self.padding.or(under.padding),
        }
    }
}

/// What a form is made of.
struct FormParts<Msg> {
    state: StepFormState,
    on_change: Rc<dyn Fn(StepFormState) -> Msg>,
    steps: Vec<FormStep<Msg>>,
    summary: Option<Build<Msg>>,
    on_complete: Option<Rc<dyn Fn(StepFormState) -> Msg>>,
    progress: StepProgress,
    revisit: bool,
    strings: Rc<StepFormStrings>,
    style: StepFormTheme,
}

/// **A questionnaire asked one step at a time** (milestone 815): a progress at the top, the
/// step's title and what it asks, Back and Next at the bottom, and a review before it is sent.
///
/// ```
/// use frus_widgets::form::{Form, Rule};
/// use frus_widgets::{FormStep, StepForm, StepFormState, Text, TextField};
///
/// let state = StepFormState::new();
/// let form: StepForm<()> = StepForm::new(&state, |_next: StepFormState| ())
///     .step(
///         FormStep::new("Your name", |s: &StepFormState| {
///             let field = TextField::new(s.answer("name"));
///             match s.error("name") {
///                 Some(error) => field.error(error),
///                 None => field,
///             }
///         })
///         .validate(|s| Form::new().field("name", s.answer("name"), Rule::required("Required"))),
///     )
///     .step(FormStep::new("Anything else?", |_| Text::new("…")).optional())
///     .summary(|s: &StepFormState| Text::new(format!("Name: {}", s.answer("name"))))
///     .on_complete(|_done: StepFormState| ());
/// # let _ = form;
/// ```
///
/// - **Next is refused while the step is invalid**, and the step's errors are listed over it
///   and handed to its fields. Back never asks.
/// - **Optional steps** say so and offer Skip.
/// - **The answers are kept** across the steps in the [`StepFormState`], whichever way the
///   reader goes; [`summary`](Self::summary) shows them on a last review step, and
///   [`on_complete`](Self::on_complete) is sent with them.
/// - **The steps slide** along the reading direction, forwards or back, over the theme's
///   motion durations.
/// - **Any width**: the [progress](StepProgress) is the numbered steps on a wide window and a
///   bar on a phone, and the form stays [no wider](StepFormTheme::max_width) than reads well.
///
/// Every part of its look is the caller's [`style`](Self::style), then the theme's
/// [`StepFormTheme`], then the framework's; every word is [`strings`](Self::strings).
pub struct StepForm<Msg = crate::callback::Callback> {
    inner: Lazy<FormParts<Msg>, Msg>,
}

impl<Msg: Clone + 'static> StepForm<Msg> {
    /// A form where `state` says, sending `on_change` the state it should have next.
    pub fn new<R: crate::callback::IntoMsg<Msg>>(
        state: &StepFormState,
        on_change: impl Fn(StepFormState) -> R + 'static,
    ) -> Self {
        Self {
            inner: Lazy::new(
                FormParts {
                    state: state.clone(),
                    on_change: Rc::new(crate::callback::handler1(on_change)),
                    steps: Vec::new(),
                    summary: None,
                    on_complete: None,
                    progress: StepProgress::Auto,
                    revisit: true,
                    strings: Rc::new(StepFormStrings::default()),
                    style: StepFormTheme::default(),
                },
                build_form::<Msg>,
            ),
        }
    }

    fn p(&mut self) -> &mut FormParts<Msg> {
        self.inner.parts_mut()
    }

    /// Adds a step, after the others.
    #[must_use]
    pub fn step(mut self, step: FormStep<Msg>) -> Self {
        self.p().steps.push(step);
        self
    }

    /// Adds steps, after the others.
    #[must_use]
    pub fn steps(mut self, steps: impl IntoIterator<Item = FormStep<Msg>>) -> Self {
        self.p().steps.extend(steps);
        self
    }

    /// A **review** step after the others, showing what `summary` builds from the answers; its
    /// button sends the form. Without one, the last step's button does.
    #[must_use]
    pub fn summary<W: Widget<Msg> + 'static>(
        mut self,
        summary: impl Fn(&StepFormState) -> W + 'static,
    ) -> Self {
        self.p().summary = Some(Rc::new(move |state| {
            Box::new(summary(state)) as Box<dyn Widget<Msg>>
        }));
        self
    }

    /// What finishing sends, with the final state — every answer in it.
    #[must_use]
    pub fn on_complete<R: crate::callback::IntoMsg<Msg>>(
        mut self,
        on_complete: impl Fn(StepFormState) -> R + 'static,
    ) -> Self {
        self.p().on_complete = Some(Rc::new(crate::callback::handler1(on_complete)));
        self
    }

    /// How the progress is shown; see [`StepProgress`].
    #[must_use]
    pub fn progress(mut self, progress: StepProgress) -> Self {
        self.p().progress = progress;
        self
    }

    /// Whether a step already done can be gone back to by pressing its number. On, unless
    /// told.
    #[must_use]
    pub fn revisit(mut self, revisit: bool) -> Self {
        self.p().revisit = revisit;
        self
    }

    /// The words it says, instead of the English ones.
    #[must_use]
    pub fn strings(mut self, strings: StepFormStrings) -> Self {
        self.p().strings = Rc::new(strings);
        self
    }

    /// Its look, over the theme's [`StepFormTheme`].
    #[must_use]
    pub fn style(mut self, style: StepFormTheme) -> Self {
        self.p().style = style;
        self
    }
}

composite_widget!(StepForm);

/// The form's look, every word decided.
#[derive(Clone, Copy, Debug)]
struct Look {
    title: TextStyle,
    subtitle: TextStyle,
    caption: TextStyle,
    muted: Color,
    accent: Color,
    track: Color,
    max_width: f32,
    padding: Insets,
}

impl Look {
    fn of(caller: StepFormTheme, theme: &Theme) -> Self {
        let t = caller.over(theme.widgets.step_form);
        let scale = crate::theme::type_scale(Some(theme));
        Self {
            title: t.title_text_style.unwrap_or(scale.headline_small),
            subtitle: t.subtitle_text_style.unwrap_or(scale.body_medium),
            caption: t.caption_text_style.unwrap_or(scale.label_medium),
            muted: theme.muted,
            accent: t.accent.unwrap_or(theme.primary),
            track: theme.scheme.surface_container_highest,
            max_width: t.max_width.unwrap_or(720.0),
            padding: t.padding.unwrap_or(Insets::uniform(16.0)),
        }
    }
}

/// Everything one build of a form decided, shared by the closures that need it.
struct Decided<Msg> {
    p: FormParts<Msg>,
    look: Look,
    /// The pages: the steps, then the review if there is one.
    pages: usize,
    current: usize,
}

impl<Msg: Clone + 'static> Decided<Msg> {
    fn on_review(&self, page: usize) -> bool {
        self.p.summary.is_some() && page == self.p.steps.len()
    }

    /// The content of page `page`, scrolling when it is taller than the room.
    fn page(&self, page: usize) -> Box<dyn Widget<Msg>> {
        let content = if self.on_review(page) {
            self.p.summary.as_ref().map(|s| s(&self.p.state))
        } else {
            self.p.steps.get(page).map(|s| (s.content)(&self.p.state))
        };
        let content = content.unwrap_or_else(|| Box::new(Container::new()));
        Box::new(
            SingleChildScrollView::new().child(
                Container::new()
                    .padding_each(4.0, 0.0, 4.0, 0.0)
                    .child(content),
            ),
        )
    }

    /// The progress, for a form `width` wide.
    fn progress(&self, width: f32) -> Option<Box<dyn Widget<Msg>>> {
        let look = self.look;
        let mode = match self.p.progress {
            StepProgress::Auto if width < COMPACT => StepProgress::Bar,
            StepProgress::Auto => StepProgress::Steps,
            other => other,
        };
        let state = &self.p.state;
        let done = (0..self.pages).filter(|&i| state.is_done(i)).count();
        Some(match mode {
            StepProgress::Steps => {
                let mut labels: Vec<String> =
                    self.p.steps.iter().map(|s| s.title.clone()).collect();
                if self.p.summary.is_some() {
                    labels.push(self.p.strings.review.clone());
                }
                let mut steps = Steps::new(labels)
                    .current(self.current)
                    .color(look.accent)
                    .completed((0..self.pages).map(|i| state.is_done(i)))
                    .skipped((0..self.pages).map(|i| state.is_skipped(i)));
                if !state.errors.is_empty() {
                    steps = steps.error(self.current);
                }
                if self.p.revisit {
                    let on_change = self.p.on_change.clone();
                    let state = state.clone();
                    let reachable: Vec<bool> = (0..self.pages)
                        .map(|i| i < self.current || state.is_done(i) || state.is_skipped(i))
                        .collect();
                    steps = steps.on_tap_where(
                        move |i| on_change(state.go_to(i)),
                        move |i| reachable.get(i).copied().unwrap_or(false),
                    );
                }
                Box::new(steps)
            }
            StepProgress::Bar => Box::new(
                LinearProgressIndicator::new(done as f32 / self.pages.max(1) as f32)
                    .color(look.accent)
                    .track_color(look.track),
            ),
            StepProgress::Dots => {
                let mut row: Flex<Msg> = Flex::row().gap(6.0).justify(Justify::Center);
                for i in 0..self.pages {
                    let current = i == self.current;
                    let colour = if current || state.is_done(i) {
                        look.accent
                    } else {
                        look.track
                    };
                    row = row.child(
                        Container::new()
                            .width(if current { 24.0 } else { 8.0 })
                            .height(8.0)
                            .radius(4.0)
                            .color(colour),
                    );
                }
                Box::new(row)
            }
            StepProgress::None | StepProgress::Auto => return None,
        })
    }

    /// The whole form, `width` wide.
    fn form(self: &Rc<Self>, width: f32) -> Box<dyn Widget<Msg>> {
        let look = self.look;
        let p = &self.p;
        let state = &p.state;
        let cur = self.current;
        let review = self.on_review(cur);
        let last = cur + 1 >= self.pages;
        let step = p.steps.get(cur).filter(|_| !review);
        let strings = &p.strings;

        // What Next does is decided now, from the answers as they stand: refused with the
        // step's errors, or on — to the next page, or to the end.
        let errors = step.map(|s| s.errors(state)).unwrap_or_default();
        let forward = if !errors.is_empty() {
            Some((p.on_change)(state.with_errors(errors)))
        } else if last {
            let mut finished = state.with_errors(Vec::new());
            if !review {
                finished.done.insert(cur);
            }
            p.on_complete.as_ref().map(|done| done(finished))
        } else {
            Some((p.on_change)(state.advanced(cur, cur + 1)))
        };

        let mut column: Flex<Msg> = Flex::column().align(Align::Stretch).gap(12.0);
        if let Some(progress) = self.progress(width) {
            column = column.child_boxed(progress);
        }
        let mut caption = strings
            .step_of
            .replace("{n}", &(cur + 1).to_string())
            .replace("{total}", &self.pages.to_string());
        if step.is_some_and(|s| s.optional) {
            caption = format!("{caption} · {}", strings.optional);
        }
        let mut heading: Flex<Msg> = Flex::column()
            .gap(4.0)
            .align(Align::Start)
            .child(Text::styled(caption, look.caption).color(look.muted));
        let title = match step {
            Some(step) => step.title.clone(),
            None => strings.review.clone(),
        };
        heading = heading.child(Semantics::heading(Text::styled(title, look.title)));
        if let Some(subtitle) = step.and_then(|s| s.subtitle.clone()) {
            heading = heading.child(Text::styled(subtitle, look.subtitle).color(look.muted));
        }
        column = column.child(heading);
        if !state.errors.is_empty() {
            column = column.child(ErrorSummary::new(
                state.errors.iter().map(|(_, message)| message.clone()),
            ));
        }

        let pages = self.clone();
        let switcher = AnimatedSwitcher::patterned(
            MotionPattern::SharedAxis(SharedAxis::X),
            cur,
            move |&page: &usize| pages.page(page),
        )
        .reverse(state.back)
        .fill_color(Color::TRANSPARENT);
        column = column.child(Expanded::new(switcher));

        let mut buttons: Flex<Msg> = Flex::row().gap(8.0).align(Align::Center);
        if cur > 0 {
            buttons = buttons.child(
                Button::new(strings.back.clone())
                    .variant(Variant::Text)
                    .on_press((p.on_change)(state.go_to(cur - 1))),
            );
        }
        buttons = buttons.child(Expanded::new(Container::new()));
        if step.is_some_and(|s| s.optional) && !last {
            buttons = buttons.child(
                Button::new(strings.skip.clone())
                    .variant(Variant::Text)
                    .on_press((p.on_change)(state.skipped_to(cur, cur + 1))),
            );
        }
        let label = if last {
            strings.finish.clone()
        } else {
            strings.next.clone()
        };
        let mut next = Button::new(label).enabled(forward.is_some());
        if let Some(forward) = forward {
            next = next.on_press(forward);
        }
        column = column.child(buttons.child(next));

        // No wider than reads well: centred in a wider window.
        let side = ((width - look.max_width) / 2.0).max(0.0);
        Box::new(column.padding_each(
            look.padding.top,
            look.padding.right + side,
            look.padding.bottom,
            look.padding.left + side,
        ))
    }
}

fn build_form<Msg: Clone + 'static>(p: FormParts<Msg>) -> Box<dyn Widget<Msg>> {
    Box::new(ThemeBuilder::boxed(move |theme: &Theme| {
        let look = Look::of(p.style, theme);
        let pages = (p.steps.len() + usize::from(p.summary.is_some())).max(1);
        let current = p.state.current.min(pages - 1);
        let decided = Rc::new(Decided {
            p,
            look,
            pages,
            current,
        });
        Box::new(LayoutBuilder::new(move |size: Size| decided.form(size.width)).filling())
            as Box<dyn Widget<Msg>>
    }))
}

#[cfg(test)]
mod tests;
