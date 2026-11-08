/// A task's page: the comment being written.
#[derive(Default)]
pub(crate) struct TaskDetailState {
    pub draft: String,
}

impl TaskDetailState {
    pub(crate) fn set_draft(&mut self, draft: String) {
        self.draft = draft;
    }
}
