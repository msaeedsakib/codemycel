use std::path::{Path, PathBuf};

use codemycel_core::ServerAdapter;

/// `<home>/servers/<server>/`.
pub fn server_dir(home: &Path, adapter: &dyn ServerAdapter) -> PathBuf {
    home.join("servers").join(adapter.name())
}

/// `<home>/servers/<server>/<pinned version>/`.
pub fn version_dir(home: &Path, adapter: &dyn ServerAdapter) -> PathBuf {
    server_dir(home, adapter).join(adapter.pinned_version())
}

/// The pinned version's binary inside the cache.
pub fn cached_binary(home: &Path, adapter: &dyn ServerAdapter) -> PathBuf {
    version_dir(home, adapter).join(binary_file_name(adapter))
}

pub fn binary_file_name(adapter: &dyn ServerAdapter) -> String {
    format!("{}{}", adapter.binary_name(), std::env::consts::EXE_SUFFIX)
}
