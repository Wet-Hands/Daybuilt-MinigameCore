use std::sync::Arc;

use super::*;
use crate::config::Config;
use crate::model::{arena::ArenaDef, mode::ModeDef};

pub fn arena() -> Arc<ArenaDef> {
    Arc::new(
        ArenaDef::parse(
            "castle",
            r#"
world = "world"
pos1 = { x = -50, y = 0, z = -50 }
pos2 = { x = 50, y = 120, z = 50 }
spectator = { x = 0.5, y = 100, z = 0.5 }
spawns = [{ x = 1, y = 65, z = 1 }, { x = 2, y = 65, z = 2 }]

[[teams]]
id = "red"
color = "red"
spawns = [{ x = 30.5, y = 65, z = 0.5 }]

[[teams]]
id = "blue"
color = "blue"
spawns = [{ x = -30.5, y = 65, z = 0.5 }]

[[spawners]]
group = "iron"
item = "minecraft:iron_ingot"
at = { x = 30.5, y = 65, z = 3.5 }
interval = 10
cap = 3
label = "&fIron {stock}"

[[regions]]
name = "red_base"
pos1 = { x = 25, y = 60, z = -5 }
pos2 = { x = 35, y = 80, z = 5 }
pvp = false
"#,
        )
        .unwrap(),
    )
}

pub fn mode(extra: &str) -> Arc<ModeDef> {
    Arc::new(
        ModeDef::parse(
            "bedwars",
            &format!(
                r#"
countdown_seconds = 5
ending_seconds = 2
[rules]
respawn = true
respawn_seconds = 2
{extra}
[formats.duo]
team_size = 2
teams = 2
min_players = 2
[formats.ffa]
team_mode = "solo"
teams = 4
min_players = 2
rules = {{ respawn = false }}
[[schedule]]
at = 3
id = "diamond_2"
label = "Diamond II"
actions = ["spawner_interval:iron:40"]
[rewards]
win = ["eco give {{player}} 10"]
kill = ["say {{player}} killed {{victim}}"]
"#
            ),
        )
        .unwrap(),
    )
}

pub fn cfg() -> Config {
    Config::default_config()
}

pub fn party(names: &[&str]) -> Vec<(String, String)> {
    names.iter().map(|n| (format!("u-{n}"), n.to_string())).collect()
}

/// Advances `game` by whole seconds, calling `second` each time.
fn run(game: &mut Game, cfg: &Config, now: &mut u64, secs: u32, out: &mut Out) {
    for _ in 0..secs {
        *now += TICKS;
        let ctx = Ctx { now: *now, cfg };
        game.second(&ctx, out);
    }
}

fn started(format: &str, names: &[&str]) -> (Game, Config, u64) {
    let cfg = cfg();
    let mut g = Game::new(arena(), mode(""), format, 1).unwrap();
    let mut now = 0;
    let mut out = Out::default();
    let ctx = Ctx { now, cfg: &cfg };
    // One at a time, so they are spread over teams.
    for n in names {
        g.add_players(&ctx, &party(&[n]), &mut out).unwrap();
    }
    run(&mut g, &cfg, &mut now, 6, &mut out);
    assert!(matches!(g.phase, GamePhase::Running { .. }), "{:?}", g.phase);
    (g, cfg, now)
}

#[test]
fn party_stays_together_and_countdown_runs() {
    let cfg = cfg();
    let mut g = Game::new(arena(), mode(""), "duo", 1).unwrap();
    let mut out = Out::default();
    let ctx = Ctx { now: 0, cfg: &cfg };
    g.add_players(&ctx, &party(&["a", "b"]), &mut out).unwrap();
    assert_eq!(g.players["u-a"].team, g.players["u-b"].team);
    assert!(matches!(g.phase, GamePhase::Countdown { .. }));
    g.add_players(&ctx, &party(&["c"]), &mut out).unwrap();
    assert_ne!(g.players["u-c"].team, g.players["u-a"].team);
    // Full countdown kicks in at 4/4.
    g.add_players(&ctx, &party(&["d"]), &mut out).unwrap();
    assert_eq!(
        g.add_players(&ctx, &party(&["e"]), &mut out),
        Err(JoinError::Full)
    );
    // Leaving below min stops the countdown.
    for n in ["b", "c", "d"] {
        g.remove_player(&ctx, &format!("u-{n}"), false, &mut out);
    }
    assert_eq!(g.phase, GamePhase::Waiting);
    assert!(out.events.iter().any(|e| matches!(e, Event::CountdownCancelled { .. })));
}

