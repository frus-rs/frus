use frus::*;

use crate::common::auth::Auth;
use crate::common::routes::*;
use crate::features::auth::pages::forgetpassword::ForgetPasswordPage;
use crate::features::auth::pages::loginpage::LoginPage;
use crate::features::auth::pages::register::RegisterPage;
use crate::features::home::pages::homepage::HomePage;
use crate::features::notifications::pages::notifications::NotificationsPage;
use crate::features::projects::pages::{
    activity::ProjectActivityPage, board::BoardPage, members::ProjectMembersPage,
    new_project::NewProjectPage, overview::OverviewPage, projects::ProjectsPage,
    tasks::ProjectTasksPage,
};
use crate::features::search::pages::{
    filters::FiltersPage, results::SearchResultsPage, search::SearchPage,
};
use crate::features::settings::pages::{
    invitations::InvitationsPage, invite::InvitePage, personal_info::PersonalInfoPage,
    profile::ProfilePage, security::SecurityPage, settings::SettingsPage, workspace::WorkspacePage,
    workspace_members::WorkspaceMembersPage,
};
use crate::features::tasks::pages::{
    assign::AssignTaskPage, edit_task::EditTaskPage, metadata::TaskMetadataPage,
    new_task::NewTaskPage, task_detail::TaskDetailPage,
};

/// The number in a route's `:id`, or 0 when it is not one (the page then says so).
fn id(state: &GoRouterState) -> u32 {
    state
        .param("id")
        .and_then(|id| id.parse().ok())
        .unwrap_or(0)
}

pub(crate) fn router(auth: Auth) -> GoRouter {
    let (for_home, for_login, for_settings, for_profile) =
        (auth.clone(), auth.clone(), auth.clone(), auth.clone());

    GoRouter::new(vec![
        GoRoute::new(LOGIN, move |_, _| {
            LoginPage::new(for_login.clone()).into_widget()
        }),
        GoRoute::new(REGISTER, move |_, _| RegisterPage::new().into_widget()),
        GoRoute::new(FORGET_PASSWORD, move |_, _| {
            ForgetPasswordPage::new().into_widget()
        }),
        GoRoute::new(HOME, move |_, _| {
            HomePage::new(for_home.clone()).into_widget()
        }),
        // Projects: the fixed paths before the ones that take an id.
        GoRoute::new(PROJECTS, |_, _| ProjectsPage.into_widget()),
        GoRoute::new(NEW_PROJECT, |_, _| NewProjectPage.into_widget()),
        GoRoute::new("/projects/:id", |_, s| {
            BoardPage { project: id(s) }.into_widget()
        }),
        GoRoute::new("/projects/:id/overview", |_, s| {
            OverviewPage { project: id(s) }.into_widget()
        }),
        GoRoute::new("/projects/:id/tasks", |_, s| {
            ProjectTasksPage { project: id(s) }.into_widget()
        }),
        GoRoute::new("/projects/:id/tasks/new", |_, s| {
            NewTaskPage { project: id(s) }.into_widget()
        }),
        GoRoute::new("/projects/:id/members", |_, s| {
            ProjectMembersPage { project: id(s) }.into_widget()
        }),
        GoRoute::new("/projects/:id/activity", |_, s| {
            ProjectActivityPage { project: id(s) }.into_widget()
        }),
        // Tasks.
        GoRoute::new("/tasks/:id", |_, s| {
            TaskDetailPage { task: id(s) }.into_widget()
        }),
        GoRoute::new("/tasks/:id/edit", |_, s| {
            EditTaskPage { task: id(s) }.into_widget()
        }),
        GoRoute::new("/tasks/:id/assign", |_, s| {
            AssignTaskPage { task: id(s) }.into_widget()
        }),
        GoRoute::new("/tasks/:id/details", |_, s| {
            TaskMetadataPage { task: id(s) }.into_widget()
        }),
        // Notifications and search.
        GoRoute::new(NOTIFICATIONS, |_, _| NotificationsPage.into_widget()),
        GoRoute::new(SEARCH, |_, _| SearchPage.into_widget()),
        GoRoute::new(SEARCH_RESULTS, |_, s| {
            SearchResultsPage {
                query: s.query("q").unwrap_or_default().to_string(),
                kind: s.query("type").and_then(|k| k.parse().ok()).unwrap_or(0),
            }
            .into_widget()
        }),
        GoRoute::new(SEARCH_FILTERS, |_, _| FiltersPage.into_widget()),
        // Settings, the account and the workspace.
        GoRoute::new(SETTINGS, move |_, _| {
            SettingsPage {
                auth: for_settings.clone(),
            }
            .into_widget()
        }),
        GoRoute::new(PROFILE, move |_, _| {
            ProfilePage {
                auth: for_profile.clone(),
            }
            .into_widget()
        }),
        GoRoute::new(PERSONAL_INFO, |_, _| PersonalInfoPage.into_widget()),
        GoRoute::new(SECURITY, |_, _| SecurityPage.into_widget()),
        GoRoute::new(WORKSPACE, |_, _| WorkspacePage.into_widget()),
        GoRoute::new(WORKSPACE_MEMBERS, |_, _| WorkspaceMembersPage.into_widget()),
        GoRoute::new(INVITE, |_, _| InvitePage.into_widget()),
        GoRoute::new(INVITATIONS, |_, _| InvitationsPage.into_widget()),
    ])
    .initial_location(LOGIN)
}
