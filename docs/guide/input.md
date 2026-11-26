# Taking input: fields, controls and forms

A control in frus shows a value that **your code** holds, and tells your code when the person
changes it. It does not keep a value of its own that you then have to read back: the state is
yours, and the control is how it is shown. That is what lets a form be checked, reset or filled
from anywhere.

## A text field

`TextField::new(value)` shows `value`; `on_input` is called with the new text at every key.
In a function component, the text lives in a `use_state`:

```rust
use frus::{component, Column, Text, TextField, Widget};

fn greeting() -> Box<dyn Widget> {
    Box::new(component(|cx| {
        let name = cx.use_state(String::new);
        let typed = name.clone();
        Box::new(
            Column::new()
                .spacing(12.0)
                .child(
                    TextField::new(name.get())
                        .label("Your name")
                        .on_input(move |text: String| typed.set(text)),
                )
                .child(Text::new(format!("Hello, {}", name.get()))),
        )
    }))
}
```

A field says what it is for, and the phone's keyboard follows: `.keyboard_type(KeyboardType::Email)`,
`.obscure(true)` for a password, `.prefix_icon(Icons::SEARCH)`, `.placeholder("…")`,
`.error("Enter an address")` under it, `.on_submit(..)` when the person presses Enter or the
keyboard's action key.

```rust
use frus::{component, Icons, KeyboardType, TextField, Widget};

fn email_field() -> Box<dyn Widget> {
    Box::new(component(|cx| {
        let email = cx.use_state(String::new);
        let typed = email.clone();
        let value = email.get();
        let mut field = TextField::new(&value)
            .label("Email")
            .keyboard_type(KeyboardType::Email)
            .prefix_icon(Icons::EMAIL)
            .on_input(move |text: String| typed.set(text));
        if !value.is_empty() && !value.contains('@') {
            field = field.error("Enter an address like name@example.com");
        }
        Box::new(field)
    }))
}
```

## When code writes the text: a controller

When something other than the keyboard changes the text — a "Clear" button, a suggestion that
fills the field — give the field a `TextEditingController`. What is typed lands in it, and what
the program writes appears in the field:

```rust
use frus::{component, Button, Column, Row, TextField, Widget};

fn message_box() -> Box<dyn Widget> {
    Box::new(component(|cx| {
        let message = cx.use_text_controller("");
        let (shout, clear) = (message.clone(), message.clone());
        Box::new(
            Column::new()
                .spacing(8.0)
                .child(TextField::new("").label("Message").controller(&message))
                .child(
                    Row::new()
                        .spacing(8.0)
                        .child(Button::new("Upper case").on_press(move || {
                            shout.set_text(shout.text().to_uppercase())
                        }))
                        .child(Button::new("Clear").on_press(move || clear.clear())),
                ),
        )
    }))
}
```

## Choices: checkbox, switch, radio, slider, dropdown

Each takes the current value and calls you with the new one. With a `StatefulWidget`, the
values are fields of the state, and `cx.handler(..)` turns "change this field" into the
control's callback:

```rust
use frus::{
    Align, Checkbox, Column, DropdownButton, RadioGroup, Slider, State, StateContext,
    StatefulWidget, Switch, Text, Widget,
};

const SIZES: [&str; 3] = ["Small", "Medium", "Large"];

struct Preferences;

#[derive(Default)]
struct PreferencesState {
    newsletter: bool,
    dark: bool,
    size: usize,
    volume: f32,
    country: usize,
    country_open: bool,
}

impl StatefulWidget for Preferences {
    type State = PreferencesState;
    fn create_state(&self) -> PreferencesState {
        PreferencesState { volume: 0.5, ..Default::default() }
    }
}

const COUNTRIES: [&str; 3] = ["Côte d'Ivoire", "Ghana", "Senegal"];

impl State for PreferencesState {
    type Widget = Preferences;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let sizes = SIZES.iter().fold(
            RadioGroup::new(self.size, cx.handler(|s: &mut Self, i: usize| s.size = i)),
            |group, size| group.option(*size),
        );
        let country = DropdownButton::new(
            COUNTRIES[self.country],
            cx.callback(|s: &mut Self| s.country_open = !s.country_open),
        )
        .selected(self.country)
        .options(
            self.country_open,
            &COUNTRIES,
            cx.handler(|s: &mut Self, i: usize| {
                s.country = i;
                s.country_open = false;
            }),
        );

        Box::new(
            Column::new()
                .spacing(12.0)
                .cross_axis_alignment(Align::Start)
                .child(
                    Checkbox::new(self.newsletter)
                        .label("Send me the newsletter")
                        .on_toggle(cx.handler(|s: &mut Self, on: bool| s.newsletter = on)),
                )
                .child(
                    Switch::new(self.dark)
                        .on_toggle(cx.handler(|s: &mut Self, on: bool| s.dark = on)),
                )
                .child(sizes)
                .child(
                    Slider::new(self.volume)
                        .on_change(cx.handler(|s: &mut Self, v: f32| s.volume = v)),
                )
                .child(Text::new(format!("Volume {:.0}%", self.volume * 100.0)))
                .child(country),
        )
    }
}
```