#[test]
fn round_start_assigns_spawns_and_teams() {
    let (g, _, _) = started("duo", &["a", "b", "c"]);
    assert_eq!(g.teams.len(), 2);
    for p in g.players.values() {
        assert_eq!(p.status, PlayerStatus::Alive);
        assert!(p.spawn.is_some());
    }
}

#[test]
fn respawn_then_final_death_ends_round() {
    let (mut g, cfg, mut now) = started("duo", &["a", "b"]);
    let red = g.players["u-a"].team.clone().unwrap();
    let blue = g.players["u-b"].team.clone().unwrap();
    assert_ne!(red, blue);
    let mut out = Out::default();
    // b hits a, a dies: credited, respawns.
    assert!(g.allow_damage("u-a", Some("u-b"), false, now));
    let ctx = Ctx { now, cfg: &cfg };
    assert!(g.on_death(&ctx, "u-a", "player", &mut out));
    assert_eq!(g.players["u-b"].kills, 1);
    assert_eq!(g.players["u-a"].status, PlayerStatus::Respawning);
    assert!(out.effects.iter().any(|e| matches!(e, Effect::Console { command } if command == "say b killed a")));
    run(&mut g, &cfg, &mut now, 3, &mut out);
    assert_eq!(g.players["u-a"].status, PlayerStatus::Alive);
    // Beds gone: no more respawns for a's team.
    assert!(g.set_respawn(Some(&red), None, false));
    let ctx = Ctx { now, cfg: &cfg };
    g.on_death(&ctx, "u-a", "void", &mut out);
    assert_eq!(g.players["u-a"].status, PlayerStatus::Spectating);
    assert!(matches!(g.phase, GamePhase::Ending { .. }));
    assert_eq!(g.winners, vec![blue]);
    let r = &out.results[0];
    assert!(r.players.iter().any(|p| p.uuid == "u-b" && p.won));
    assert!(r.players.iter().any(|p| p.uuid == "u-a" && !p.won));
    assert!(out.effects.iter().any(|e| matches!(e, Effect::Console { command } if command == "eco give b 10")));
    // Ending → reset.
    run(&mut g, &cfg, &mut now, 3, &mut out);
    assert_eq!(g.phase, GamePhase::Resetting);
    assert!(g.players.is_empty());
    assert_eq!(out.resets, vec!["castle".to_string()]);
}

#[test]
fn damage_rules() {
    let (mut g, _, now) = started("duo", &["a", "b", "c", "d"]);
    let a_team = g.players["u-a"].team.clone();
    let mate = g
        .players
        .values()
        .find(|p| p.uuid != "u-a" && p.team == a_team)
        .unwrap()
        .uuid
        .clone();
    let enemy = g
        .players
        .values()
        .find(|p| p.team != a_team)
        .unwrap()
        .uuid
        .clone();
    assert!(!g.allow_damage("u-a", Some(&mate), false, now), "friendly fire");
    assert!(g.allow_damage("u-a", Some(&enemy), false, now));
    // Inside the no-pvp region.
    g.players.get_mut("u-a").unwrap().regions = vec!["red_base".into()];
    assert!(!g.allow_damage("u-a", Some(&enemy), false, now));
    assert!(g.allow_damage("u-a", None, true, now), "fall damage is on");
    g.rules.set("fall_damage", serde_json::json!(false)).unwrap();
    assert!(!g.allow_damage("u-a", None, true, now));
}

#[test]
fn block_rules_placed_only() {
    let (mut g, _, _) = started("duo", &["a", "b"]);
    let pos = BlockPos::new(0, 70, 0);
    assert!(!g.allow_block("u-a", pos, BlockAction::Break { block: "minecraft:stone" }));
    assert!(g.allow_block("u-a", pos, BlockAction::Place));
    assert!(g.allow_block("u-b", pos, BlockAction::Break { block: "minecraft:white_wool" }));
    assert!(!g.allow_block("u-a", BlockPos::new(500, 70, 0), BlockAction::Place), "outside arena");
    g.rules.breakable = vec!["*_bed".into()];
    assert!(g.allow_block("u-a", BlockPos::new(1, 65, 1), BlockAction::Break { block: "minecraft:red_bed" }));
    assert!(!g.explosion_may_break(BlockPos::new(1, 66, 1)));
}

