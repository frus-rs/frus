use frus::Color;

/// Where a project stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectStatus {
    Active,
    Completed,
}

impl ProjectStatus {
    pub fn label(self) -> &'static str {
        match self {
            ProjectStatus::Active => "Active",
            ProjectStatus::Completed => "Completed",
        }
    }
}

/// The accent a project is drawn in: its card's edge, its icon, its progress bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectColor {
    Green,
    Purple,
    Orange,
    Blue,
    Pink,
}

impl ProjectColor {
    pub const ALL: [ProjectColor; 5] = [
        ProjectColor::Green,
        ProjectColor::Purple,
        ProjectColor::Orange,
        ProjectColor::Blue,
        ProjectColor::Pink,
    ];

    /// The accent itself.
    pub fn color(self) -> Color {
        match self {
            ProjectColor::Green => Color::hex("#00C950"),
            ProjectColor::Purple => Color::hex("#8E51FF"),
            ProjectColor::Orange => Color::hex("#FE9A00"),
            ProjectColor::Blue => Color::hex("#2B7FFF"),
            ProjectColor::Pink => Color::hex("#F6339A"),
        }
    }

    /// The pale square behind the project's icon.
    pub fn tint(self) -> Color {
        match self {
            ProjectColor::Green => Color::hex("#F0FDF4"),
            ProjectColor::Purple => Color::hex("#F5F3FF"),
            ProjectColor::Orange => Color::hex("#FFF7ED"),
            ProjectColor::Blue => Color::hex("#EFF6FF"),
            ProjectColor::Pink => Color::hex("#FDF2F8"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    pub id: u32,
    pub name: String,
    pub description: String,
    pub color: ProjectColor,
    pub status: ProjectStatus,
    /// The tasks counted on the cards; the board shows the ones in [`super::Task`].
    pub task_count: usize,
    pub member_count: usize,
    /// The members whose avatars the cards show, owner first.
    pub members: Vec<u32>,
    /// Done, 0 to 100.
    pub progress: u32,
    pub in_progress: usize,
    pub due_soon: usize,
}
