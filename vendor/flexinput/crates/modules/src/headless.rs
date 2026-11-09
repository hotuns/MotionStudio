//! Selected upstream module descriptors; no UI, JSM or provider startup.
#![allow(dead_code)]
pub mod generator;
pub mod logic;
pub mod math;
pub mod processing;
mod util;
pub fn all_modules() -> Vec<flexinput_core::ModuleRegistration> {
    let mut modules = processing::registrations();
    modules.extend(math::registrations());
    modules.extend(logic::registrations());
    modules.extend(generator::registrations());
    modules
}
