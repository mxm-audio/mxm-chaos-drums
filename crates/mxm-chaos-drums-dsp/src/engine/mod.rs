//! The five engines a model is built from. Each changes character as its cell's charge fills and
//! drains; none knows about charge itself: the model turns charge into each engine's operating point.

pub mod dust;
pub mod fold;
pub mod ring;
pub mod spark;
pub mod strand;
pub mod svf;

pub use dust::Dust;
pub use fold::Fold;
pub use ring::Ring;
pub use spark::Spark;
pub use strand::{Strand, StrandDrive};
