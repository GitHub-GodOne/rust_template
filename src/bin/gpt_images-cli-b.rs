// Same CLI as `main.rs` under another file name, so one copy can be rebuilt
// while the service runs the other (Windows locks a running .exe).
include!("main.rs");
