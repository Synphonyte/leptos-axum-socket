# Changelog

## [0.8.0] - unreleased

### Breaking Changes
- requires `leptos` `0.8.19` or newer
- requires the `leptos-use` `0.20` micro-crates (`leptos-use-websocket`, `leptos-use-header`)
- `SocketContext::ready_state` is now a `Signal<ConnectionReadyState>`
- `ServerSocket::lock()` is now synchronous
- `upgrade_websocket()` takes the request's `HeaderMap` as second argument for the `Origin` check
- messages from the browser are only broadcast if a send mapper accepts them
- `SocketContext::subscribe()` returns a `SubscriptionHandle` and a key can have several subscribers
- `ServerSocketInner::send_to_self()` is no longer `async`
- subscribe filters and send mappers with the wrong context type deny instead of panicking
- `send_to_self()` reaches every tab of the calling browser
- the client id cookie is `Secure` and `__Host-` prefixed by default
- messages from the browser are limited to 64 KiB by default

### Added
- `SocketOptions` and `ServerSocket::with_options()` for size limits, allowed origins, ping interval, timeouts and a secure client id cookie
- server pings clients and closes unresponsive connections

### Fixes
- malformed messages are dropped instead of panicking the connection
- disconnects no longer leak tasks or connections
- re-subscribing a key no longer spawns a duplicate task or bypasses the subscription limit
- lagging subscribers keep their subscription
- mailboxes without subscribers are removed
- send mappers run without the socket lock
- broadcast messages are serialized only once
- tracing spans no longer record socket state, payloads or headers
- `SocketContext::reconnect()` keeps subscriptions with the components that made them
- client handlers run untracked and new subscribers no longer receive the previous message
- client id cookie is parsed with the `cookie` crate
- docs.rs builds with the `ssr` feature

## [0.7.0] - 2026-07-04

### Breaking Change
- updated dependency `leptos-use` to version `0.19`

## [0.6.0] - 2026-01-13

### Breaking Change
- updated dependency `leptos-use` to version `0.18`

### Special thanks to our sponsor
- @benwis

## [0.5.0] - 2025-12-19

### Breaking Change
- updated dependency `leptos-use` to version `0.17` (thanks to @DrFlowerkick) 

### Fixes
- prevent recreating `SocketContext` in `provide_socket_context()` (thanks to @DrFlowerkick) 
- `SocketContext::unsubscribe()` removes `key_value` from `self.subscribers` (thanks to @DrFlowerkick) 

### Special thanks to our sponsor
- @benwis

## [0.4.1] - 2025-10-26

- Added `ServerSocketInner::send_to_self` to allow sending messages from an axum handler.
- Improved docs

## [0.4.0] - 2025-10-18

- Subscription filters are now `async`
- Added a `send()` method to the `ServerSocket` to allow sending messages outside of server functions

## [0.3.0] - 2025-09-10

- The `provide_socket_context...` methods now return the `SocketContext`.
- Added `send_to_self()` for server functions.
- Instead of calling `ws.on_upgrade` with a `handle_websocket...` function you now call `upgrade_websocket()`.
- `ServerSocket::lock()` and server function `send()` are now async.

## [0.2.1] - 2025-09-09

- `SocketContext::reconnect()` now keeps existing subscriptions

## [0.2.0] - 2025-09-09 (yanked)

- Added proper cleanup of client side connection resources
- Added `provide_socket_context_with_query` to add extra query parameters to the socket URL
- Added method `ServerSocket::lock`
- Replaced `add_permission_filter` with `add_subscribe_filter` and `add_send_mapper` to have more
  fine-grained control over the socket's behavior.
- Added the `chat` example

## [0.1.0] - 2025-09-05

- Initial release
