use frus::{
    Align, Column, FontWeight, Justify, KeyboardType, RichText, Row, State, StateContext,
    StatefulWidget, TextAlign, TextSpan, Widget,
};

use crate::{
    common::{
        colors::{grey, primary_color},
        routes::{HOME, LOGIN},
        shared::on,
        ui::{gap, primary_button, tap, Field},
    },
    features::auth::{
        states::registerstate::RegisterState,
        widgets::auth_form::{auth_field, auth_page, auth_title},
    },
};

/// Create an account: name, email, a password typed twice.
pub(crate) struct RegisterPage;

impl RegisterPage {
    pub(crate) fn new() -> Self {
        Self {}
    }
}

impl StatefulWidget for RegisterPage {
    type State = RegisterState;

    fn create_state(&self) -> Self::State {
        RegisterState::default()
    }
}

impl State for RegisterState {
    type Widget = RegisterPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let router = cx.router();
        let form = self.form();
        let error_of = |key| {
            if self.submitted {
                form.error(key).map(str::to_string)
            } else {
                None
            }
        };

        let create = {
            let router = router.clone();
            cx.callback(move |s: &mut RegisterState| {
                s.submitted = true;
                if s.form().is_valid() {
                    router.go(HOME);
                }
            })
        };
        let sign_in = {
            let router = router.clone();
            on(move || {
                if !router.pop() {
                    router.go(LOGIN);
                }
            })
        };

        let content = Column::new()
            .cross_axis_alignment(Align::Stretch)
            .child(auth_title(
                "Create your account",
                "Start building with your team on Forge.",
            ))
            .child(gap(32.0))
            .child(auth_field(
                "Full name",
                Field::new(&self.name, "Kokou", cx.handler(RegisterState::set_name))
                    .font(16.0)
                    .build(),
                error_of("name"),
            ))
            .child(gap(20.0))
            .child(auth_field(
                "Email",
                Field::new(
                    &self.email,
                    "kokou@forge.dev",
                    cx.handler(RegisterState::set_email),
                )
                .font(16.0)
                .keyboard(KeyboardType::Email)
                .build(),
                error_of("email"),
            ))
            .child(gap(20.0))
            .child(auth_field(
                "Password",
                Field::new(
                    &self.password,
                    "At least 8 characters",
                    cx.handler(RegisterState::set_password),
                )
                .font(16.0)
                .keyboard(KeyboardType::Password)
                .obscure(true)
                .build(),
                error_of("password"),
            ))
            .child(gap(20.0))
            .child(auth_field(
                "Confirm password",
                Field::new(
                    &self.confirm,
                    "Type it again",
                    cx.handler(RegisterState::set_confirm),
                )
                .font(16.0)
                .keyboard(KeyboardType::Password)
                .obscure(true)
                .on_submit(create.clone())
                .build(),
                error_of("confirm"),
            ))
            .child(gap(32.0))
            .child(primary_button("Create account", 48.0, 12.0, create))
            .child(gap(24.0))
            .child(
                Row::new().main_axis_alignment(Justify::Center).child(tap(
                    RichText::new(
                        TextSpan::new("Already have an account? ")
                            .color(grey())
                            .size(14.0)
                            .child(
                                TextSpan::new("Sign in")
                                    .color(primary_color())
                                    .weight(FontWeight::Medium),
                            ),
                    )
                    .align(TextAlign::Center),
                    sign_in,
                )),
            );

        auth_page(Box::new(content), 56.0)
    }
}
