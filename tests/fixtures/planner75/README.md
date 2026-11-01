# Frozen pre-planner plate

`plate-v028.step.gz` is the committed `parts/plate.step`, written by the unmodified
v0.2.8-pinned Specify checkout at the parent of `8d88347`, accepting defaults with
`{"part.material":"steel"}`. It has 21 dimensions and 28 characteristics. This is
an independent historical input: never regenerate it with the current writer.

`tests/planner_dimensions.rs` resumes it through the shared planner (#75), preserves
surviving numbers (including the reversed 10 mm extent), retires grid member
locations 6–11 and 21–26, allocates above 28 and checks a second, unchanged resume.
