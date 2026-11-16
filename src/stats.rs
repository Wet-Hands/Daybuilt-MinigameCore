//! Player statistics per mode. Pure; saved as `stats.json` by `host::storage`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::game::effects::RoundResult;

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct PlayerStats {
    pub name: String,
    /// mode → key → value. Keys: played, wins, losses, draws, kills, deaths, win_streak,
    /// best_win_streak, plus mode counters.
    pub modes: BTreeMap<String, BTreeMap<String, i64>>,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct Stats {
    pub players: BTreeMap<String, PlayerStats>,
}

impl Stats {
    pub fn apply(&mut self, result: &RoundResult) {
        for p in &result.players {
            let entry = self.players.entry(p.uuid.clone()).or_default();
            entry.name = p.name.clone();
            let m = entry.modes.entry(result.mode.clone()).or_default();
            let mut add = |k: &str, v: i64| *m.entry(k.to_string()).or_insert(0) += v;
            add("played", 1);
            add("kills", i64::from(p.kills));
            add("deaths", i64::from(p.deaths));
            if result.draw {
                add("draws", 1);
            } else if p.won {
                add("wins", 1);
            } else {
                add("losses", 1);
            }
            for (k, v) in &p.counters {
                add(k, *v);
            }
            if !result.draw {
                let streak = if p.won {
                    m.get("win_streak").copied().unwrap_or(0) + 1
                } else {
                    0
                };
                m.insert("win_streak".into(), streak);
                let best = m.get("best_win_streak").copied().unwrap_or(0).max(streak);
                m.insert("best_win_streak".into(), best);
            }
        }
    }

    /// A value for one mode, or summed over all modes (`best_*` and `win_streak`
    /// take the highest instead of the sum).
    pub fn get(&self, uuid: &str, mode: Option<&str>, key: &str) -> i64 {
        let Some(p) = self.players.get(uuid) else {
            return 0;
        };
        match mode {
            Some(m) => p
                .modes
                .get(m)
                .and_then(|s| s.get(key))
                .copied()
                .unwrap_or(0),
            None => {
                let values = p.modes.values().filter_map(|s| s.get(key).copied());
                if key.starts_with("best_") || key == "win_streak" {
                    values.max().unwrap_or(0)
                } else {
                    values.sum()
                }
            }
        }
    }

    /// Finds a player by name (case-insensitive), for offline lookups.
    pub fn uuid_by_name(&self, name: &str) -> Option<String> {
        self.players
            .iter()
            .find(|(_, p)| p.name.eq_ignore_ascii_case(name))
            .map(|(u, _)| u.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::effects::PlayerResult;

    fn result(won: bool, draw: bool) -> RoundResult {
        RoundResult {
            mode: "bedwars".into(),
            draw,
            players: vec![PlayerResult {
                uuid: "u1".into(),
                name: "Steve".into(),
                won,
                kills: 2,
                deaths: 1,
                counters: [("beds_broken".to_string(), 1)].into(),
            }],
        }
    }

    #[test]
    fn counts_and_streaks() {
        let mut s = Stats::default();
        s.apply(&result(true, false));
        s.apply(&result(true, false));
        s.apply(&result(false, true));
        assert_eq!(s.get("u1", Some("bedwars"), "wins"), 2);
        assert_eq!(s.get("u1", Some("bedwars"), "win_streak"), 2);
        assert_eq!(s.get("u1", None, "draws"), 1);
        s.apply(&result(false, false));
        assert_eq!(s.get("u1", None, "win_streak"), 0);
        assert_eq!(s.get("u1", None, "best_win_streak"), 2);
        assert_eq!(s.get("u1", None, "kills"), 8);
        assert_eq!(s.get("u1", None, "beds_broken"), 4);
        assert_eq!(s.uuid_by_name("steve"), Some("u1".into()));
    }
}
