// Studio headless port. Upstream: x-iso/FlexInput c69f53ad3bca4ed34ef5f2d63509addaa7c767be (MIT).
// Only execution/data types: no UI, background host, hardware, audio or sockets.
// Unused helpers remain upstream source to keep rebasing reviewable.
#![allow(dead_code, unused_imports)]
pub mod eval;
pub mod graph;
pub mod hr_timer;
pub use hr_timer::HrWaiter;
pub use eval::jsm_motion_curves as motion_curves;
pub mod state;
pub use eval::eval_graph_tick;
pub use graph::{NodeSnap, ProcessingGraph};
pub use state::NodeState;
