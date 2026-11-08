use frus::Color;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    Online,
    Away,
    Offline,
}

impl Presence {
    pub fn label(self) -> &'static str {
        match self {
            Presence::Online => "Online",
            Presence::Away => "Away",
            Presence::Offline => "Offline",
        }
    }

    /// The dot before the label.
    pub fn color(self) -> Color {
        match self {
            Presence::Online => Color::hex("#00C950"),
            Presence::Away => Color::hex("#F0B100"),
            Presence::Offline => Color::hex("#9F9FA9"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceRole {
    Owner,
    Admin,
    Member,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    pub id: u32,
    /// As lists show it: "Sarah M.".
    pub name: String,
    /// As the workspace shows it: "Sarah Moreau".
    pub full_name: String,
    /// The two letters on the board's cards.
    pub initials: String,
    /// The job shown under the name: "Designer".
    pub title: String,
    pub role: WorkspaceRole,
    pub presence: Presence,
    /// The signed-in user: their avatar is green.
    pub is_you: bool,
}

impl Member {
    /// The single letter of the round avatars.
    pub fn letter(&self) -> String {
        self.name
            .chars()
            .next()
            .map(|c| c.to_uppercase().collect())
            .unwrap_or_default()
    }

    /// The first word of the name: "Sarah".
    pub fn first_name(&self) -> &str {
        self.name.split_whitespace().next().unwrap_or(&self.name)
    }
}
