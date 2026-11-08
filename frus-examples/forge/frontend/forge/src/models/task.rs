#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskStatus {
    Todo,
    InProgress,
    Review,
    Done,
}

impl TaskStatus {
    /// The columns of the board, in order.
    pub const ALL: [TaskStatus; 4] = [
        TaskStatus::Todo,
        TaskStatus::InProgress,
        TaskStatus::Review,
        TaskStatus::Done,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TaskStatus::Todo => "Todo",
            TaskStatus::InProgress => "In progress",
            TaskStatus::Review => "Review",
            TaskStatus::Done => "Done",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Priority {
    None,
    Low,
    Medium,
    High,
}

impl Priority {
    pub const ALL: [Priority; 4] = [
        Priority::None,
        Priority::Low,
        Priority::Medium,
        Priority::High,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Priority::None => "None",
            Priority::Low => "Low",
            Priority::Medium => "Medium",
            Priority::High => "High",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    /// Shown as `#126`, and as `FRU-126` on the task's page.
    pub id: u32,
    pub project_id: u32,
    pub title: String,
    pub description: String,
    pub status: TaskStatus,
    pub priority: Priority,
    pub assignee: Option<u32>,
    /// Day of September 2026, the month the sample data lives in.
    pub due_day: Option<u32>,
    pub labels: Vec<String>,
    pub estimate: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Comment {
    pub task_id: u32,
    pub author: u32,
    pub ago: String,
    pub text: String,
}
