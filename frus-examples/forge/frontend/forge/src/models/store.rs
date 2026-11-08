//! The application's data, held in memory.
//!
//! Forge's backend is not wired in yet: the pages read and change this store, which starts
//! with the sample data of the design. Every page reads it again when it is built, so a
//! project or a task created on one page shows on the next.

use std::cell::RefCell;

use super::{
    activity::{Activity, ActivityKind, Invitation, InvitationStatus},
    member::{Member, Presence, WorkspaceRole},
    project::{Project, ProjectColor, ProjectStatus},
    task::{Comment, Priority, Task, TaskStatus},
};

pub struct Store {
    pub projects: Vec<Project>,
    pub tasks: Vec<Task>,
    pub members: Vec<Member>,
    pub comments: Vec<Comment>,
    pub activity: Vec<Activity>,
    pub invitations: Vec<Invitation>,
    pub recent_searches: Vec<String>,
    /// The filters applied to search results, once applied.
    pub search_filters: Option<SearchFilters>,
}

/// What the filters page narrowed the search to.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchFilters {
    /// Todo, In progress, Review, Done; none checked means any.
    pub statuses: [bool; 4],
    pub labels: Vec<String>,
}

thread_local! {
    static STORE: RefCell<Store> = RefCell::new(Store::sample());
}

/// Reads the store.
pub fn read<R>(read: impl FnOnce(&Store) -> R) -> R {
    STORE.with(|store| read(&store.borrow()))
}

/// Changes the store.
pub fn write<R>(change: impl FnOnce(&mut Store) -> R) -> R {
    STORE.with(|store| change(&mut store.borrow_mut()))
}

impl Store {
    pub fn project(&self, id: u32) -> Option<&Project> {
        self.projects.iter().find(|p| p.id == id)
    }

    pub fn task(&self, id: u32) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    pub fn member(&self, id: u32) -> Option<&Member> {
        self.members.iter().find(|m| m.id == id)
    }

    pub fn you(&self) -> &Member {
        self.members
            .iter()
            .find(|m| m.is_you)
            .expect("the signed-in member")
    }

    pub fn tasks_of(&self, project: u32) -> Vec<Task> {
        self.tasks
            .iter()
            .filter(|t| t.project_id == project)
            .cloned()
            .collect()
    }

    pub fn members_of(&self, project: &Project) -> Vec<Member> {
        project
            .members
            .iter()
            .filter_map(|id| self.member(*id))
            .cloned()
            .collect()
    }

    pub fn comments_of(&self, task: u32) -> Vec<Comment> {
        self.comments
            .iter()
            .filter(|c| c.task_id == task)
            .cloned()
            .collect()
    }

    pub fn activity_of(&self, project: u32) -> Vec<Activity> {
        self.activity
            .iter()
            .filter(|a| a.project_id == project)
            .cloned()
            .collect()
    }

    /// The next free task number.
    pub fn next_task_id(&self) -> u32 {
        self.tasks.iter().map(|t| t.id).max().unwrap_or(100) + 1
    }

    pub fn next_project_id(&self) -> u32 {
        self.projects.iter().map(|p| p.id).max().unwrap_or(0) + 1
    }

