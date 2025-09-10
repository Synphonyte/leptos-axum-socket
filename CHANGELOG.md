# Changelog

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
