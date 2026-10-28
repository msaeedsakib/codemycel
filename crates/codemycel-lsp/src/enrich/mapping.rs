use std::path::{Path, PathBuf};

use codemycel_core::{LanguageAdapter, NodeRef, RepoPath, UnresolvedReason};
use lsp_types::{Range, Uri};

use super::IndexView;
use crate::client::uri_to_path;

/// Where a server location lands in the index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Node(NodeRef),
    /// Inside the header of the `index`th impl record of `file`.
    ImplHeader {
        file: RepoPath,
        index: usize,
    },
    External,
    Unindexed,
    Unmapped,
}

impl Target {
    /// The reason a candidate gets when its definition lands here; `None` for a node.
    pub fn reason(&self) -> Option<UnresolvedReason> {
        match self {
            Target::Node(_) => None,
            Target::ImplHeader { .. } | Target::Unmapped => Some(UnresolvedReason::Unmapped),
            Target::External => Some(UnresolvedReason::ExternalDependency),
            Target::Unindexed => Some(UnresolvedReason::UnindexedFile),
        }
    }
}

/// Maps server locations to nodes by canonical path and selection-range containment.
pub struct Mapper<'a> {
    view: &'a IndexView,
    language: &'a dyn LanguageAdapter,
    repo_root: PathBuf,
}

impl<'a> Mapper<'a> {
    pub fn new(view: &'a IndexView, language: &'a dyn LanguageAdapter, repo_root: &Path) -> Self {
        Self {
            view,
            language,
            repo_root: repo_root
                .canonicalize()
                .unwrap_or_else(|_| repo_root.to_owned()),
        }
    }

    /// The repository-relative path of a server URI, after resolving symlinks; `Err` with the
    /// classification when it is outside the repository's own code.
    pub fn repo_path(&self, uri: &Uri) -> Result<RepoPath, Target> {
        let path = uri_to_path(uri).ok_or(Target::External)?;
        let canonical = path.canonicalize().map_err(|_| Target::External)?;
        if self
            .language
            .external_path_predicate(&canonical, &self.repo_root)
        {
            return Err(Target::External);
        }
        let relative = canonical
            .strip_prefix(&self.repo_root)
            .map_err(|_| Target::External)?;
        let text: Vec<String> = relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        RepoPath::new(&text.join("/")).map_err(|_| Target::Unindexed)
    }

    /// The innermost node whose selection holds `range.start`; else the first node whose
    /// selection lies inside `range` (servers that return a whole declaration); else an impl
    /// header; else the file node when `range` starts at the top of the file.
    pub fn locate(&self, uri: &Uri, range: Range) -> Target {
        let file = match self.repo_path(uri) {
            Ok(file) => file,
            Err(target) => return target,
        };
        let Some(view) = self.view.file(&file) else {
            return Target::Unindexed;
        };
        let (Some(start), Some(end)) =
            (view.index.offset(range.start), view.index.offset(range.end))
        else {
            return Target::Unmapped;
        };
        let holding = view
            .nodes
            .iter()
            .filter_map(|node| Some((node, node.selection?)))
            .filter(|(_, sel)| sel.start <= start && start <= sel.end)
            .min_by_key(|(_, sel)| sel.end - sel.start);
        if let Some((node, _)) = holding {
            return Target::Node(node.node_ref());
        }
        let inside = view
            .nodes
            .iter()
            .filter_map(|node| Some((node, node.selection?)))
            .filter(|(_, sel)| start <= sel.start && sel.end <= end && start < end)
            .min_by_key(|(_, sel)| sel.start);
        if let Some((node, _)) = inside {
            return Target::Node(node.node_ref());
        }
        if let Some(index) = view
            .impl_records
            .iter()
            .position(|record| record.header.start <= start && start < record.header.end)
        {
            return Target::ImplHeader { file, index };
        }
        if range.start.line == 0 && range.start.character == 0 {
            return Target::Node(view.file_node.clone());
        }
        Target::Unmapped
    }
}
