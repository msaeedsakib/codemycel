use serde::{Deserialize, Serialize};

/// The community map's data: every community with its size, and the verified relations between
/// symbols of two communities counted per pair. Communities are derived, never verified.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommunityGraph {
    pub communities: Vec<CommunityNode>,
    pub links: Vec<CommunityLink>,
    pub derived: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommunityNode {
    /// The stable id; `/api/community/<id>` lists the members.
    pub id: String,
    pub label: String,
    pub size: usize,
}

/// `source` sorts before `target`; `weight` counts edges in both directions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommunityLink {
    pub source: String,
    pub target: String,
    pub weight: usize,
}
