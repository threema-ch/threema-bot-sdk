//! Command parsing and handling infrastructure.
//!
//! Provides base command types and parsing for bot commands. Bot implementations define their
//! commands via [`MessageHandler::commands`](crate::server::handler::MessageHandler::commands).
//!
//! ## Command Styles
//!
//! Two command styles are supported:
//!
//! - Slash style: `/remind 30m`
//! - Word style: `start newsletter`
//!
//! The style can be set globally using the [`Commands::style()`] method. By default, slash style is
//! used.
//!
//! ## Command Groups
//!
//! Commands can be organized into groups, which are rendered as separate sections in the
//! auto-generated help text. Groups are identified by a stable ID (used to control visibility),
//! while the title is the display text shown as the section header. A command may be registered
//! in multiple sections; it is then listed in every section where it is visible (see
//! [`Commands::group`]):
//!
//! ```rust
//! use threema_gateway_bot::commands::Commands;
//!
//! let commands = Commands::new()
//!     .register("ping", "Check if bot is alive")
//!     .group("admin", "Admin commands", |group| {
//!         group.register("restart", "Restart the bot")
//!     });
//! ```
//!
//! ## Help Visibility
//!
//! By default, the help text lists all registered commands. To show different help text to
//! different users (e.g. hiding admin commands from regular users), implement
//! [`MessageHandler::help_visibility`](crate::server::handler::MessageHandler::help_visibility)
//! and return a [`HelpVisibility`]. When an ID is used both during registration and for
//! visibility control, prefer a shared constant over repeated string literals:
//!
//! ```rust
//! use threema_gateway_bot::commands::{Commands, HelpVisibility};
//!
//! const GROUP_ADMIN: &str = "admin";
//!
//! let commands = Commands::new()
//!     .register("ping", "Check if bot is alive")
//!     .group(GROUP_ADMIN, "Admin commands", |group| {
//!         group.register("restart", "Restart the bot")
//!     });
//!
//! // In `MessageHandler::help_visibility`:
//! let visibility = HelpVisibility::all().hide_group(GROUP_ADMIN);
//! ```
//!
//! Note that visibility only affects help rendering: Every registered command remains
//! dispatchable by any sender. Access control must be implemented in
//! [`MessageHandler::handle_command`](crate::server::handler::MessageHandler::handle_command).
//!
//! ## As Your Command Set Grows
//!
//! All IDs are accepted as `AsRef<str>`, so instead of repeating string literals, command IDs can
//! also be managed in a dependency-free enum:
//!
//! ```rust
//! #[derive(Debug, Copy, Clone, PartialEq, Eq)]
//! enum Cmd {
//!     Ping,
//!     Restart,
//! }
//!
//! impl Cmd {
//!     const ALL: [Cmd; 2] = [Cmd::Ping, Cmd::Restart];
//!
//!     /// Stable command ID, used for registration and dispatch.
//!     fn id(self) -> &'static str {
//!         match self {
//!             Cmd::Ping => "ping",
//!             Cmd::Restart => "restart",
//!         }
//!     }
//!
//!     fn from_id(id: &str) -> Option<Cmd> {
//!         Self::ALL.into_iter().find(|cmd| cmd.id() == id)
//!     }
//! }
//!
//! impl AsRef<str> for Cmd {
//!     fn as_ref(&self) -> &str {
//!         self.id()
//!     }
//! }
//! ```
//!
//! Register with `.register(Cmd::Ping, "...")` and match exhaustively in
//! [`handle_command`](crate::server::handler::MessageHandler::handle_command) via `Cmd::from_id`.
//! (The [strum](https://crates.io/crates/strum) crate automates these conversions, if you prefer
//! derives.)

use std::{
    collections::{HashMap, HashSet},
    fmt::Write as _,
};

use crate::errors::InitError;

/// Name of the built-in help command. Reserved: it cannot be registered as a custom command.
const HELP_COMMAND_NAME: &str = "help";

/// The style of command syntax used by the bot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandStyle {
    /// Commands start with `/` (e.g. `/help`, `/remind 30`).
    Slash,
    /// Commands start with a word (e.g. `help`, `remind 30`).
    Word,
}

