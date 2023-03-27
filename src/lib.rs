#![warn(
    clippy::undocumented_unsafe_blocks,
    missing_debug_implementations,
    missing_docs,
    clippy::missing_safety_doc,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::serde_api_misuse
)]
#![doc = include_str!("../README.md")]
#![deny(unsafe_op_in_unsafe_fn)]

pub mod sync;
