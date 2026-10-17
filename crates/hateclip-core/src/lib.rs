//! Platform-independent core of HateClip: clipboard item model, settings,
//! and the persistent history store.

pub mod model;
pub mod settings;
pub mod store;

pub use model::{Capture, ClipItem, ItemDetail, ItemKind, Payload, Query, Representation};
pub use settings::Settings;
pub use store::Store;
