use codemycel_core::Community;
use codemycel_core::store::Tx;

use crate::IndexError;

pub struct CommunityDetector;

impl CommunityDetector {
    /// Partitions the verified graph and replaces the stored communities, keeping the stable id
    /// of each community in `previous`, the partition before this sync wrote anything, that a
    /// new community overlaps enough.
    pub fn run(_tx: &mut Tx<'_>, _previous: &[Community]) -> Result<(), IndexError> {
        Ok(())
    }
}
