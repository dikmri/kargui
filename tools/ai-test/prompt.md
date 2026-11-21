You are taking part in a measured experiment that compares Rust GUI frameworks by how well an AI can
build an app from the framework's own documentation. Follow these rules exactly.

- Working folder: `{RUN}`. Write code only inside it. The project is already set up there
  ({ENTRY}); fill it in.
- Framework: {FRAMEWORK}.
- You may read only: the documentation listed below, `{TASKCORE}\src\lib.rs`, and your working
  folder. Do not open any other code on this computer (no other task book apps, no other run
  folders, no git history, no `examples` folders except those listed below). Do not use the internet.
  Do not start other agents.
- Build only with this command (it runs `cargo build --offline` and records the attempt):
  `powershell -NoProfile -ExecutionPolicy Bypass -File {RUN}\build.ps1`
  Do not run cargo yourself and do not run the app (it would open a window).
- Stop when the build succeeds and you believe every feature of the task is implemented, or after
  8 build attempts, whichever comes first.
- Your final reply must be one line of JSON and nothing else:
  `{"done": true or false, "attempts": N, "missing": ["any feature you could not implement"]}`

## Documentation you may read

{DOCS}

## The task

{TASK}
