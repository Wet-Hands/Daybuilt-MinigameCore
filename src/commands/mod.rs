//! Commands. `/mg` for players, `/dbmg` for admins (and for console-driven integrations
//! such as NPCs or menus: `/dbmg join <player> <mode>`).
//!
//! Never use `CommandNode::suggest` (server-side tab completion panics Pumpkin 0.2.2).

mod admin;
mod player;

use pumpkin_plugin_api::{
    Context,
    command::{Command, CommandError, CommandSender, ConsumedArgs},
    command_wit::{Arg, Number},
    permission::{Permission, PermissionDefault, PermissionLevel},
    text::TextComponent,
};

use crate::host::colored;
use crate::state;

/// Registers permissions and commands. Each step is independent: a failed permission
/// registration is logged and never stops the commands from being registered.
pub fn register(context: &Context) {
    perm(
        context,
        "build",
        "Change blocks in the lobby and in arenas",
        PermissionDefault::Op(PermissionLevel::Two),
    );
    perm(
        context,
        "kit.all",
        "Use any kit",
        PermissionDefault::Op(PermissionLevel::Two),
    );
    register_command(
        context,
        player::command(),
        "command.mg",
        "Use /mg",
        PermissionDefault::Allow,
    );
    register_command(
        context,
        admin::command(),
        "command.dbmg",
        "Use /dbmg (arena setup and game control)",
        PermissionDefault::Op(PermissionLevel::Two),
    );
}

fn perm(context: &Context, node: &str, description: &str, default: PermissionDefault) {
    let full = format!("{}:{node}", crate::ID);
    if let Err(e) = context.register_permission(&Permission {
        node: full.clone(),
        description: description.into(),
        default,
        children: vec![],
    }) {
        tracing::warn!("{}: could not register permission {full}: {e}", crate::ID);
    }
}

fn register_command(
    context: &Context,
    command: Command,
    node: &str,
    description: &str,
    default: PermissionDefault,
) {
    let full = format!("{}:{node}", crate::ID);
    perm(context, node, description, default);
    context.register_command(command, &full);
}

pub fn word(args: &ConsumedArgs, key: &str) -> Option<String> {
    match args.get_value(key) {
        Arg::Simple(s) | Arg::Msg(s) => Some(s),
        _ => None,
    }
}

pub fn int(args: &ConsumedArgs, key: &str) -> Option<i32> {
    match args.get_value(key) {
        Arg::Num(Ok(Number::Int32(n))) => Some(n),
        _ => None,
    }
}

pub fn boolean(args: &ConsumedArgs, key: &str) -> Option<bool> {
    match args.get_value(key) {
        Arg::Bool(b) => Some(b),
        _ => None,
    }
}

/// Sends `&`-coloured text with the configured prefix.
pub fn reply(sender: &CommandSender, text: &str) {
    let prefix = state::with(|s| s.cfg.messages.prefix.clone());
    sender.send_message(colored(&format!("{prefix}{text}")));
}

/// Sends lines without the prefix.
pub fn lines(sender: &CommandSender, lines: &[String]) {
    for l in lines {
        sender.send_message(colored(l));
    }
}

pub fn fail(text: &str) -> CommandError {
    CommandError::CommandFailed(colored(text))
}

pub fn need_player(
    sender: &CommandSender,
) -> std::result::Result<pumpkin_plugin_api::player::Player, CommandError> {
    sender.as_player().ok_or_else(|| {
        CommandError::CommandFailed(TextComponent::text("Only players can use this."))
    })
}

/// Reloads all files (used at load and by `/dbmg reload`).
pub fn reload_all() -> String {
    admin::reload()
}
