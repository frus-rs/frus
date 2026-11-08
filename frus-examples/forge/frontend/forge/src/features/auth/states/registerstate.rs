use frus::form::{Form, Rule};

/// The sign-up form.
#[derive(Default)]
pub(crate) struct RegisterState {
    pub name: String,
    pub email: String,
    pub password: String,
    pub confirm: String,
    /// Errors are shown only after the first attempt.
    pub submitted: bool,
}

impl RegisterState {
    pub(crate) fn set_name(&mut self, value: String) {
        self.name = value
    }

    pub(crate) fn set_email(&mut self, value: String) {
        self.email = value
    }

    pub(crate) fn set_password(&mut self, value: String) {
        self.password = value
    }

    pub(crate) fn set_confirm(&mut self, value: String) {
        self.confirm = value
    }

    pub(crate) fn form(&self) -> Form {
        Form::new()
            .field("name", &self.name, Rule::required("Your name is required"))
            .field(
                "email",
                &self.email,
                Rule::all([
                    Rule::required("Email is required"),
                    Rule::email("Enter a valid email address"),
                ]),
            )
            .field(
                "password",
                &self.password,
                Rule::all([
                    Rule::required("Password is required"),
                    Rule::min_len(8, "At least 8 characters"),
                ]),
            )
            .matches("confirm", &self.confirm, "password", "The passwords differ")
    }
}
