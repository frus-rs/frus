# Fixtures of tests/messages.rs.
app-title = My Tasks
hello = Hello, { $name }!
task-count = { $n ->
    [0] No tasks
    [one] { $n } task
   *[other] { $n } tasks
  }
# A reference passes its arguments on.
welcome = { hello } You have { task-count }.
type = A keyword as a key
