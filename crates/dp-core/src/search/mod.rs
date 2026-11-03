//! Candidate search: CPA tables → event/flight association → joint locked scoring → refinement.
//! Stage design: see docs/ARCHITECTURE.md#search.

pub mod backend;
pub mod calibration;
pub mod catalog;
mod fixed;
pub mod global;
#[cfg(feature = "cuda")]
mod gpu;
pub mod hypothesis;
pub mod job;
pub mod likelihood;
pub mod locate;
pub mod modes;
pub mod pipeline;
pub mod score;
pub mod surface;
mod sweep;

pub use backend::{Backend, Scorer};
pub use catalog::{Catalog, MemCatalog};
pub use hypothesis::{AssocParams, EventSpec};
pub use locate::locate;
pub use pipeline::{Region, SearchInput, SearchParams, SearchResult, search};
