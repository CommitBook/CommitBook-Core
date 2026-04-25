pub mod operations;
pub mod remote;

#[cfg(test)]
pub(crate) mod test_support;

pub use operations::{ChangesSummary, GitRepo};