/// Parsed command from user input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ParsedCommand<'cmd> {
    /// Show help text.
    Help,
    /// A registered custom command.
    Registered { name: &'cmd str, args: &'cmd str },
    /// Unknown command (not registered).
    Unknown { name: &'cmd str, args: &'cmd str },
    /// Not a command - regular message.
    None(&'cmd str),
}

/// A custom command definition with its name and description.
struct CustomCommand {
    name: String,
    description: String,
}

/// A group of commands, rendered as a separate section in the help text.
struct CommandGroup {
    id: String,
    title: String,
    commands: Vec<CustomCommand>,
}

/// Builder for the commands of a group, passed to the closure of [`Commands::group`].
pub struct CommandGroupBuilder {
    commands: Vec<CustomCommand>,
}

impl CommandGroupBuilder {
    /// Register a custom command with a name and description, as part of this group.
    ///
    /// See [`Commands::register`] for details.
    #[must_use]
    pub fn register<N: AsRef<str>, D: Into<String>>(mut self, name: N, description: D) -> Self {
        self.commands.push(CustomCommand {
            name: name.as_ref().to_owned(),
            description: description.into(),
        });
        self
    }
}

/// Command configuration for a bot.
///
/// Defines which commands the bot supports and how unknown commands are handled.
/// Returned by [`MessageHandler::commands`](crate::server::handler::MessageHandler::commands).
///
/// # Example
///
/// ```rust
/// # use threema_gateway_bot::commands::{Commands, CommandStyle};
/// let commands = Commands::new()
///     .style(CommandStyle::Slash)
///     .register("remind", "Set a reminder")
///     .register("list", "List your reminders")
///     .handle_unknown(true);
/// ```
pub struct Commands {
    pub(crate) style: CommandStyle,
    registered: Vec<CustomCommand>,
    groups: Vec<CommandGroup>,
    pub(crate) handle_unknown: bool,
}

impl Commands {
    /// Create an empty command configuration.
    ///
    /// Base commands (like `/help`) are always included.
    /// The default command style is [`CommandStyle::Slash`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            style: CommandStyle::Slash,
            registered: Vec::new(),
            groups: Vec::new(),
            handle_unknown: false,
        }
    }

    /// Set the command style.
    #[must_use]
    pub fn style(mut self, style: CommandStyle) -> Self {
        self.style = style;
        self
    }

    /// Register a custom command with a name and description.
    ///
    /// Registered commands are dispatched to
    /// [`MessageHandler::handle_command`](crate::server::handler::MessageHandler::handle_command)
    /// and included in the auto-generated help text.
    ///
    /// The name `help` is reserved for the built-in help command and is rejected by
    /// [`BotServer::new`](crate::server::BotServer::new).
    #[must_use]
    pub fn register<N: AsRef<str>, D: Into<String>>(mut self, name: N, description: D) -> Self {
        self.registered.push(CustomCommand {
            name: name.as_ref().to_owned(),
            description: description.into(),
        });
        self
    }

    /// Register a group of commands, rendered as a separate section in the help text.
    ///
    /// The `id` is a stable identifier used to control visibility (see
    /// [`HelpVisibility`]), while the `title` is the display text shown as the
    /// section header. Group IDs must be unique.
    ///
    /// Commands registered within the group behave exactly like commands registered via
    /// [`register`](Self::register): they are dispatched to
    /// [`MessageHandler::handle_command`](crate::server::handler::MessageHandler::handle_command).
    ///
    /// # Multiple Sections
    ///
    /// A command name may be registered in multiple sections (several groups, or a group and
    /// the ungrouped commands). It is then listed in every section where it is visible, with
    /// visibility resolved per section: e.g. a command registered in both `monitoring` and
    /// `admin` still shows under *Monitoring* when the `admin` group is hidden. Dispatching is
    /// by name and unaffected.
    ///
    /// Each registration carries its own description, allowing context-specific phrasing per
    /// section. If the description should stay identical everywhere, use a shared constant.
    ///
    /// Registering the same command name twice *within* one section (or reusing a group ID) is
    /// always a mistake and rejected by
    /// [`BotServer::new`](crate::server::BotServer::new).
    ///
    /// # Example
    ///
    /// ```rust
    /// # use threema_gateway_bot::commands::Commands;
    /// let commands = Commands::new()
    ///     .register("ping", "Check if bot is alive")
    ///     .group("admin", "Admin commands", |group| {
    ///         group
    ///             .register("restart", "Restart the bot")
    ///             .register("kick", "Remove a user")
    ///     });
    /// ```
    #[must_use]
    pub fn group<I, T, F>(mut self, id: I, title: T, build: F) -> Self
    where
        I: AsRef<str>,
        T: Into<String>,
        F: FnOnce(CommandGroupBuilder) -> CommandGroupBuilder,
    {
        let builder = build(CommandGroupBuilder {
            commands: Vec::new(),
        });
        self.groups.push(CommandGroup {
            id: id.as_ref().to_owned(),
            title: title.into(),
            commands: builder.commands,
        });
        self
    }

    /// Enable dispatching unknown commands to the handler.
    ///
    /// By default, unknown commands (not registered via [`register`](Self::register)) auto-respond
    /// with help text. When enabled, unknown commands are dispatched to
    /// [`MessageHandler::handle_command`](crate::server::handler::MessageHandler::handle_command).
    ///
    /// Note: This will only work for [`CommandStyle::Slash`], since word style command parsing
    /// cannot differentiate between unknown commands and plain text.
    #[must_use]
    pub fn handle_unknown(mut self, enabled: bool) -> Self {
        self.handle_unknown = enabled;
        self
    }
}