#[test]
fn void_and_regions_from_positions() {
    let (mut g, cfg, now) = started("duo", &["a", "b"]);
    let ctx = Ctx { now, cfg: &cfg };
    let mut out = Out::default();
    let positions = vec![
        ("u-a".to_string(), "world".to_string(), Location::new(30.0, 65.0, 0.0)),
        ("u-b".to_string(), "world".to_string(), Location::new(0.0, -5.0, 0.0)),
    ];
    g.on_positions(&ctx, &positions, &mut out);
    assert!(out.events.iter().any(|e| matches!(e, Event::RegionEntered { region, .. } if region == "red_base")));
    assert_eq!(g.players["u-b"].deaths, 1, "fell below the arena");
}

#[test]
fn schedule_actions_and_spawners() {
    let (mut g, cfg, mut now) = started("duo", &["a", "b"]);
    let mut out = Out::default();
    for _ in 0..40 {
        now += 1;
        g.spawner_tick(now, true, &mut out);
    }
    assert_eq!(g.spawners[0].stock, 3, "capped");
    let positions = vec![("u-a".to_string(), "world".to_string(), Location::new(30.5, 65.0, 3.0))];
    let picks = g.pickups(&positions, 1.5);
    assert_eq!(picks, vec![(0, "u-a".to_string(), "minecraft:iron_ingot".to_string(), 3)]);
    g.pickup_done(0, 2);
    assert_eq!(g.spawners[0].stock, 1);
    assert_eq!(g.hologram_texts(now)[0].1, "&fIron 1");
    run(&mut g, &cfg, &mut now, 4, &mut out);
    assert!(out.events.iter().any(|e| matches!(e, Event::Timer { id, .. } if id == "diamond_2")));
    assert_eq!(g.groups["iron"].interval, Some(40));
}

#[test]
fn solo_format_last_player_wins() {
    let (mut g, cfg, now) = started("ffa", &["a", "b", "c"]);
    assert_eq!(g.teams.len(), 3);
    assert_eq!(g.players["u-a"].team.as_deref(), Some("a"));
    let mut out = Out::default();
    let ctx = Ctx { now, cfg: &cfg };
    g.on_death(&ctx, "u-a", "void", &mut out);
    assert!(matches!(g.phase, GamePhase::Running { .. }));
    g.remove_player(&ctx, "u-b", true, &mut out);
    assert!(matches!(g.phase, GamePhase::Ending { .. }));
    assert_eq!(g.winners, vec!["c".to_string()]);
    let r = &out.results[0];
    assert!(r.players.iter().any(|p| p.uuid == "u-b" && !p.won), "leaver counted");
}

#[test]
fn manual_win_and_time_up_event() {
    let cfg = cfg();
    let m = mode("win = \"manual\"\ntime_limit = 2\ntime_up = \"event\"");
    let mut g = Game::new(arena(), m, "duo", 1).unwrap();
    let mut now = 0;
    let mut out = Out::default();
    let ctx = Ctx { now, cfg: &cfg };
    g.add_players(&ctx, &party(&["a"]), &mut out).unwrap();
    g.add_players(&ctx, &party(&["b"]), &mut out).unwrap();
    run(&mut g, &cfg, &mut now, 6, &mut out);
    run(&mut g, &cfg, &mut now, 3, &mut out);
    assert!(matches!(g.phase, GamePhase::Running { .. }));
    assert!(out.events.iter().any(|e| matches!(e, Event::TimeUp { .. })));
    let ctx = Ctx { now, cfg: &cfg };
    let red = g.players["u-a"].team.clone().unwrap();
    assert!(g.end_round(&ctx, vec![red.clone(), "nope".into()], "objective", &mut out));
    assert_eq!(g.winners, vec![red]);
}

#[test]
fn sidebar_renders() {
    let (g, cfg, now) = started("duo", &["a", "b"]);
    let (title, lines) = sidebar::render(&g, &cfg, now, "u-a");
    assert_eq!(title, "&e&lbedwars");
    assert!(lines.iter().any(|l| l.starts_with("&7Time: &f00:")));
    assert!(lines.iter().any(|l| l.contains("Diamond II")));
    assert_eq!(lines.iter().filter(|l| l.contains('■')).count(), 2);
}

#[test]
fn abort_resets_without_results() {
    let (mut g, _, _) = started("duo", &["a", "b"]);
    let mut out = Out::default();
    g.abort(&mut out);
    assert_eq!(g.phase, GamePhase::Resetting);
    assert!(out.results.is_empty());
    assert_eq!(out.removed.len(), 2);
}
