//! Voice-graph modules (spec: `.claude/notes/aems-voice-graph-spec.md` §4, §6).
//!
//! Each module processes one 256-frame block of planar f32 channels at the block's rate.
pub mod biquad;
pub mod delay;
pub mod fss;
pub mod gain;
pub mod pan;
pub mod peaking;
pub mod resample;
pub mod reverb;
pub mod routes;
pub mod send;
pub mod shelf;

/// 1/32767 as the retail cell holds it (0x822F8898): converts 15-bit levels to linear gain.
pub const INV_32767: f32 = 0.000_030_518_509;

/// Degrees per azimuth unit (65536 = 360°), cell 0x822F88E8.
pub const DEGREES_PER_UNIT: f32 = 360.0 / 65536.0;