impl Default for Commands {
    fn default() -> Self {
        Self::new()
    }
}

/// Which commands and groups are visible in the rendered help text.
///
/// Constructed in
/// [`MessageHandler::help_visibility`](crate::server::handler::MessageHandler::help_visibility)
/// per incoming message. Resolution is most-specific-wins: a command-level override beats a
/// group-level override, which beats the base visibility. Within the same level, the last
/// builder call wins. The built-in `help` command is always visible; overrides referencing it
/// have no effect.
///
/// Note: Visibility only affects help rendering. Every registered command remains dispatchable
/// by any sender; access control is the handler's responsibility in
/// [`handle_command`](crate::server::handler::MessageHandler::handle_command).
///
/// # Example
///
/// ```rust
/// use threema_gateway_bot::commands::HelpVisibility;
///
/// const GROUP_ADMIN: &str = "admin";
///
/// let visibility = HelpVisibility::all()
///     .hide_group(GROUP_ADMIN)
///     .show_command("restart");
///
/// assert!(visibility.is_command_visible("ping", None));
/// assert!(!visibility.is_command_visible("kick", Some(GROUP_ADMIN)));
/// assert!(visibility.is_command_visible("restart", Some(GROUP_ADMIN)));
/// ```
#[derive(Debug, Clone)]
pub struct HelpVisibility {
    /// Fallback for entries with no more specific override.
    base: bool,
    /// Per-group overrides, keyed by group ID.
    groups: HashMap<String, bool>,
    /// Per-command overrides, keyed by command ID.
    commands: HashMap<String, bool>,
}

impl HelpVisibility {
    /// Everything visible (the default).
    #[must_use]
    pub fn all() -> Self {
        Self {
            base: true,
            groups: HashMap::new(),
            commands: HashMap::new(),
        }
    }

    /// Nothing visible except the built-in `help` command.
    #[must_use]
    pub fn none() -> Self {
        Self {
            base: false,
            groups: HashMap::new(),
            commands: HashMap::new(),
        }
    }

    /// Show a group and its commands (unless overridden per command).
    #[must_use]
    pub fn show_group<I: AsRef<str>>(mut self, id: I) -> Self {
        self.groups.insert(id.as_ref().to_owned(), true);
        self
    }

    /// Hide a group and its commands (unless overridden per command).
    #[must_use]
    pub fn hide_group<I: AsRef<str>>(mut self, id: I) -> Self {
        self.groups.insert(id.as_ref().to_owned(), false);
        self
    }

    /// Show a single command, regardless of its group's visibility.
    #[must_use]
    pub fn show_command<I: AsRef<str>>(mut self, id: I) -> Self {
        self.commands.insert(id.as_ref().to_owned(), true);
        self
    }

