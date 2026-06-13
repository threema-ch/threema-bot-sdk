# Threema Bot SDK

[![GitHub CI][github-actions-badge]][github-actions]
[![Crates.io Version][crates-io-badge]][crates-io]
[![Crates.io Downloads][crates-io-download-badge]][crates-io-download]

A library for building Threema Gateway bots in Rust.

This library provides the foundational components for building Threema bots
with:

- **Webhook handling** to receive and validate Threema Gateway messages
- **Configuration system** based on TOML files and env vars, extensible by your bot
- **Rate limiting** and **caching** built-in
- **Command parsing** infrastructure with grouped, per-user help text

The command parsing infrastructure allows for both slash-command style (`/remind 30m`) or
word-command style (`remind 30m`).

Documentation: [docs.rs/threema-gateway-bot/](https://docs.rs/threema-gateway-bot/)

## Quick Start

```rust
use std::path::Path;
use threema_gateway_bot::{
    config::BotConfig,
    server::{
        BotServer,
        handler::{Action, HandlerResult, MessageContext, MessageHandler, Response, TypingHandle},
    },
};

// Create a handler struct
struct MyHandler;

// Implement `MessageHandler` trait for your struct
#[async_trait::async_trait]
impl MessageHandler for MyHandler {
    async fn handle_text(
        &self,
        _ctx: &MessageContext,
        text: &str,
        typing: &TypingHandle,
    ) -> HandlerResult<Action> {
        let text_response = Response::text(format!("You said: {}", text));
        Ok(Action::Respond(vec![text_response]))
    }
}

// Start bot server
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = BotConfig::load_with_prefix("MYBOT", Path::new("config.toml"))?;
    BotServer::new(config, MyHandler)?.run().await?;
    Ok(())
}
```

## References

- [Threema Gateway Documentation](https://gateway.threema.ch/en/developer/api)
- [threema-gateway crate](https://crates.io/crates/threema-gateway)

## Rust Version Requirements (MSRV)

This library generally tracks the latest stable Rust version but tries to
guarantee backwards compatibility with older stable versions as much as
possible. However, in many cases transitive dependencies make guaranteeing a
minimal supported Rust version impossible (see [this
discussion](https://users.rust-lang.org/t/rust-version-requirement-change-as-semver-breaking-or-not/20980/25)).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  http://opensource.org/licenses/MIT) at your option.

### Contributing

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.

<!-- Badges -->

[github-actions]: https://github.com/threema-ch/threema-bot-sdk/actions?query=branch%3Amain
[github-actions-badge]: https://github.com/threema-ch/threema-bot-sdk/workflows/CI/badge.svg
[crates-io]: https://crates.io/crates/threema-gateway-bot
[crates-io-badge]: https://img.shields.io/crates/v/threema-gateway-bot.svg?maxAge=3600
[crates-io-download]: https://crates.io/crates/threema-gateway-bot
[crates-io-download-badge]: https://img.shields.io/crates/d/threema-gateway-bot.svg?maxAge=3600
