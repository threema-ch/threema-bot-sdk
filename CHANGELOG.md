# Changelog

This project follows semantic versioning.

Possible log types:

- `[added]` for new features.
- `[changed]` for changes in existing functionality.
- `[deprecated]` for once-stable features removed in upcoming releases.
- `[removed]` for deprecated features removed in this release.
- `[fixed]` for any bug fixes.
- `[security]` to invite users to upgrade in case of vulnerabilities.

### Unreleased

- [fixed] List config values (currently `threema.allowed_users`) can now be set through environment
  variables as a comma-separated list, e.g. `PREFIX__THREEMA__ALLOWED_USERS=ECHOECHO,ABCD1234`
- [added] Add `BotConfig::env_source` plus the `BotConfig::ENV_SEPARATOR`,
  `BotConfig::ENV_LIST_SEPARATOR` and `BotConfig::ENV_LIST_KEYS` constants, so bots that extend the
  configuration can reuse the environment variable setup of this crate instead of replicating it

### v0.2.1 (2026-06-16)

- [added] Support command groups and per-message help visibility
- [added] Expose `ThreemaClient` for proactively sending messages outside the webhook flow
  (e.g. in reaction to an external event)
- [changed] `BotServer::new` now rejects invalid command registrations (the same command
  registered twice within one section, duplicate group IDs, or the reserved `help` command name)
- [changed] The built-in `help` command no longer bypasses rate limiting

### v0.2.0 (2026-06-12)

- [added] Add location message support through `MessageHandler::handle_location` (#8)
- [fixed] Fix handling of reactions when `allowed_users` is empty (#10)
- [changed] Include dimensions for image messages (#7)
- [changed] Improve timestamp handling (#9)
- [changed] Bump `threema-gateway` dependency from 0.20 to 0.21

### v0.1.0 (2026-04-24)

- Initial release