    /// Hide a single command, regardless of its group's visibility.
    #[must_use]
    pub fn hide_command<I: AsRef<str>>(mut self, id: I) -> Self {
        self.commands.insert(id.as_ref().to_owned(), false);
        self
    }

    /// Resolve visibility for a command, given the ID of the group it belongs to (if any).
    #[must_use]
    pub fn is_command_visible(&self, command_id: &str, group_id: Option<&str>) -> bool {
        if let Some(&visible) = self.commands.get(command_id) {
            return visible;
        }
        if let Some(group_id) = group_id
            && let Some(&visible) = self.groups.get(group_id)
        {
            return visible;
        }
        self.base
    }
}

impl Default for HelpVisibility {
    fn default() -> Self {
        Self::all()
    }
}

/// Internal registry used by the server for parsing and help text generation.
pub(crate) struct CommandRegistry {
    description: Option<String>,
    commands: Commands,
}

impl CommandRegistry {
    /// Build a registry from a description and command configuration.
    ///
    /// Returns [`InitError::InvalidCommands`] if the same command is registered more than once
    /// within one section (one group or the ungrouped commands), or if two groups share an ID.
    pub(crate) fn new(description: Option<String>, commands: Commands) -> Result<Self, InitError> {
        let registry = Self {
            description,
            commands,
        };
        registry.validate()?;
        Ok(registry)
    }

    /// Whether unknown commands should be dispatched to the handler.
    pub(crate) fn handle_unknown(&self) -> bool {
        self.commands.handle_unknown
    }

    /// Iterate over all registered commands, ungrouped and grouped.
    fn all_commands(&self) -> impl Iterator<Item = &CustomCommand> {
        self.commands.registered.iter().chain(
            self.commands
                .groups
                .iter()
                .flat_map(|group| group.commands.iter()),
        )
    }

    /// Whether a command with this name was registered (in any group or ungrouped).
    fn is_registered(&self, name: &str) -> bool {
        self.all_commands().any(|cmd| cmd.name == name)
    }

    /// Validate the command configuration.
    ///
    /// The same command name may be registered in multiple sections (see [`Commands::group`]),
    /// but not twice within the same section. Group IDs must be unique, and the built-in
    /// `help` command name cannot be registered.
    fn validate(&self) -> Result<(), InitError> {
        fn check_section(commands: &[CustomCommand], section: &str) -> Result<(), InitError> {
            let mut seen = HashSet::new();
            for cmd in commands {
                if cmd.name == HELP_COMMAND_NAME {
                    return Err(InitError::InvalidCommands(format!(
                        "the command name \"{HELP_COMMAND_NAME}\" is reserved for the built-in \
                         help command"
                    )));
                }
                if !seen.insert(cmd.name.as_str()) {
                    return Err(InitError::InvalidCommands(format!(
                        "command \"{}\" registered more than once in {section}",
                        cmd.name
                    )));
                }
            }
            Ok(())
        }

        check_section(&self.commands.registered, "the ungrouped commands")?;
        let mut group_ids = HashSet::new();
        for group in &self.commands.groups {
            if !group_ids.insert(group.id.as_str()) {
                return Err(InitError::InvalidCommands(format!(
                    "group ID \"{}\" used more than once",
                    group.id
                )));
            }
            check_section(&group.commands, &format!("group \"{}\"", group.id))?;
        }
        Ok(())
    }

    /// Log a warning for every visibility override that references an unknown ID.
    fn warn_unknown_visibility_ids(&self, visibility: &HelpVisibility) {
        for group_id in visibility.groups.keys() {
            if !self
                .commands
                .groups
                .iter()
                .any(|group| &group.id == group_id)
            {
                tracing::warn!("Help visibility references unknown group ID: {group_id}");
            }
        }
        for command_id in visibility.commands.keys() {
            if command_id == HELP_COMMAND_NAME {
                tracing::warn!(
                    "Help visibility cannot override the built-in help command (always visible)"
                );
            } else if !self.is_registered(command_id) {
                tracing::warn!("Help visibility references unknown command ID: {command_id}");
            }
        }
    }

