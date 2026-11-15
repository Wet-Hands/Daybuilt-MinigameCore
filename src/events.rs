//! Event listeners. Pattern: one `EventHandler<E>` struct per event, registered in `register`.
//! Return the (possibly modified) event data; set `cancelled = true` to cancel when supported.

use pumpkin_plugin_api::{
    Context, Result, Server,
    common::NamedColor,
    events::{EventData, EventHandler, EventPriority, PlayerJoinEvent},
    text::TextComponent,
};

pub fn register(context: &Context) -> Result<()> {
    // `blocking = true` lets the handler modify/cancel the event before the server acts on it.
    context.register_event_handler::<PlayerJoinEvent, _>(
        WelcomeOnJoin,
        EventPriority::Normal,
        true,
    )?;
    Ok(())
}

struct WelcomeOnJoin;

impl EventHandler<PlayerJoinEvent> for WelcomeOnJoin {
    fn handle(
        &self,
        _server: Server,
        mut event: EventData<PlayerJoinEvent>,
    ) -> EventData<PlayerJoinEvent> {
        let name = event.player.get_name();
        event.join_message =
            TextComponent::text(&format!("{name} joined the game")).color_named(NamedColor::Yellow);
        event
    }
}
