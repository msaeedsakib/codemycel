use serde::{Deserialize, Serialize};

use super::{NodeRef, RepoPath};

/// The `packages` side-table row of a `package` node (spec §6.7, §9). External packages have no
/// manifest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Package {
    pub node: NodeRef,
    pub name: String,
    pub version: Option<String>,
    pub external: bool,
    pub manifest: Option<RepoPath>,
}