    /// Format a command name with the appropriate prefix for the current style.
    pub(crate) fn format_command(&self, name: &str) -> String {
        match self.commands.style {
            CommandStyle::Slash => format!("/{name}"),
            CommandStyle::Word => name.to_owned(),
        }
    }

    /// Parse a text message into a command.
    ///
    /// Commands are case-sensitive.
    pub(crate) fn parse<'cmd>(&self, text: &'cmd str) -> ParsedCommand<'cmd> {
        let trimmed = text.trim();

        match self.commands.style {
            CommandStyle::Slash => {
                let Some(rest) = trimmed.strip_prefix('/') else {
                    return ParsedCommand::None(trimmed);
                };
                let Some(name) = rest.split_whitespace().next() else {
                    // Bare `/` with no command name
                    return ParsedCommand::None(trimmed);
                };
                let args = rest.strip_prefix(name).map_or("", str::trim);

                if name == HELP_COMMAND_NAME {
                    ParsedCommand::Help
                } else if self.is_registered(name) {
                    ParsedCommand::Registered { name, args }
                } else {
                    ParsedCommand::Unknown { name, args }
                }
            }
            CommandStyle::Word => {
                let Some(name) = trimmed.split_whitespace().next() else {
                    return ParsedCommand::None(trimmed);
                };
                let args = trimmed.strip_prefix(name).map_or("", str::trim);

                if name == HELP_COMMAND_NAME {
                    ParsedCommand::Help
                } else if self.is_registered(name) {
                    ParsedCommand::Registered { name, args }
                } else {
                    // Note: In case of "Word" style (with no command prefix) we cannot
                    // differentiate unknown commands from plain text, so we always return
                    // `ParsedCommand::None`.
                    ParsedCommand::None(trimmed)
                }
            }
        }
    }

    /// Generate help text, optionally preceded by a message.
    pub(crate) fn help_text_with_prelude(
        &self,
        prelude: Option<&str>,
        visibility: &HelpVisibility,
    ) -> String {
        let help = self.help_text(visibility);
        match prelude {
            Some(prelude) => format!("{prelude}\n\n---\n\n{help}"),
            None => help,
        }
    }

    /// Generate help text from the registered commands, filtered by `visibility`.
    pub(crate) fn help_text(&self, visibility: &HelpVisibility) -> String {
        self.warn_unknown_visibility_ids(visibility);

        let mut text = String::new();
        let prefix = match self.commands.style {
            CommandStyle::Slash => "/",
            CommandStyle::Word => "",
        };

        if let Some(description) = &self.description {
            writeln!(text, "{description}\n").expect("write to String");
        }

        writeln!(text, "Available Commands:\n").expect("write to String");
        writeln!(text, "{prefix}{HELP_COMMAND_NAME} - Show this help message")
            .expect("write to String");
        for cmd in &self.commands.registered {
            if visibility.is_command_visible(&cmd.name, None) {
                writeln!(text, "{prefix}{} - {}", cmd.name, cmd.description)
                    .expect("write to String");
            }
        }

        for group in &self.commands.groups {
            let visible_commands = group
                .commands
                .iter()
                .filter(|cmd| visibility.is_command_visible(&cmd.name, Some(&group.id)))
                .collect::<Vec<_>>();
            // A group header is only rendered if the group has at least one visible command
            if visible_commands.is_empty() {
                continue;
            }
            writeln!(text, "\n*{}:*\n", group.title).expect("write to String");
            for cmd in visible_commands {
                writeln!(text, "{prefix}{} - {}", cmd.name, cmd.description)
                    .expect("write to String");
            }
        }

        text.truncate(text.trim_end().len());
        text
    }
}

/// Default messages for common bot responses.
pub(crate) mod messages {
    /// Access denied for unauthorized users.
    pub(crate) const ACCESS_DENIED: &str = "Sorry, you are not authorized to use this service. Please contact the administrator if you believe this is an error.";

    /// Generic error message.
    pub(crate) const GENERIC_ERROR: &str =
        "Sorry, I encountered an error processing your request. Please try again.";
}

#[cfg(test)]
mod tests {
    use super::*;

    mod parse_slash_style {
        use super::*;

