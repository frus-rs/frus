use frus::form::{Form, Rule};

/// The reset form.
#[derive(Default)]
pub(crate) struct ForgetPasswordState {
    pub email: String,
    pub submitted: bool,
    /// The link went out.
    pub sent: bool,
}

impl ForgetPasswordState {
    pub(crate) fn set_email(&mut self, value: String) {
        self.email = value;
        self.sent = false;
    }

    pub(crate) fn form(&self) -> Form {
        Form::new().field(
            "email",
            &self.email,
            Rule::all([
                Rule::required("Email is required"),
                Rule::email("Enter a valid email address"),
            ]),
        )
    }
}
