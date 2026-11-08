/// What an activity entry is about, for the activity page's filter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivityKind {
    Task,
    Comment,
    Member,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Activity {
    pub project_id: u32,
    pub member: u32,
    /// "moved Router to Review".
    pub action: String,
    /// The quoted subject on the dashboard: `"Router"`.
    pub subject: String,
    /// The rest after the subject on the dashboard: "to Review".
    pub tail: String,
    /// "12 min ago".
    pub ago: String,
    /// "10:18", for the activity page's timeline.
    pub time: String,
    /// "Today" or "Yesterday".
    pub day: &'static str,
    pub kind: ActivityKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvitationStatus {
    Pending,
    Accepted,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Invitation {
    pub email: String,
    /// "Invited by Kokou · Apr 18" or "Accepted Apr 12".
    pub detail: String,
    pub status: InvitationStatus,
}
