/// A project's activity: which kind of entry is shown.
#[derive(Default)]
pub(crate) struct ProjectActivityState {
    /// 0 everything, 1 tasks, 2 comments.
    pub filter: usize,
}
