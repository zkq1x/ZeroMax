# ZeroMax

Privacy-focused client for the [MAX](https://max.ru) messenger. Zero telemetry. Zero trackers.

## Architecture

```
zeromax-core/    Rust library — MAX protocol (WebSocket, auth, chats, messages, events)
zeromax-ffi/     UniFFI bridge — generates Swift/Kotlin bindings from Rust
apps/macos/      macOS client — SwiftUI app using zeromax-ffi
```

## Building

### Requirements

- Rust 1.75+ (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
- Xcode 15+ (for macOS app)
- aarch64-apple-darwin target (`rustup target add aarch64-apple-darwin`)

### Rust core

```bash
cargo build -p zeromax-core
cargo clippy -p zeromax-core
```

### FFI + Swift bindings

```bash
# Build FFI crate and generate Swift bindings + xcframework
cd apps/macos
./build-rust.sh          # debug
./build-rust.sh --release  # release
```

### macOS app

Open `apps/macos/ZeroMax/ZeroMax.xcodeproj` in Xcode and build.

## Protocol

ZeroMax implements the MAX messenger protocol reverse-engineered from the web client:

- **Transport**: WebSocket (`wss://ws-api.oneme.ru/websocket`)
- **Serialization**: JSON over WebSocket (MessagePack + LZ4 for binary socket — planned)
- **Auth**: Phone + SMS code, QR code, 2FA password
- **Features**: Messages (send/edit/delete/pin), reactions, chat history, groups, channels, contacts, file uploads, folders

## License

MIT
