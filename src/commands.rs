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

use std::fmt::Write as _;

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
    #[must_use]
    pub fn register<N: Into<String>, D: Into<String>>(mut self, name: N, description: D) -> Self {
        self.registered.push(CustomCommand {
            name: name.into(),
            description: description.into(),
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

/// Internal registry used by the server for parsing and help text generation.
pub(crate) struct CommandRegistry {
    description: Option<String>,
    commands: Commands,
}

impl CommandRegistry {
    /// Build a registry from a description and command configuration.
    pub(crate) fn new(description: Option<String>, commands: Commands) -> Self {
        Self {
            description,
            commands,
        }
    }

    /// Whether unknown commands should be dispatched to the handler.
    pub(crate) fn handle_unknown(&self) -> bool {
        self.commands.handle_unknown
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

                if name == "help" {
                    ParsedCommand::Help
                } else if self.commands.registered.iter().any(|cmd| cmd.name == name) {
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

                if name == "help" {
                    ParsedCommand::Help
                } else if self.commands.registered.iter().any(|cmd| cmd.name == name) {
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
    pub(crate) fn help_text_with_prelude(&self, prelude: Option<&str>) -> String {
        let help = self.help_text();
        match prelude {
            Some(prelude) => format!("{prelude}\n\n---\n\n{help}"),
            None => help,
        }
    }

    /// Generate help text from the registered commands.
    pub(crate) fn help_text(&self) -> String {
        let mut text = String::new();
        let prefix = match self.commands.style {
            CommandStyle::Slash => "/",
            CommandStyle::Word => "",
        };

        if let Some(description) = &self.description {
            writeln!(text, "{description}\n").expect("write to String");
        }

        writeln!(text, "Available Commands:\n").expect("write to String");
        writeln!(text, "{prefix}help - Show this help message").expect("write to String");
        for cmd in &self.commands.registered {
            writeln!(text, "{prefix}{} - {}", cmd.name, cmd.description).expect("write to String");
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
            CommandRegistry::new(None, commands)
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
            CommandRegistry::new(None, commands)
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

    mod help_text {
        use super::*;

        #[test]
        fn without_description() {
            let reg = CommandRegistry::new(None, Commands::new());
            insta::assert_snapshot!(reg.help_text());
        }

        #[test]
        fn with_description() {
            let reg = CommandRegistry::new(Some("My cool bot.".into()), Commands::new());
            insta::assert_snapshot!(reg.help_text());
        }

        #[test]
        fn with_custom_commands() {
            let commands = Commands::new()
                .register("remind", "Set a reminder")
                .register("list", "List your reminders");
            let reg = CommandRegistry::new(None, commands);
            insta::assert_snapshot!(reg.help_text());
        }

        #[test]
        fn with_prelude() {
            let reg = CommandRegistry::new(None, Commands::new());
            insta::assert_snapshot!(reg.help_text_with_prelude(Some("Unknown command: /foo")));
        }

        #[test]
        fn word_style() {
            let commands = Commands::new()
                .style(CommandStyle::Word)
                .register("remind", "Set a reminder")
                .register("list", "List your reminders");
            let reg = CommandRegistry::new(None, commands);
            insta::assert_snapshot!(reg.help_text());
        }
    }

    #[test]
    fn messages_not_empty() {
        assert!(!messages::ACCESS_DENIED.is_empty());
        assert!(!messages::GENERIC_ERROR.is_empty());
    }
}
