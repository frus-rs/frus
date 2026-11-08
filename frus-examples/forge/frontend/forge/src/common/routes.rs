//! The application's routes.

/// The dashboard.
pub(crate) const HOME: &str = "/";
/// Sign in.
pub(crate) const LOGIN: &str = "/login";
/// Create an account.
pub(crate) const REGISTER: &str = "/register";
/// Reset a forgotten password.
pub(crate) const FORGET_PASSWORD: &str = "/forget-password";

/// Every project.
pub(crate) const PROJECTS: &str = "/projects";
/// Create a project.
pub(crate) const NEW_PROJECT: &str = "/projects/new";
/// The notifications.
pub(crate) const NOTIFICATIONS: &str = "/notifications";
/// Search across Forge.
pub(crate) const SEARCH: &str = "/search";
/// Search results; the words are the `q` query parameter.
pub(crate) const SEARCH_RESULTS: &str = "/search/results";
/// The filters of a search.
pub(crate) const SEARCH_FILTERS: &str = "/search/filters";

/// The settings.
pub(crate) const SETTINGS: &str = "/settings";
/// The signed-in member's profile.
pub(crate) const PROFILE: &str = "/profile";
/// Edit the profile.
pub(crate) const PERSONAL_INFO: &str = "/profile/personal";
/// Password, two-factor authentication and sessions.
pub(crate) const SECURITY: &str = "/profile/security";
/// The workspace.
pub(crate) const WORKSPACE: &str = "/workspace";
/// The workspace's members.
pub(crate) const WORKSPACE_MEMBERS: &str = "/workspace/members";
/// Invite people into the workspace.
pub(crate) const INVITE: &str = "/workspace/invite";
/// The invitations sent.
pub(crate) const INVITATIONS: &str = "/workspace/invitations";

/// A project's board: `/projects/1`.
pub(crate) fn project_board(id: u32) -> String {
    format!("/projects/{id}")
}

/// A project's overview.
pub(crate) fn project_overview(id: u32) -> String {
    format!("/projects/{id}/overview")
}

/// A project's tasks, as a list.
pub(crate) fn project_tasks(id: u32) -> String {
    format!("/projects/{id}/tasks")
}

/// A project's members.
pub(crate) fn project_members(id: u32) -> String {
    format!("/projects/{id}/members")
}

/// A project's activity.
pub(crate) fn project_activity(id: u32) -> String {
    format!("/projects/{id}/activity")
}

/// Create a task in a project.
pub(crate) fn new_task(project: u32) -> String {
    format!("/projects/{project}/tasks/new")
}

/// A task's page.
pub(crate) fn task(id: u32) -> String {
    format!("/tasks/{id}")
}

/// Edit a task.
pub(crate) fn edit_task(id: u32) -> String {
    format!("/tasks/{id}/edit")
}

/// Choose who a task is assigned to.
pub(crate) fn assign_task(id: u32) -> String {
    format!("/tasks/{id}/assign")
}

/// A task's due date, labels and estimate.
pub(crate) fn task_metadata(id: u32) -> String {
    format!("/tasks/{id}/details")
}

/// Search results for `query`.
pub(crate) fn search_results(query: &str) -> String {
    format!("{SEARCH_RESULTS}?q={}", encode(query))
}

/// Everything of one type: `kind` is 1 projects, 2 tasks, 3 members.
pub(crate) fn browse(kind: usize) -> String {
    format!("{SEARCH_RESULTS}?q=&type={kind}")
}

/// Percent-encodes what a query parameter cannot carry as is.
fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}