        fn registry() -> CommandRegistry {
            let commands = Commands::new()
                .style(CommandStyle::Slash)
                .register("remind", "Set a reminder")
                .register("list", "List your reminders");
            CommandRegistry::new(None, commands).expect("valid commands")
        }

        #[test]
        fn help_command() {
            let reg = registry();
            assert_eq!(reg.parse("/help"), ParsedCommand::Help);
            assert_eq!(reg.parse("  /help  "), ParsedCommand::Help);
        }

        #[test]
        fn registered_command() {
            let reg = registry();
            assert_eq!(
                reg.parse("/remind 30 Take a break"),
                ParsedCommand::Registered {
                    name: "remind",
                    args: "30 Take a break",
                }
            );
            assert_eq!(
                reg.parse("/list"),
                ParsedCommand::Registered {
                    name: "list",
                    args: "",
                }
            );
        }

        #[test]
        fn grouped_command() {
            let commands = Commands::new().register("remind", "Set a reminder").group(
                "admin",
                "Admin commands",
                |group| group.register("restart", "Restart the bot"),
            );
            let reg = CommandRegistry::new(None, commands).expect("valid commands");
            assert_eq!(
                reg.parse("/restart now"),
                ParsedCommand::Registered {
                    name: "restart",
                    args: "now",
                }
            );
        }

        #[test]
        fn unknown_command() {
            let reg = registry();
            assert_eq!(
                reg.parse("/foo bar"),
                ParsedCommand::Unknown {
                    name: "foo",
                    args: "bar",
                }
            );
            assert_eq!(
                reg.parse("/unknown"),
                ParsedCommand::Unknown {
                    name: "unknown",
                    args: "",
                }
            );
        }

        #[test]
        fn regular_message() {
            let reg = registry();
            assert_eq!(reg.parse("Hello"), ParsedCommand::None("Hello"));
            assert_eq!(reg.parse("  Hello  "), ParsedCommand::None("Hello"));
        }

        #[test]
        fn bare_slash() {
            let reg = registry();
            assert_eq!(reg.parse("/"), ParsedCommand::None("/"));
            assert_eq!(reg.parse("  /  "), ParsedCommand::None("/"));
        }
    }

    mod parse_word_style {
        use super::*;

        fn registry() -> CommandRegistry {
            let commands = Commands::new()
                .style(CommandStyle::Word)
                .register("remind", "Set a reminder")
                .register("list", "List your reminders");
            CommandRegistry::new(None, commands).expect("valid commands")
        }

        #[test]
        fn help_command() {
            let reg = registry();
            assert_eq!(reg.parse("help"), ParsedCommand::Help);
            assert_eq!(reg.parse("  help  "), ParsedCommand::Help);
        }

        #[test]
        fn registered_command() {
            let reg = registry();
            assert_eq!(
                reg.parse("remind 30 Take a break"),
                ParsedCommand::Registered {
                    name: "remind",
                    args: "30 Take a break",
                }
            );
            assert_eq!(
                reg.parse("list"),
                ParsedCommand::Registered {
                    name: "list",
                    args: "",
                }
            );
        }

        #[test]
        fn regular_message() {
            let reg = registry();
            assert_eq!(reg.parse("Hello world"), ParsedCommand::None("Hello world"));
            assert_eq!(reg.parse("  Hello  "), ParsedCommand::None("Hello"));
        }

        #[test]
        fn empty_message() {
            let reg = registry();
            assert_eq!(reg.parse(""), ParsedCommand::None(""));
            assert_eq!(reg.parse("   "), ParsedCommand::None(""));
        }
    }

    mod command_registry_new {
        use super::*;

        /// Build the registry and return the expected validation error.
        fn registry_err(commands: Commands) -> InitError {
            CommandRegistry::new(None, commands)
                .err()
                .expect("invalid commands must be rejected")
        }

        #[test]
        fn duplicate_in_ungrouped_section_rejected() {
            let commands = Commands::new()
                .register("echo", "Echo back text")
                .register("echo", "Echo back text again");
            insta::assert_snapshot!(
                registry_err(commands),
                @r#"invalid command registration: command "echo" registered more than once in the ungrouped commands"#
            );
        }