    /// The design's sample data.
    fn sample() -> Self {
        let member = |id: u32,
                      name: &str,
                      full_name: &str,
                      initials: &str,
                      title: &str,
                      role: WorkspaceRole,
                      presence: Presence| Member {
            id,
            name: name.into(),
            full_name: full_name.into(),
            initials: initials.into(),
            title: title.into(),
            role,
            presence,
            is_you: id == 1,
        };
        let members = vec![
            member(
                1,
                "Kokou",
                "Kokou",
                "KO",
                "Owner",
                WorkspaceRole::Owner,
                Presence::Online,
            ),
            member(
                2,
                "Sarah M.",
                "Sarah Moreau",
                "SA",
                "Designer",
                WorkspaceRole::Admin,
                Presence::Online,
            ),
            member(
                3,
                "David K.",
                "David Kim",
                "DK",
                "Developer",
                WorkspaceRole::Member,
                Presence::Away,
            ),
            member(
                5,
                "Amélie P.",
                "Amélie Petit",
                "AP",
                "Product manager",
                WorkspaceRole::Member,
                Presence::Offline,
            ),
            member(
                4,
                "Amina R.",
                "Amina Rahmani",
                "AR",
                "Developer",
                WorkspaceRole::Member,
                Presence::Offline,
            ),
            member(
                6,
                "Moussa D.",
                "Moussa Diallo",
                "MD",
                "Developer",
                WorkspaceRole::Member,
                Presence::Online,
            ),
            member(
                7,
                "Lucas B.",
                "Lucas Bernard",
                "LB",
                "Developer",
                WorkspaceRole::Member,
                Presence::Away,
            ),
            member(
                8,
                "Inès L.",
                "Inès Laurent",
                "IL",
                "Designer",
                WorkspaceRole::Member,
                Presence::Offline,
            ),
        ];

        let projects = vec![
            Project {
                id: 1,
                name: "Frus Framework".into(),
                description: "A collaborative workspace for building the next generation of Forge."
                    .into(),
                color: ProjectColor::Green,
                status: ProjectStatus::Active,
                task_count: 12,
                member_count: 4,
                members: vec![1, 2, 3, 4],
                progress: 72,
                in_progress: 3,
                due_soon: 2,
            },
            Project {
                id: 2,
                name: "Forge Mobile".into(),
                description: "The Forge application for phones, built with Frus.".into(),
                color: ProjectColor::Purple,
                status: ProjectStatus::Active,
                task_count: 8,
                member_count: 3,
                members: vec![1, 4],
                progress: 38,
                in_progress: 2,
                due_soon: 1,
            },
            Project {
                id: 3,
                name: "Rust Docs".into(),
                description: "Guides and references for the Rust side of Forge.".into(),
                color: ProjectColor::Orange,
                status: ProjectStatus::Completed,
                task_count: 24,
                member_count: 6,
                members: vec![2, 3, 6],
                progress: 100,
                in_progress: 0,
                due_soon: 0,
            },
        ];

        let task = |id: u32,
                    project_id: u32,
                    title: &str,
                    status: TaskStatus,
                    priority: Priority,
                    assignee: u32,
                    labels: &[&str]| Task {
            id,
            project_id,
            title: title.into(),
            description: String::new(),
            status,
            priority,
            assignee: Some(assignee),
            due_day: None,
            labels: labels.iter().map(|l| l.to_string()).collect(),
            estimate: None,
        };
        let mut tasks = vec![
            task(
                126,
                1,
                "Implement Frus navigation",
                TaskStatus::InProgress,
                Priority::High,
                1,
                &["frontend", "routing", "mvp"],
            ),
            task(
                128,
                1,
                "Connect Axum API",
                TaskStatus::InProgress,
                Priority::None,
                3,
                &["backend"],
            ),
            task(
                124,
                1,
                "Implement login form",
                TaskStatus::Todo,
                Priority::Low,
                1,
                &["frontend"],
            ),
            task(
                125,
                1,
                "Add project creation",
                TaskStatus::Todo,
                Priority::Medium,
                2,
                &["frontend"],
            ),
            task(
                127,
                1,
                "Design empty states",
                TaskStatus::Todo,
                Priority::Low,
                3,
                &["design"],
            ),
            task(
                129,
                2,
                "Add mobile back stack",
                TaskStatus::Todo,
                Priority::Medium,
                2,
                &["mobile"],
            ),
            task(
                131,
                2,
                "Review protected routes",
                TaskStatus::Review,
                Priority::Medium,
                3,
                &["routing"],
            ),
            task(
                132,
                2,
                "Offline task cache",
                TaskStatus::InProgress,
                Priority::Medium,
                4,
                &["mobile"],
            ),
            task(
                133,
                3,
                "Router guide",
                TaskStatus::Done,
                Priority::Low,
                2,
                &["docs"],
            ),
        ];
        tasks[0].description = "Implement nested navigation with protected routes, project context, and a clear mobile back stack.".into();
        tasks[0].due_day = Some(25);
        tasks[5].description =
            "Keep the navigation history when the phone's back button is used.".into();
        tasks[6].description =
            "Check that every protected route redirects to sign in, and back after navigation."
                .into();

        let comments = vec![
            Comment {
                task_id: 126,
                author: 2,
                ago: "12 min ago".into(),
                text: "We should support nested routes and preserve the project context.".into(),
            },
            Comment {
                task_id: 126,
                author: 3,
                ago: "4 min ago".into(),
                text: "I’ll verify the protected route behavior on iOS.".into(),
            },
        ];

        let activity = |member: u32,
                        verb: &str,
                        subject: &str,
                        tail: &str,
                        ago: &str,
                        time: &str,
                        day: &'static str,
                        kind: ActivityKind| Activity {
            project_id: 1,
            member,
            action: verb.into(),
            subject: subject.into(),
            tail: tail.into(),
            ago: ago.into(),
            time: time.into(),
            day,
            kind,
        };
        let activity = vec![
            activity(
                2,
                "moved",
                "Router",
                "to Review",
                "12 min ago",
                "10:18",
                "Today",
                ActivityKind::Task,
            ),
            activity(
                3,
                "commented on",
                "WebSocket",
                "",
                "28 min ago",
                "09:56",
                "Today",
                ActivityKind::Comment,
            ),
            activity(
                1,
                "assigned",
                "API error states",
                "",
                "1 h ago",
                "10:42",
                "Today",
                ActivityKind::Task,
            ),
            activity(
                4,
                "joined the project",
                "",
                "",
                "Yesterday",
                "16:24",
                "Yesterday",
                ActivityKind::Member,
            ),
            activity(
                1,
                "created task",
                "Implement login form",
                "",
                "Yesterday",
                "14:08",
                "Yesterday",
                ActivityKind::Task,
            ),
        ];

        let invitations = vec![
            Invitation {
                email: "lucy@northstar.co".into(),
                detail: "Invited by Kokou · Apr 18".into(),
                status: InvitationStatus::Pending,
            },
            Invitation {
                email: "marco@atelier.io".into(),
                detail: "Invited by Sarah Moreau · Apr 16".into(),
                status: InvitationStatus::Pending,
            },
            Invitation {
                email: "ines@studio.fr".into(),
                detail: "Accepted Apr 12".into(),
                status: InvitationStatus::Accepted,
            },
        ];

        Store {
            projects,
            tasks,
            members,
            comments,
            activity,
            invitations,
            recent_searches: vec![
                "Frus Framework".into(),
                "Marketing website".into(),
                "Onboarding flow".into(),
            ],
            search_filters: None,
        }
    }
}
