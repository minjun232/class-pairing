//! 자리배치 핵심 로직. 파일·네트워크·OS에 접근하지 않는다.

pub mod layout;
pub mod rules;
pub mod solver;

pub use layout::{CellEdit, CellKind, ClassroomConfig, GenderMode, Layout, Preset, Seat};
pub use rules::{Facility, Place, Rule, Zone, ZoneSettings};
pub use solver::{pairs_of, satisfies, solve, Arrangement, Gender, Problem, SolveError, Student};