        #[test]
        fn duplicate_in_group_rejected() {
            let commands = Commands::new().group("admin", "Admin commands", |group| {
                group
                    .register("restart", "Restart the bot")
                    .register("restart", "Restart the bot again")
            });
            insta::assert_snapshot!(
                registry_err(commands),
                @r#"invalid command registration: command "restart" registered more than once in group "admin""#
            );
        }

        #[test]
        fn duplicate_group_id_rejected() {
            let commands = Commands::new()
                .group("admin", "Admin commands", |group| {
                    group.register("restart", "Restart the bot")
                })
                .group("admin", "More admin commands", |group| {
                    group.register("kick", "Remove a user")
                });
            insta::assert_snapshot!(
                registry_err(commands),
                @r#"invalid command registration: group ID "admin" used more than once"#
            );
        }

        #[test]
        fn reserved_help_name_rejected() {
            let commands = Commands::new().register("help", "Custom help");
            insta::assert_snapshot!(
                registry_err(commands),
                @r#"invalid command registration: the command name "help" is reserved for the built-in help command"#
            );

            let commands = Commands::new().group("admin", "Admin commands", |group| {
                group.register("help", "Custom help")
            });
            insta::assert_snapshot!(
                registry_err(commands),
                @r#"invalid command registration: the command name "help" is reserved for the built-in help command"#
            );
        }

        #[test]
        fn same_command_across_sections_allowed() {
            let commands = Commands::new()
                .register("status", "Show system status")
                .group("monitoring", "Monitoring", |group| {
                    group.register("status", "Show system status")
                })
                .group("admin", "Admin commands", |group| {
                    group.register("status", "Show system status")
                });
            assert!(CommandRegistry::new(None, commands).is_ok());
        }
    }

    mod help_text {
        use super::*;

        /// A registry with ungrouped and grouped commands.
        fn grouped_registry(style: CommandStyle) -> CommandRegistry {
            let commands = Commands::new()
                .style(style)
                .register("echo", "Echo back text")
                .register("ping", "Check if bot is alive")
                .group("admin", "Admin commands", |group| {
                    group
                        .register("restart", "Restart the bot")
                        .register("kick", "Remove a user")
                });
            CommandRegistry::new(None, commands).expect("valid commands")
        }

        #[test]
        fn without_description() {
            let reg = CommandRegistry::new(None, Commands::new()).expect("valid commands");
            insta::assert_snapshot!(reg.help_text(&HelpVisibility::all()));
        }

        #[test]
        fn with_description() {
            let reg = CommandRegistry::new(Some("My cool bot.".into()), Commands::new())
                .expect("valid commands");
            insta::assert_snapshot!(reg.help_text(&HelpVisibility::all()));
        }

        #[test]
        fn with_custom_commands() {
            let commands = Commands::new()
                .register("remind", "Set a reminder")
                .register("list", "List your reminders");
            let reg = CommandRegistry::new(None, commands).expect("valid commands");
            insta::assert_snapshot!(reg.help_text(&HelpVisibility::all()));
        }

        #[test]
        fn with_prelude() {
            let reg = CommandRegistry::new(None, Commands::new()).expect("valid commands");
            insta::assert_snapshot!(
                reg.help_text_with_prelude(Some("Unknown command: /foo"), &HelpVisibility::all())
            );
        }

        #[test]
        fn word_style() {
            let commands = Commands::new()
                .style(CommandStyle::Word)
                .register("remind", "Set a reminder")
                .register("list", "List your reminders");
            let reg = CommandRegistry::new(None, commands).expect("valid commands");
            insta::assert_snapshot!(reg.help_text(&HelpVisibility::all()));
        }

        #[test]
        fn with_groups() {
            let reg = grouped_registry(CommandStyle::Slash);
            insta::assert_snapshot!(reg.help_text(&HelpVisibility::all()));
        }

        #[test]
        fn with_groups_word_style() {
            let reg = grouped_registry(CommandStyle::Word);
            insta::assert_snapshot!(reg.help_text(&HelpVisibility::all()));
        }

        #[test]
        fn with_hidden_group() {
            let reg = grouped_registry(CommandStyle::Slash);
            insta::assert_snapshot!(reg.help_text(&HelpVisibility::all().hide_group("admin")));
        }

