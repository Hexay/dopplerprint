//! Candidate search: CPA tables → event/flight association → joint locked scoring → refinement.
//! Stage design: see docs/ARCHITECTURE.md#search.

pub mod catalog;
pub mod hypothesis;
pub mod likelihood;
pub mod locate;
pub mod pipeline;
pub mod score;

pub use catalog::{Catalog, MemCatalog};
pub use hypothesis::{AssocParams, EventSpec};
pub use locate::locate;
pub use pipeline::{Region, SearchInput, SearchParams, SearchResult, search};