The dropdown is controlled twice over: the state holds the choice **and** whether its list is
open. That is what lets a screen close it, or open it from a keyboard shortcut.

A control is turned off with `.enabled(false)`; it then looks disabled and ignores the person.
There are more: `RangeSlider`, `SegmentedButton`, `Rating`, `Stepper`, date and time pickers,
`Autocomplete`, `SearchBar`. The widget gallery (`frus-examples/widgets`) shows each one.

## A one-time code, a labelled field, a group

`OtpInput` is the row of boxes a code sent by SMS is typed into. Typing moves to the next box,
backspace to the one before, and a paste fills them all. It tells the platform it holds a
one-time code, so Android's autofill and the browser offer the code from the message, and a
phone shows its number keypad. `.on_completed` hears the edit that fills the last box:

```rust
use frus::{component, Field, OtpInput, Widget};

fn verify() -> Box<dyn Widget> {
    Box::new(component(|cx| {
        let code = cx.use_state(String::new);
        let wrong = cx.use_state(|| false);
        let (typed, done, refused) = (code.clone(), code.clone(), wrong.clone());
        let checked = wrong.clone();
        Box::new(
            Field::new(
                OtpInput::new(&code.get(), 6, move |c: String| {
                    typed.set(c);
                    refused.set(false);
                })
                .on_completed(move |c: String| {
                    checked.set(c != "123456");
                    done.set(c);
                })
                .groups([3, 3])
                .error(wrong.get()),
            )
            .label("Verification code")
            .description("We sent it to your phone.")
            .error(wrong.get().then(|| "That code is not right.".to_string())),
        )
    }))
}
```

`.masked(true)` shows dots, `.kind(OtpKind::Alphanumeric)` takes letters too, `.enabled(false)`
greys it out. `Field` puts a label, a description, an error and a required marker around any
control — a field, a dropdown, a group of radio buttons — and a screen reader reads its error out
when it appears.

`InputGroup` joins a field and what is attached to it into one bordered box: words, an icon, a
button. The box lights up as a whole while the field has the focus.

```rust
use frus::{button, component, Addon, Icons, InputGroup, TextField, Widget};

fn website() -> Box<dyn Widget> {
    Box::new(component(|cx| {
        let site = cx.use_state(String::new);
        let typed = site.clone();
        Box::new(
            InputGroup::new(
                TextField::new(&site.get()).on_input(move |text: String| typed.set(text)),
            )
            .leading(Addon::text("https://"))
            .trailing(Addon::icon(Icons::SEARCH))
            .trailing(Addon::widget(button("Go", || {}))),
        )
    }))
}
```

Every colour, size and type of these is the theme's: `theme.widgets.otp_input`,
`theme.widgets.field` and `theme.widgets.input_group`, and each widget's own builders win over
the theme.

## A form, checked

`frus::form` checks a set of values against rules and says what is wrong with each. It only
reads values: it works on whatever the state holds.

```rust
use frus::form::{Form, Rule};

fn sign_up(name: &str, email: &str, password: &str) -> Form {
    Form::new()
        .field("name", name, Rule::required("Enter your name"))
        .field(
            "email",
            email,
            Rule::all([
                Rule::required("Enter your email"),
                Rule::email("Enter a valid email address"),
            ]),
        )
        .field("password", password, Rule::min_len(8, "At least 8 characters"))
}

let form = sign_up("Ada", "ada@example", "short");
assert!(!form.is_valid());
assert_eq!(form.error("password"), Some("At least 8 characters"));
assert_eq!(form.first_invalid(), Some("email"));
```

