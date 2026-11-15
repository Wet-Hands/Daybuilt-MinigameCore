//! Slash commands. Pattern: one `CommandHandler` struct per command, registered in `register`.

use pumpkin_plugin_api::{
    Context, Result, Server,
    command::{
        ArgumentType, Command, CommandError, CommandNode, CommandSender, ConsumedArgs, StringType,
    },
    command_wit::Arg,
    commands::CommandHandler,
    permission::{Permission, PermissionDefault},
    text::TextComponent,
};

use crate::ID;

pub fn register(context: &Context) -> Result<()> {
    // /hello [name]  (alias: /hi) — everyone may use it.
    let command = Command::new(&["hello".into(), "hi".into()], "Say hello.")
        .execute(Hello)
        .then(
            CommandNode::argument("name", &ArgumentType::String(StringType::Greedy)).execute(Hello),
        );
    register_command(
        context,
        command,
        "command.hello",
        "Use /hello",
        PermissionDefault::Allow,
    )
}

/// Registers `<ID>:<node>` as a permission, then the command guarded by it.
/// Use `PermissionDefault::Op(PermissionLevel::Two)` for admin-only commands.
fn register_command(
    context: &Context,
    command: Command,
    node: &str,
    description: &str,
    default: PermissionDefault,
) -> Result<()> {
    let full = format!("{ID}:{node}");
    context.register_permission(&Permission {
        node: full.clone(),
        description: description.into(),
        default,
        children: vec![],
    })?;
    context.register_command(command, &full);
    Ok(())
}

struct Hello;

impl CommandHandler for Hello {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        // Optional argument: only present when the `name` branch matched.
        let name = match args.get_value("name") {
            Arg::Simple(s) | Arg::Msg(s) => s,
            _ => sender.get_name(),
        };
        sender.send_message(TextComponent::text(&format!("Hello, {name}!")));
        Ok(1) // success count
    }
}
