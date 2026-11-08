//! Tests for `forge::models::store`, the in-memory data the pages share.

use forge::models::{
    project::ProjectStatus,
    store,
    task::{Priority, Task, TaskStatus},
};

#[test]
fn the_sample_matches_the_design() {
    store::read(|s| {
        assert_eq!(s.projects.len(), 3);
        assert_eq!(s.you().name, "Kokou");
        let frus = s.project(1).expect("Frus Framework");
        assert_eq!(frus.progress, 72);
        assert_eq!(frus.status, ProjectStatus::Active);
        // The board's first column holds the three tasks the design shows.
        let todo = s
            .tasks_of(1)
            .into_iter()
            .filter(|t| t.status == TaskStatus::Todo)
            .count();
        assert_eq!(todo, 3);
        assert_eq!(s.comments_of(126).len(), 2);
    });
}

#[test]
fn a_new_task_takes_the_next_number() {
    let id = store::write(|s| {
        let id = s.next_task_id();
        s.tasks.push(Task {
            id,
            project_id: 1,
            title: "Write the guide".into(),
            description: String::new(),
            status: TaskStatus::Todo,
            priority: Priority::None,
            assignee: None,
            due_day: None,
            labels: vec![],
            estimate: None,
        });
        id
    });
    store::read(|s| {
        assert!(s.tasks.iter().all(|t| t.id <= id));
        assert_eq!(
            s.task(id).map(|t| t.title.as_str()),
            Some("Write the guide")
        );
    });
}

#[test]
fn every_project_member_exists() {
    store::read(|s| {
        for project in &s.projects {
            assert_eq!(
                s.members_of(project).len(),
                project.members.len(),
                "{}",
                project.name
            );
        }
    });
}
