//! A fictional machine and its data (docs/PUBLIC-PLAN.md, P1): `generate` builds a demo database from nothing, `world` is the invented
//! machine (programs, makers, addresses) that both the generator and the live demo collector draw from.

pub mod generate;
pub mod world;

pub use generate::{generate, Summary};
