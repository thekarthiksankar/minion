pub mod in_place;

pub use in_place::InPlaceBranchIsolation;

use std::path::Path;

/// Abstraction over how a run is isolated from the developer's working tree.
/// Concrete implementations handle setup in their constructor and teardown in Drop.
pub trait Isolation: Send + Sync {
    fn working_path(&self) -> &Path;
    fn branch(&self) -> &str;
}
