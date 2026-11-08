/// The new project form.
pub(crate) struct NewProjectState {
    pub name: String,
    pub description: String,
    /// Index into the project colours.
    pub color: usize,
    /// 0 private, 1 the whole workspace.
    pub visibility: usize,
    /// The address being typed in "Invite members".
    pub email: String,
    /// Who is invited; the signed-in member is there from the start.
    pub invitees: Vec<String>,
    /// The form was sent once: its errors show from then on.
    pub submitted: bool,
}

impl Default for NewProjectState {
    fn default() -> Self {
        NewProjectState {
            name: String::new(),
            description: String::new(),
            color: 0,
            visibility: 0,
            email: String::new(),
            invitees: vec!["Kokou".to_string()],
            submitted: false,
        }
    }
}

impl NewProjectState {
    pub(crate) fn set_name(&mut self, name: String) {
        self.name = name;
    }

    pub(crate) fn set_description(&mut self, description: String) {
        self.description = description;
    }

    pub(crate) fn set_email(&mut self, email: String) {
        self.email = email;
    }

    /// Adds what was typed in "Invite members" as a chip.
    pub(crate) fn add_invitee(&mut self) {
        let email = self.email.trim();
        if email.contains('@') && !self.invitees.iter().any(|i| i == email) {
            self.invitees.push(email.to_string());
            self.email.clear();
        }
    }
}
