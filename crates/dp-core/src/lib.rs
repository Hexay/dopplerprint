//! doppler_print core: locate a recording from aircraft Doppler curves matched to ADS-B.
//! Module map and data flow: see docs/ARCHITECTURE.md.

pub mod adsb;
pub mod audio;
pub mod config;
pub mod dem;
pub mod eval;
pub mod events;
pub mod geo;
pub mod output;
pub mod physics;
pub mod search;
pub mod sim;
pub mod track;
