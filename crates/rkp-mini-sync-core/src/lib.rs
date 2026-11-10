//! `rkp-mini-sync-core`: everything the sync protocol needs that is
//! independent of the network stack, of secret provisioning and of Android.
//!
//! - [`envelope`]: age v1 passphrase encryption with ASCII armor.
//! - [`manifest`]: canonical CBOR plaintext (manifest + keybox bytes).
//! - [`transport`]: provider-neutral fetch/put trait plus a local directory transport.
//! - [`protocol`]: push base selection and receiver replay/rollback rules.
//! - [`secret`]: secret-file loading (permission checks, newline handling).
//!
//! Basic types (errors, keybox validation, state files, limits, digests) come
//! from `rkp-mini-core`, so the deployer links those without `age`.

pub mod envelope;
pub mod manifest;
pub mod protocol;
pub mod secret;
pub mod transport;

pub use age::secrecy;
