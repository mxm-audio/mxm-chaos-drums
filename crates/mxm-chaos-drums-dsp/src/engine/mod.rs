//! The engines a model is built from. Each changes character as its cell's charge fills and drains;
//! none knows about charge itself: the model turns charge into each engine's operating point.
//!
//! P1 has three of the five: Ring, Spark and Dust. Fold and Loop arrive in P2.

pub mod dust;
pub mod ring;
pub mod spark;
pub mod svf;

pub use dust::Dust;
pub use ring::Ring;
pub use spark::Spark;
