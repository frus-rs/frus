use frus::{
    Align, Column, FontWeight, Justify, KeyboardType, RichText, Row, State, StateContext,
    StatefulWidget, TextAlign, TextSpan, Widget,
};

use crate::{
    common::{
        auth::Auth,
        colors::{black, grey, primary_color},
        icons::Glyph,
        routes::{FORGET_PASSWORD, HOME, REGISTER},
        shared::on,
        ui::{gap, icon_button, outline_button, primary_button, tap, text, Field},
    },
    features::auth::{
        states::loginstate::LoginState,
        widgets::auth_form::{auth_field, auth_page, auth_title, or_divider},
    },
};

pub(crate) struct LoginPage {
    auth: Auth,
}

impl LoginPage {
    pub(crate) fn new(auth: Auth) -> Self {
        Self { auth }
    }
}

impl StatefulWidget for LoginPage {
    type State = LoginState;

    fn create_state(&self) -> LoginState {
        LoginState {
            email: String::new(),
            password: String::new(),
            visibility: false,
            submitted: false,
        }
    }
}

impl State for LoginState {
    type Widget = LoginPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let router = cx.router();
        let auth = cx.widget().auth.clone();

        let form = self.form();
        let error_of = |key| {
            if self.submitted {
                form.error(key).map(str::to_string)
            } else {
                None
            }
        };

        let go_to = |path: &'static str| {
            let r = router.clone();
            on(move || r.push(path))
        };

        // Signs in only once the form is valid; its errors show from the first attempt.
        let sign_in = {
            let router = router.clone();
            cx.callback(move |s: &mut LoginState| {
                s.submitted = true;
                if s.form().is_valid() {
                    auth.sign_in(s.email.clone());
                    router.go(HOME);
                }
            })
        };

        let email = Field::new(
            &self.email,
            "kokou@forge.dev",
            cx.handler(LoginState::set_email),
        )
        .font(16.0)
        .keyboard(KeyboardType::Email)
        .build();

        let eye = icon_button(
            if self.visibility {
                Glyph::EyeOff
            } else {
                Glyph::Eye
            },
            16.0,
            24.0,
            grey(),
            cx.callback(LoginState::toggle_visibility),
        );
        let password = Field::new(
            &self.password,
            "••••••••••••",
            cx.handler(LoginState::set_password),
        )
        .font(16.0)
        .keyboard(KeyboardType::Password)
        .obscure(!self.visibility)
        .trailing(eye)
        .on_submit(sign_in.clone())
        .build();

        let create_account = tap(
            RichText::new(
                TextSpan::new("Don’t have an account? ")
                    .color(grey())
                    .size(14.0)
                    .child(
                        TextSpan::new("Create account")
                            .color(primary_color())
                            .weight(FontWeight::Medium),
                    ),
            )
            .align(TextAlign::Center),
            go_to(REGISTER),
        );

        let content = Column::new()
            .cross_axis_alignment(Align::Stretch)
            .child(auth_title(
                "Welcome back",
                "Sign in to continue collaborating with your team.",
            ))
            .child(gap(32.0))
            .child(auth_field("Email", email, error_of("email")))
            .child(gap(24.0))
            .child(auth_field("Password", password, error_of("password")))
            .child(gap(8.0))
            .child(Row::new().main_axis_alignment(Justify::End).child(tap(
                text(
                    "Forgot password?",
                    14.0,
                    20.0,
                    primary_color(),
                    FontWeight::Medium,
                ),
                go_to(FORGET_PASSWORD),
            )))
            .child(gap(24.0))
            .child(primary_button("Sign in", 48.0, 12.0, sign_in))
            .child(gap(32.0))
            .child(or_divider())
            .child(gap(24.0))
            .child(outline_button(
                Box::new(text(
                    "Continue with GitHub",
                    14.0,
                    20.0,
                    black(),
                    FontWeight::Medium,
                )),
                48.0,
                12.0,
                on(|| {}),
            ))
            .child(gap(24.0))
            .child(
                Row::new()
                    .main_axis_alignment(Justify::Center)
                    .child(create_account),
            );

        auth_page(Box::new(content), 80.0)
    }
}
