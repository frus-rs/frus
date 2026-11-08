//! The pieces at the top of a project's pages.

use frus::{Align, Callback, GoRouter, Justify, Row};

use crate::common::{
    chrome::back,
    icons::Glyph,
    routes::{
        new_task, project_activity, project_board, project_members, project_overview,
        project_tasks, PROJECTS,
    },
    shared::on,
    ui::{back_link, small_primary, underline_tabs, W},
};

/// "‹ Frus Framework" on the left, "+ Task" on the right.
pub(crate) fn back_and_new_task(router: &GoRouter, project: u32, name: &str) -> W {
    let add = {
        let router = router.clone();
        on(move || router.push(new_task(project)))
    };
    Box::new(
        Row::new()
            .main_axis_alignment(Justify::SpaceBetween)
            .cross_axis_alignment(Align::Center)
            .child(back_link(
                name,
                16.0,
                8.0,
                back(router, PROJECTS.to_string()),
            ))
            .child(small_primary(Glyph::Plus, "Task", 36.0, 16.0, add)),
    )
}

/// "‹ Frus Framework", back to the project's board.
pub(crate) fn back_to_project(router: &GoRouter, project: u32, name: &str) -> W {
    back_link(name, 16.0, 8.0, back(router, project_board(project)))
}

/// The tabs of a project: Board, Overview, Tasks, Members, Activity.
pub(crate) fn project_tabs(router: &GoRouter, project: u32, selected: usize) -> W {
    let router = router.clone();
    underline_tabs(
        &["Board", "Overview", "Tasks", "Members", "Activity"],
        selected,
        move |i| -> Callback {
            let router = router.clone();
            on(move || {
                if i == selected {
                    return;
                }
                let to = match i {
                    0 => project_board(project),
                    1 => project_overview(project),
                    2 => project_tasks(project),
                    3 => project_members(project),
                    _ => project_activity(project),
                };
                router.push(to);
            })
        },
    )
}
