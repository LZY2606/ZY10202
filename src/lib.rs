//! 刃缘解像台 library surface (used by the binary and the test suite).

pub mod analysis;
pub mod db;
pub mod fixtures;
pub mod geometry;
pub mod image;
pub mod server;

pub use analysis::{analyze, AnalysisParams, AnalysisResult, DerivativeKernel, PitchMode};
pub use fixtures::all_fixtures;
pub use geometry::{Roi, Rotation};
pub use image::{checksum_hex, GrayImage, PitchBand};
pub use server::{build_router, memory_db};
