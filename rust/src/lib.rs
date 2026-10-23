//! A minimal sample core for the playground.
//!
//! This crate is a stand-in: a tiny stack language just large enough to
//! exercise every surface of the playground (stack view, dictionary panel,
//! step mode, persistence, the worker round-trip). Replace `interpreter`,
//! `tokenizer` and `value` with your own core and keep `wasm` speaking the
//! same protocol (`src/wasm-interpreter-types.ts`).

pub mod interpreter;
pub mod model;
pub mod tokenizer;
pub mod value;

#[cfg(feature = "wasm")]
mod wasm;

pub use interpreter::{ExecError, Interpreter};
pub use value::Value;
