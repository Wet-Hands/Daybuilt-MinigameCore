//! Join, quit and chat.

use pumpkin_plugin_api::{
    Context, Result, Server,
    events::{
        EventData, EventHandler, EventPriority, PlayerChatEvent, PlayerJoinEvent, PlayerLeaveEvent,
    },
};

use super::uuid_of;
use crate::game::effects::Effect;
use crate::host;
use crate::state;

pub fn register(context: &Context) -> Result<()> {
    context.register_event_handler::<PlayerJoinEvent, _>(Join, EventPriority::Normal, true)?;
    context.register_event_handler::<PlayerLeaveEvent, _>(Leave, EventPriority::Normal, true)?;
    context.register_event_handler::<PlayerChatEvent, _>(Chat, EventPriority::Normal, true)?;
    Ok(())
}

struct Join;

impl EventHandler<PlayerJoinEvent> for Join {
    fn handle(&self, _server: Server, e: EventData<PlayerJoinEvent>) -> EventData<PlayerJoinEvent> {
        let uuid = uuid_of(&e.player);
        let name = e.player.get_name();
        let id = e.player.as_entity().get_id() as i32;
        state::with(|s| {
            s.player_online(&uuid, &name, id);
            let mut later = Vec::new();
            if s.cfg.lobby.teleport_on_join {
                later.push(Effect::ToLobby {
                    player: uuid.clone(),
                });
            }
            if s.cfg.auto_join().is_some() {
                later.push(Effect::AutoJoin {
                    player: uuid.clone(),
                });
            }
            // The join event fires before the client is fully in; act a few ticks later.
            s.after(5, later);
        });
        e
    }
}

struct Leave;

impl EventHandler<PlayerLeaveEvent> for Leave {
    fn handle(
        &self,
        server: Server,
        e: EventData<PlayerLeaveEvent>,
    ) -> EventData<PlayerLeaveEvent> {
        let uuid = uuid_of(&e.player);
        let effects = state::with(|s| {
            let effects = s.leave(&uuid, true);
            s.player_offline(&uuid);
            effects
        });
        host::sidebar::forget(&uuid);
        host::apply(&server, effects);
        e
    }
}

struct Chat;

impl EventHandler<PlayerChatEvent> for Chat {
    fn handle(
        &self,
        _server: Server,
        mut e: EventData<PlayerChatEvent>,
    ) -> EventData<PlayerChatEvent> {
        let sender = uuid_of(&e.player);
        let recipients: Vec<String> = e.recipients.iter().map(uuid_of).collect();
        // Who may hear the sender: same game (if that game isolates chat), or for lobby
        // players everyone not in an isolating game.
        let keep: Option<Vec<bool>> = state::with(|s| {
            let isolating = |u: &str| {
                s.game_of(u)
                    .filter(|g| g.rules.isolate_chat)
                    .map(|g| g.id.clone())
            };
            let mine = isolating(&sender);
            let any_isolating = !s.index.is_empty();
            if mine.is_none() && !any_isolating {
                return None;
            }
            Some(recipients.iter().map(|r| isolating(r) == mine).collect())
        });
        if let Some(keep) = keep {
            let all = std::mem::take(&mut e.recipients);
            e.recipients = all
                .into_iter()
                .zip(keep)
                .filter(|(_, k)| *k)
                .map(|(p, _)| p)
                .collect();
        }
        e
    }
}
