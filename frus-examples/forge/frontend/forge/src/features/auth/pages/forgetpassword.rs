use frus::{
    Align, Column, Justify, KeyboardType, Row, State, StateContext, StatefulWidget, Widget,
};

use crate::{
    common::{
        colors::primary_color,
        icons::{icon, Glyph},
        routes::LOGIN,
        shared::on,
        ui::{back_link, banner_large, gap, primary_button, Field},
    },
    features::auth::{
        states::forgetpassword::ForgetPasswordState,
        widgets::auth_form::{auth_field, auth_page, auth_title},
    },
};

/// Ask for a link to choose a new password.
pub(crate) struct ForgetPasswordPage;

impl ForgetPasswordPage {
    pub(crate) fn new() -> Self {
        Self {}
    }
}

impl StatefulWidget for ForgetPasswordPage {
    type State = ForgetPasswordState;

    fn create_state(&self) -> Self::State {
        ForgetPasswordState::default()
    }
}

impl State for ForgetPasswordState {
    type Widget = ForgetPasswordPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let router = cx.router();
        let error = if self.submitted {
            self.form().error("email").map(str::to_string)
        } else {
            None
        };

        let send = cx.callback(|s: &mut ForgetPasswordState| {
            s.submitted = true;
            s.sent = s.form().is_valid();
        });
        let to_login = {
            let router = router.clone();
            on(move || {
                if !router.pop() {
                    router.go(LOGIN);
                }
            })
        };

        let mut content = Column::new()
            .cross_axis_alignment(Align::Stretch)
            .child(auth_title(
                "Reset your password",
                "Enter your email and we’ll send you a link to choose a new one.",
            ))
            .child(gap(32.0))
            .child(auth_field(
                "Email",
                Field::new(
                    &self.email,
                    "kokou@forge.dev",
                    cx.handler(ForgetPasswordState::set_email),
                )
                .font(16.0)
                .keyboard(KeyboardType::Email)
                .on_submit(send.clone())
                .build(),
                error,
            ))
            .child(gap(24.0))
            .child(primary_button("Send reset link", 48.0, 12.0, send));
        if self.sent {
            content = content.child(gap(24.0)).child(banner_large(
                Box::new(icon(Glyph::CircleCheck, 16.0, primary_color())),
                &format!("Check {} for the link.", self.email.trim()),
                12.0,
                12.0,
            ));
        }
        content = content.child(gap(24.0)).child(
            Row::new()
                .main_axis_alignment(Justify::Center)
                .child(back_link("Back to sign in", 16.0, 6.0, to_login)),
        );

        auth_page(Box::new(content), 80.0)
    }
}
