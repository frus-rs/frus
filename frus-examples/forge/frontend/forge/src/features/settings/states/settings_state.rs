/// The settings page.
pub(crate) struct SettingsState {
    pub notifications: bool,
}

/// The personal information form.
pub(crate) struct PersonalInfoState {
    pub name: String,
    pub email: String,
    pub job: String,
    pub saved: bool,
}

impl PersonalInfoState {
    pub(crate) fn set_name(&mut self, name: String) {
        self.name = name;
    }

    pub(crate) fn set_email(&mut self, email: String) {
        self.email = email;
    }

    pub(crate) fn set_job(&mut self, job: String) {
        self.job = job;
    }
}

/// The security page.
#[derive(Default)]
pub(crate) struct SecurityState {
    pub two_factor: bool,
    /// The other session was signed out.
    pub laptop_signed_out: bool,
    pub everywhere: bool,
}

/// The workspace's member list.
#[derive(Default)]
pub(crate) struct WorkspaceMembersState {
    pub query: String,
    /// 0 all, 1 admins, 2 members.
    pub role: usize,
}

/// The invite form.
pub(crate) struct InviteState {
    pub email: String,
    pub emails: Vec<String>,
    pub another: String,
    pub admin: bool,
    pub message: String,
}

impl Default for InviteState {
    fn default() -> Self {
        InviteState {
            email: "lucy@northstar.co".into(),
            emails: vec!["lucy@northstar.co".into()],
            another: String::new(),
            admin: false,
            message: "Welcome to Forge Studio — let’s build together.".into(),
        }
    }
}

impl InviteState {
    /// Takes every address typed in `text`, split by commas, as a chip.
    pub(crate) fn take_emails(&mut self, text: &str) {
        for email in text.split(',').map(str::trim) {
            if email.contains('@') && !self.emails.iter().any(|e| e == email) {
                self.emails.push(email.to_string());
            }
        }
    }
}
