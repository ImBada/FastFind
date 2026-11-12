//! Platform-independent logic of FastFind, ported one-to-one from SearchEverything's Swift sources.

pub mod file_info;
pub mod index;
pub mod query;
pub mod row;
pub mod search;
pub mod sort;
pub mod text;

pub use file_info::FileInfo;
pub use index::Index;
pub use search::{Plan, SearchOptions, Shown};
pub use sort::SortOption;