A screen usually shows the errors only once the person has tried to send, and then moves the
focus to the first field that is wrong. Each field carries a key (`keyed`) so that
`host::focus(key)` can find it:

```rust
use frus::form::{Form, Rule};
use frus::{host, keyed, Button, Column, State, StateContext, StatefulWidget, TextField, Widget};

struct Login;

#[derive(Default)]
struct LoginState {
    email: String,
    password: String,
    submitted: bool,
}

impl LoginState {
    fn form(&self) -> Form {
        Form::new()
            .field("email", &self.email, Rule::email("Enter a valid email address"))
            .field("password", &self.password, Rule::required("Enter your password"))
    }

    fn submit(&mut self) {
        self.submitted = true;
        if let Some(first) = self.form().first_invalid() {
            host::focus(("login", first));
        }
    }
}

impl StatefulWidget for Login {
    type State = LoginState;
    fn create_state(&self) -> LoginState {
        LoginState::default()
    }
}

impl State for LoginState {
    type Widget = Login;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let form = self.form();
        let error = |key| if self.submitted { form.error(key) } else { None };

        let mut email = TextField::new(&self.email)
            .label("Email")
            .on_input(cx.handler(|s: &mut Self, t: String| s.email = t));
        if let Some(message) = error("email") {
            email = email.error(message);
        }
        let mut password = TextField::new(&self.password)
            .label("Password")
            .obscure(true)
            .on_input(cx.handler(|s: &mut Self, t: String| s.password = t));
        if let Some(message) = error("password") {
            password = password.error(message);
        }

        Box::new(
            Column::new()
                .spacing(12.0)
                .child(keyed(("login", "email"), email))
                .child(keyed(("login", "password"), password))
                .child(Button::new("Sign in").on_press(cx.callback(LoginState::submit))),
        )
    }
}
```

The checking is a plain function of the state, so it is tested without a screen — see
[Testing](testing.md). Sending the form to a server is an `async` handler: see
[Talking to a server](network.md).

### Rules of your own, and fields that depend on each other

`Rule::new` turns any check into a rule. `matches` compares a field with one declared before it,
and `field_with` hands a check the fields already declared. `errors()` lists every message, in
order, for an `ErrorSummary` at the head of the form:

```rust
use frus::form::{Form, Rule};
use frus::ErrorSummary;

let no_spaces = Rule::new(|v: &str| v.contains(' ').then(|| "No spaces".to_string()));

let form = Form::new()
    .field("user", "ada lovelace", Rule::all([Rule::required("Required"), no_spaces]))
    .field("password", "correct horse", Rule::min_len(8, "At least 8 characters"))
    .matches("confirm", "correct hose", "password", "The passwords differ")
    .field("start", "2026-03-01", Rule::required("Required"))
    .field_with("end", "2026-01-01", |end, form| {
        (Some(end) < form.value("start")).then(|| "Ends before it starts".to_string())
    });

assert_eq!(form.error("user"), Some("No spaces"));
assert_eq!(form.error("confirm"), Some("The passwords differ"));
assert_eq!(form.error("end"), Some("Ends before it starts"));
let summary: ErrorSummary = ErrorSummary::new(form.errors().into_iter().map(|(_, message)| message));
assert!(!summary.is_empty());
```

`ErrorSummary::links` makes each line a button, to focus the field it is about.

### Helping the person fill it in

A field can say what it holds, so that the keyboard, the system's autofill and password
managers help; it can also refuse characters as they are typed:

```rust
use frus::{
    AutofillGroup, AutofillHint, Capitalization, Column, KeyboardType, TextField, TextInputAction,
};

let address: AutofillGroup = AutofillGroup::new(
    Column::new()
        .spacing(12.0)
        .child(
            TextField::new("")
                .label("Full name")
                .autofill([AutofillHint::Name])
                .capitalization(Capitalization::Words)
                .action(TextInputAction::Next), // the keyboard's key moves to the next field
        )
        .child(
            TextField::new("")
                .label("Postcode")
                .autofill([AutofillHint::PostalCode])
                .input_filter(|c| c.is_ascii_alphanumeric().then(|| c.to_ascii_uppercase()))
                .max_length(8),
        )
        .child(
            TextField::new("")
                .label("Phone")
                .keyboard_type(KeyboardType::Phone)
                .autofill([AutofillHint::TelephoneNumber])
                .helper("We call only about the delivery"),
        )
        .child(TextField::new("").label("Quantity").digits_only()),
);
# let _ = address;
```

