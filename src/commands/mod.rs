//! Commands. `/mg` for players, `/mga` for admins (and for console-driven integrations
//! such as NPCs or menus: `/mga join <player> <mode>`).
//!
//! Never use `CommandNode::suggest` (server-side tab completion panics Pumpkin 0.2.2).

mod admin;
mod player;

use pumpkin_plugin_api::{
    Context, Result,
    command::{Command, CommandError, CommandSender, ConsumedArgs},
    command_wit::{Arg, Number},
    permission::{Permission, PermissionDefault, PermissionLevel},
    text::TextComponent,
};

use crate::host::colored;
use crate::state;

pub fn register(context: &Context) -> Result<()> {
    perm(
        context,
        "build",
        "Change blocks in the lobby and in arenas",
        PermissionDefault::Op(PermissionLevel::Two),
    )?;
    perm(
        context,
        "kit.all",
        "Use any kit",
        PermissionDefault::Op(PermissionLevel::Two),
    )?;
    register_command(
        context,
        player::command(),
        "command.mg",
        "Use /mg",
        PermissionDefault::Allow,
    )?;
    register_command(
        context,
        admin::command(),
        "command.mga",
        "Use /mga (arena setup and game control)",
        PermissionDefault::Op(PermissionLevel::Two),
    )?;
    Ok(())
}

fn perm(
    context: &Context,
    node: &str,
    description: &str,
    default: PermissionDefault,
) -> Result<()> {
    context.register_permission(&Permission {
        node: format!("{}:{node}", crate::ID),
        description: description.into(),
        default,
        children: vec![],
    })
}

fn register_command(
    context: &Context,
    command: Command,
    node: &str,
    description: &str,
    default: PermissionDefault,
) -> Result<()> {
    let full = format!("{}:{node}", crate::ID);
    perm(context, node, description, default)?;
    context.register_command(command, &full);
    Ok(())
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

/// Reloads all files (used at load and by `/mga reload`).
pub fn reload_all() -> String {
    admin::reload()
}
