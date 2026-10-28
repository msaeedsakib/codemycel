# Fixtures

Small repositories with hand-reviewed ground truth. Each lives at `fixtures/<language>/<name>/`,
committed as a plain directory without its own `.git`, and its expected dumps live beside it in
`fixtures/<language>/<name>.expected/`:

- `nodes.txt`: one node per line, `<semantic path>\t<kind>\t<line_start>-<line_end>`, sorted.
- `edges.txt`: one edge per line,
  `<src semantic path> (<src kind>)\t<edge kind>\t<dst semantic path> (<dst kind>)\t<occurrence count>`,
  sorted. Kinds are part of node identity, so every edge line carries both.
- `unresolved.txt`: one candidate per line, `<file>:<line>:<col>\t<role>\t<text>\t<reason>`,
  sorted. Lines are 1-based, columns 0-based characters.

Lines sort by plain string order, so `a.rs:10:` precedes `a.rs:9:`.

The test harness in `crates/codemycel-index/tests/harness` indexes a fixture into a temporary
directory and diffs the three dumps against the expected files, printing a unified diff on
mismatch. Until the resolvers land, every candidate is listed in `unresolved.txt` with reason
`server_unavailable`, and `edges.txt` holds only `contains`, `defines`, `depends_on`, and
`mentions`.

Running a fixture test with `CODEMYCEL_UPDATE_EXPECTED=1` rewrites its expected files from the
current dumps. A regenerated file is ground truth only after every changed line has been reviewed
against the fixture's source.
