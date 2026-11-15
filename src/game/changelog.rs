//! Block change log: the first state of every block changed during a round, so the
//! arena can be put back exactly. Pure; persistence lives in `host::crashlog`.

use std::collections::HashMap;

use crate::model::BlockPos;

#[derive(Debug, Default, Clone)]
pub struct ChangeLog {
    originals: HashMap<BlockPos, u16>,
    order: Vec<BlockPos>,
}

impl ChangeLog {
    /// Records `state` as the original of `pos` unless it was already recorded.
    /// Returns true if this was a new entry.
    pub fn record(&mut self, pos: BlockPos, state: u16) -> bool {
        if self.originals.contains_key(&pos) {
            return false;
        }
        self.originals.insert(pos, state);
        self.order.push(pos);
        true
    }

    pub fn contains(&self, pos: &BlockPos) -> bool {
        self.originals.contains_key(pos)
    }

    pub fn original(&self, pos: &BlockPos) -> Option<u16> {
        self.originals.get(pos).copied()
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// Entries to restore, most recent change first (so blocks placed on top of other
    /// placed blocks come off before their support).
    pub fn into_restore_list(self) -> Vec<(BlockPos, u16)> {
        let ChangeLog { originals, order } = self;
        order
            .into_iter()
            .rev()
            .filter_map(|p| originals.get(&p).map(|s| (p, *s)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_state_wins_and_order_is_reversed() {
        let mut log = ChangeLog::default();
        assert!(log.record(BlockPos::new(1, 2, 3), 10));
        assert!(!log.record(BlockPos::new(1, 2, 3), 99));
        assert!(log.record(BlockPos::new(1, 3, 3), 0));
        assert_eq!(log.original(&BlockPos::new(1, 2, 3)), Some(10));
        assert_eq!(
            log.into_restore_list(),
            vec![(BlockPos::new(1, 3, 3), 0), (BlockPos::new(1, 2, 3), 10)]
        );
    }
}
