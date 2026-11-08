/// The projects list: what is searched and which status is shown.
#[derive(Default)]
pub(crate) struct ProjectsState {
    pub query: String,
    /// 0 all, 1 active, 2 archived or completed.
    pub filter: usize,
}

impl ProjectsState {
    pub(crate) fn set_query(&mut self, query: String) {
        self.query = query;
    }
}
