You are reviewing apps that other AIs wrote for a measured comparison of Rust GUI frameworks. The task they
were given is in `D:\soft\kargui\tools\ai-test\task.md` (read it first). You review all runs of one framework:
{FRAMEWORK}. The runs are these folders (read only the source files the app consists of: `src\*.rs`, and for
Tauri also `src-tauri\src\*.rs` and `ui\*`):

{RUNS}

To judge how the framework behaves (in particular input-method handling), you may read the framework's own
documentation and source: {DOCS}. Do not run or build anything, and do not edit any file.

Score every run on these 8 points. For each point give 1 (done and correct), 0.5 (partly done, or done in a
way that will not work as asked) or 0 (missing or broken). Judge what the code does, not what its comments say.

1. Window: title "Task Book" and size `WINDOW_SIZE`.
2. Top row: heading and a working "Dark" switch that changes the look.
3. Adding: "New task" field taking the remaining width; "Add" button and Enter both add the task and clear the field.
4. IME Enter: the Enter that commits an input-method composition does not add a task (only a later Enter does).
   Decide from how the framework delivers Enter and composition events, using its documentation or source.
5. Filtering: "Filter" field (about 240 px), the three status choices, and the "Showing X of Y" label using
   `Store::filter` and `group`.
6. IME filtering: while composing in the Filter field, filtering does not run until the text is committed.
   Decide from the framework's behavior, as in point 4.
7. The list: scrolls, builds only visible rows, each row has a check box with the task text that toggles done,
   done tasks dimmed or struck through, and a "Delete" button at the right end.
8. Bottom row: "Add 10,000", "Clear done", "Clear all" buttons and the "Active N · Done M" label.

Write your result as JSON to `{OUT}` with this shape, then reply with one line saying the file was written:

{"framework": "...", "runs": {"<run folder name>": {"scores": [s1, s2, s3, s4, s5, s6, s7, s8], "total": T,
"notes": "one or two sentences on what is missing or wrong"}}}
