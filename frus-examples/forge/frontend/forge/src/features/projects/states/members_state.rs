/// A project's member list: what is searched.
#[derive(Default)]
pub(crate) struct ProjectMembersState {
    pub query: String,
}

impl ProjectMembersState {
    pub(crate) fn set_query(&mut self, query: String) {
        self.query = query;
    }
}