`AutofillGroup` tells the system the fields are one form, so it fills them together. On Android
the autofill service offers them; on the web the hints become the input's `autocomplete`.

## A form in steps

`StepForm` asks a long form one step at a time: a progress at the top, Back and Next, a review
at the end. Each step validates with the same `Form` rules, and **Next is refused** while the
step is invalid. The answers live in a `StepFormState` the application keeps:

```rust
use frus::form::{Form, Rule};
use frus::{
    Callback, FormStep, State, StateContext, StatefulWidget, StateHandle, StepForm, StepFormState,
    Text, TextField, Widget,
};

struct Survey;
#[derive(Default)]
struct SurveyState {
    form: StepFormState,
    sent: Option<StepFormState>,
}

impl StatefulWidget for Survey {
    type State = SurveyState;
    fn create_state(&self) -> SurveyState {
        SurveyState::default()
    }
}

/// A field bound to the answer `key`, showing its error once Next has refused it.
fn field(handle: &StateHandle<SurveyState>, form: &StepFormState, key: &'static str) -> TextField {
    let handle = handle.clone();
    let field = TextField::new(form.answer(key)).label(key).on_input(move |text: String| {
        let handle = handle.clone();
        Callback::new(move || {
            let text = text.clone();
            handle.set_state(|s| s.form = s.form.with_answer(key, text))
        })
    });
    match form.error(key) {
        Some(error) => field.error(error),
        None => field,
    }
}

impl State for SurveyState {
    type Widget = Survey;
    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let handle = cx.handle();
        Box::new(
            StepForm::new(&self.form, cx.handler(|s: &mut Self, next: StepFormState| s.form = next))
                .step(
                    FormStep::new("About you", move |form: &StepFormState| field(&handle, form, "name"))
                        .validate(|form| {
                            Form::new().field("name", form.answer("name"), Rule::required("Enter your name"))
                        }),
                )
                .step(FormStep::new("Anything else?", |_| Text::new("Nothing more to ask.")).optional())
                .summary(|form: &StepFormState| Text::new(format!("Name: {}", form.answer("name"))))
                .on_complete(cx.handler(|s: &mut Self, done: StepFormState| s.sent = Some(done))),
        )
    }
}
# let _ = Survey;
```

`FormStep::check` validates what involves several answers at once; `.optional()` offers Skip;
`StepForm::progress` chooses numbered steps, a bar or dots (by default, numbered steps on a wide
window and a bar on a phone).

## Keyboard shortcuts

`Shortcuts` binds a key to an intent and `Actions` says what the intent does. Write the key with
`.primary()` for the platform's own modifier: one binding is Cmd+S on a Mac (in a browser on a
Mac too) and Ctrl+S everywhere else.

```rust
use frus::{Actions, Callback, Intent, KeyStroke, ShortcutKey, Shortcuts, Text, TextField, column};

const SAVE: Intent = Intent("save");

let save = Callback::new(|| println!("saved"));
let screen = Actions::new(
    Shortcuts::new(column![Text::new("Draft"), TextField::new("")])
        .bind(KeyStroke::new(ShortcutKey::Char('s')).primary(), SAVE)
        // The fields' own keys can be moved or switched off below this point.
        .bind(KeyStroke::new(ShortcutKey::Char('d')).primary(), Intent::SELECT_ALL)
        .bind(KeyStroke::new(ShortcutKey::Char('a')).primary(), Intent::DO_NOTHING),
)
.action(SAVE, save);
# let _ = screen;
```

A text field and a selection area answer `Intent::COPY`, `CUT`, `PASTE`, `SELECT_ALL`, `UNDO` and
`REDO` themselves, on the keys `default_editing_shortcuts(platform)` lists: the primary modifier
with C, X, V, A and Z, Shift+Z for Redo, and Ctrl+Y as well off Apple keyboards. A binding of
yours on one of those keys wins; bound to `Intent::DO_NOTHING`, the key does nothing at all.
