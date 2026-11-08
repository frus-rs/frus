/// The search page: what is being typed.
#[derive(Default)]
pub(crate) struct SearchState {
    pub query: String,
}

impl SearchState {
    pub(crate) fn set_query(&mut self, query: String) {
        self.query = query;
    }
}

/// The results page: the words searched, which type is shown.
pub(crate) struct ResultsState {
    pub query: String,
    /// 0 all, 1 projects, 2 tasks, 3 members.
    pub kind: usize,
}

impl ResultsState {
    pub(crate) fn set_query(&mut self, query: String) {
        self.query = query;
    }
}

/// The filters page.
pub(crate) struct FiltersState {
    /// 0 all, 1 projects, 2 tasks, 3 members.
    pub kind: usize,
    /// Todo, In progress, Review, Done.
    pub statuses: [bool; 4],
    pub labels: Vec<(String, bool)>,
    /// 0 any time, 1 due this week, 2 overdue.
    pub range: usize,
}

impl Default for FiltersState {
    fn default() -> Self {
        FiltersState {
            kind: 0,
            statuses: [false, true, false, false],
            labels: vec![
                ("frontend".into(), false),
                ("mobile".into(), false),
                ("routing".into(), false),
            ],
            range: 0,
        }
    }
}
