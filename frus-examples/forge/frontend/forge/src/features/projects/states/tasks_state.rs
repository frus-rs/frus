/// A project's task list: what is searched and which status is shown.
#[derive(Default)]
pub(crate) struct ProjectTasksState {
    pub query: String,
    /// 0 all, then Todo, In progress, Review.
    pub filter: usize,
}

impl ProjectTasksState {
    pub(crate) fn set_query(&mut self, query: String) {
        self.query = query;
    }
}
