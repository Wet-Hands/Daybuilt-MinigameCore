use super::*;
use crate::game::tests::{arena, mode};

fn state() -> State {
    let mut s = State::new();
    s.arenas.insert("castle".into(), arena());
    s.modes.insert("bedwars".into(), mode(""));
    s.owners.insert("bedwars".into(), "bedwars_plugin".into());
    s
}

#[test]
fn matchmaking_join_events_and_index() {
    let mut s = state();
    let (game, effects) = s
        .join(vec![("u1".into(), "Steve".into())], "bedwars", None, None)
        .unwrap();
    assert_eq!(game, "castle");
    assert!(!effects.is_empty());
    assert_eq!(s.index.get("u1"), Some(&"castle".to_string()));
    // Already in a game.
    let err = s
        .join(vec![("u1".into(), "Steve".into())], "bedwars", None, None)
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::AlreadyInGame);
    // A second player goes to the same game (default format "duo").
    s.join(vec![("u2".into(), "Alex".into())], "bedwars", None, None)
        .unwrap();
    // The arena is taken by the duo game, so ffa finds nothing.
    let err = s
        .join(
            vec![("u3".into(), "Bob".into())],
            "bedwars",
            Some("ffa"),
            None,
        )
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::NoArena);
    let (events, dropped) = s.poll("bedwars_plugin", 100);
    assert_eq!(dropped, 0);
    assert!(matches!(events[0].event, Event::GameCreated { .. }));
    assert!(
        events
            .iter()
            .any(|e| matches!(e.event, Event::CountdownStarted { .. }))
    );
    // Seq numbers increase.
    assert!(events.windows(2).all(|w| w[0].seq < w[1].seq));
}

#[test]
fn leaving_an_unstarted_game_frees_the_arena() {
    let mut s = state();
    s.join(vec![("u1".into(), "Steve".into())], "bedwars", None, None)
        .unwrap();
    s.leave("u1", true);
    assert!(s.games.is_empty());
    assert!(s.index.is_empty());
    let (events, _) = s.poll("bedwars_plugin", 100);
    assert!(matches!(
        events.last().unwrap().event,
        Event::GameReset { .. }
    ));
}

#[test]
fn full_round_through_ticks_and_restore() {
    let mut s = state();
    s.join(
        vec![("u1".into(), "Steve".into()), ("u2".into(), "Alex".into())],
        "bedwars",
        Some("ffa"),
        None,
    )
    .unwrap();
    for _ in 0..(6 * 20) {
        s.tick();
    }
    assert!(matches!(s.games["castle"].phase, GamePhase::Running { .. }));
    assert_eq!(
        s.game_at(Some("world"), BlockPos::new(0, 64, 0)),
        Some("castle".into())
    );
    assert!(s.record_block("castle", BlockPos::new(0, 64, 0), 1));
    assert!(!s.record_block("castle", BlockPos::new(0, 64, 0), 2));
    let effects = s.death("u1", "void").unwrap();
    assert!(!effects.is_empty());
    assert!(matches!(s.games["castle"].phase, GamePhase::Ending { .. }));
    assert_eq!(s.stats.get("u2", Some("bedwars"), "wins"), 1);
    assert_eq!(s.stats.get("u1", Some("bedwars"), "losses"), 1);
    for _ in 0..(3 * 20) {
        s.tick();
    }
    assert!(s.index.is_empty());
    let job = s.restores.pop_front().unwrap();
    assert_eq!(job.blocks, vec![(BlockPos::new(0, 64, 0), 1)]);
    s.restore_done("castle");
    assert!(s.games.is_empty());
}

#[test]
fn queue_drops_oldest_when_full() {
    let mut s = state();
    for i in 0..(QUEUE_CAP + 5) {
        s.push_event(
            "bedwars",
            Event::TimeUp {
                game: i.to_string(),
            },
        );
    }
    let (events, dropped) = s.poll("bedwars_plugin", 10);
    assert_eq!(dropped, 5);
    assert_eq!(events[0].event.game(), "5");
    // Modes without an owner queue nothing.
    s.push_event("spleef", Event::TimeUp { game: "x".into() });
    assert!(s.queues.len() == 1);
}

#[test]
fn resolve_names() {
    let mut s = state();
    s.player_online("0000-1", "Steve", 7);
    assert_eq!(s.resolve("steve"), Some(("0000-1".into(), "Steve".into())));
    assert_eq!(s.resolve("0000-1").map(|r| r.1), Some("Steve".into()));
    s.player_online("0000-1", "Steve2", 9);
    assert!(s.resolve("steve").is_none());
    assert_eq!(s.entities.get(&9), Some(&"0000-1".to_string()));
    assert!(!s.entities.contains_key(&7));
}
