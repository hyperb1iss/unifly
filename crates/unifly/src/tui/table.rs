//! Shared plumbing for the list screens' tables: column widths planned from
//! the cell contents, and selection that follows an entity across refreshes.

mod columns;
mod selection;

pub use columns::{Column, plan_columns};
pub use selection::reselect;