        #[test]
        fn with_hidden_group_and_shown_command() {
            let reg = grouped_registry(CommandStyle::Slash);
            insta::assert_snapshot!(
                reg.help_text(
                    &HelpVisibility::all()
                        .hide_group("admin")
                        .show_command("restart")
                )
            );
        }

        #[test]
        fn with_hidden_ungrouped_command() {
            let reg = grouped_registry(CommandStyle::Slash);
            insta::assert_snapshot!(reg.help_text(&HelpVisibility::all().hide_command("ping")));
        }

        #[test]
        fn with_none_visibility() {
            let reg = grouped_registry(CommandStyle::Slash);
            insta::assert_snapshot!(reg.help_text(&HelpVisibility::none()));
        }

        /// A registry where the `status` command is registered in two groups.
        fn shared_command_registry() -> CommandRegistry {
            let commands = Commands::new()
                .group("monitoring", "Monitoring", |group| {
                    group
                        .register("uptime", "Show uptime")
                        .register("status", "Show system status")
                })
                .group("admin", "Admin commands", |group| {
                    group
                        .register("restart", "Restart the bot")
                        .register("status", "Show admin system status")
                });
            CommandRegistry::new(None, commands).expect("valid commands")
        }

        #[test]
        fn with_shared_command() {
            let reg = shared_command_registry();
            insta::assert_snapshot!(reg.help_text(&HelpVisibility::all()));
        }

        #[test]
        fn with_shared_command_hidden_section() {
            let reg = shared_command_registry();
            insta::assert_snapshot!(reg.help_text(&HelpVisibility::all().hide_group("admin")));
        }

        #[test]
        fn with_shared_command_hidden_everywhere() {
            let reg = shared_command_registry();
            insta::assert_snapshot!(reg.help_text(&HelpVisibility::all().hide_command("status")));
        }
    }

    mod help_visibility {
        use super::*;

        #[test]
        fn all_shows_everything() {
            let visibility = HelpVisibility::all();
            assert!(visibility.is_command_visible("echo", None));
            assert!(visibility.is_command_visible("restart", Some("admin")));
        }

        #[test]
        fn none_hides_everything() {
            let visibility = HelpVisibility::none();
            assert!(!visibility.is_command_visible("echo", None));
            assert!(!visibility.is_command_visible("restart", Some("admin")));
        }

        #[test]
        fn group_override_beats_base() {
            let visibility = HelpVisibility::all().hide_group("admin");
            assert!(!visibility.is_command_visible("restart", Some("admin")));
            assert!(visibility.is_command_visible("echo", None));

            let visibility = HelpVisibility::none().show_group("admin");
            assert!(visibility.is_command_visible("restart", Some("admin")));
            assert!(!visibility.is_command_visible("echo", None));
        }

        #[test]
        fn command_override_beats_group() {
            let visibility = HelpVisibility::all()
                .hide_group("admin")
                .show_command("restart");
            assert!(visibility.is_command_visible("restart", Some("admin")));
            assert!(!visibility.is_command_visible("kick", Some("admin")));

            let visibility = HelpVisibility::all().hide_command("kick");
            assert!(!visibility.is_command_visible("kick", Some("admin")));
            assert!(visibility.is_command_visible("restart", Some("admin")));
        }

        #[test]
        fn last_write_wins_within_level() {
            let visibility = HelpVisibility::all()
                .hide_group("admin")
                .show_group("admin");
            assert!(visibility.is_command_visible("restart", Some("admin")));

            let visibility = HelpVisibility::all()
                .show_command("restart")
                .hide_command("restart");
            assert!(!visibility.is_command_visible("restart", Some("admin")));
        }

        #[test]
        fn default_is_all() {
            let visibility = HelpVisibility::default();
            assert!(visibility.is_command_visible("echo", None));
            assert!(visibility.is_command_visible("restart", Some("admin")));
        }
    }

    #[test]
    fn messages_not_empty() {
        assert!(!messages::ACCESS_DENIED.is_empty());
        assert!(!messages::GENERIC_ERROR.is_empty());
    }
}
