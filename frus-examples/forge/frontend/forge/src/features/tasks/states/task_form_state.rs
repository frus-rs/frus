use crate::models::task::{Priority, Task, TaskStatus};

/// Which inline picker of the new task form is open.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Picker {
    #[default]
    None,
    Assignee,
    Due,
    Label,
}

/// A task being written: the new task form and the edit form.
pub(crate) struct TaskFormState {
    pub title: String,
    pub description: String,
    /// Index into Todo, In progress, Review.
    pub status: usize,
    pub priority: Priority,
    pub assignee: Option<u32>,
    pub due_day: Option<u32>,
    /// The month the calendar shows.
    pub month: (i32, u32),
    pub labels: Vec<String>,
    pub label_draft: String,
    pub picker: Picker,
    pub submitted: bool,
}

/// The statuses a form offers.
pub(crate) const FORM_STATUSES: [TaskStatus; 3] =
    [TaskStatus::Todo, TaskStatus::InProgress, TaskStatus::Review];

impl TaskFormState {
    /// An empty form, assigned to `assignee`.
    pub(crate) fn blank(assignee: Option<u32>) -> Self {
        TaskFormState {
            title: String::new(),
            description: String::new(),
            status: 0,
            priority: Priority::None,
            assignee,
            due_day: None,
            month: (2025, 9),
            labels: vec!["frontend".into(), "mobile".into()],
            label_draft: String::new(),
            picker: Picker::None,
            submitted: false,
        }
    }

    /// The form filled with `task`.
    pub(crate) fn of(task: &Task) -> Self {
        TaskFormState {
            title: task.title.clone(),
            description: task.description.clone(),
            status: FORM_STATUSES
                .iter()
                .position(|s| *s == task.status)
                .unwrap_or(0),
            priority: task.priority,
            assignee: task.assignee,
            due_day: task.due_day,
            month: (2025, 9),
            labels: task.labels.clone(),
            label_draft: String::new(),
            picker: Picker::None,
            submitted: false,
        }
    }

    pub(crate) fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub(crate) fn set_description(&mut self, description: String) {
        self.description = description;
    }

    pub(crate) fn set_label_draft(&mut self, label: String) {
        self.label_draft = label;
    }

    /// Adds the label being typed, once.
    pub(crate) fn add_label(&mut self) {
        let label = self.label_draft.trim().to_lowercase();
        if !label.is_empty() && !self.labels.contains(&label) {
            self.labels.push(label);
        }
        self.label_draft.clear();
    }

    /// Opens `picker`, or closes it when it is the one open.
    pub(crate) fn toggle(&mut self, picker: Picker) {
        self.picker = if self.picker == picker {
            Picker::None
        } else {
            picker
        };
    }
}
