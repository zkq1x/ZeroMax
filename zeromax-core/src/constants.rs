use std::time::Duration;

/// WebSocket endpoint for the MAX messenger API.
pub const WEBSOCKET_URI: &str = "wss://ws-api.oneme.ru/websocket";

/// Origin header sent during WebSocket handshake.
pub const WEBSOCKET_ORIGIN: &str = "https://web.max.ru";

/// TCP socket API host (for future binary transport).
pub const API_HOST: &str = "api.oneme.ru";

/// TCP socket API port.
pub const API_PORT: u16 = 443;

/// Default timeout for send-and-wait operations.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);

/// Interval between keepalive pings.
pub const DEFAULT_PING_INTERVAL: Duration = Duration::from_secs(30);

/// Backoff delay when recv loop encounters an error.
pub const RECV_LOOP_BACKOFF: Duration = Duration::from_millis(500);

/// Protocol version used in wire frames.
pub const PROTOCOL_VERSION: u8 = 11;

/// Default app version string sent in user agent.
pub const DEFAULT_APP_VERSION: &str = "25.12.14";

/// Default build number sent in user agent.
pub const DEFAULT_BUILD_NUMBER: u32 = 0x97CB;

/// Default locale.
pub const DEFAULT_LOCALE: &str = "ru";

/// Default SQLite database filename for session storage.
pub const SESSION_DB_NAME: &str = "session.db";

/// Phone number validation pattern.
pub const PHONE_REGEX: &str = r"^\+?\d{10,15}$";

/// Default number of chats to request during initial sync.
pub const DEFAULT_SYNC_CHATS_COUNT: u32 = 40;

/// Circuit breaker: error threshold before activation.
pub const CIRCUIT_BREAKER_THRESHOLD: u32 = 10;

/// Circuit breaker: cooldown duration after activation.
pub const CIRCUIT_BREAKER_COOLDOWN: Duration = Duration::from_secs(60);

/// Maximum retries for queued outgoing messages.
pub const DEFAULT_MAX_RETRIES: u32 = 3;

/// Device names used to randomize user agent.
pub const DEVICE_NAMES: &[&str] = &[
    "Chrome",
    "Firefox",
    "Edge",
    "Safari",
    "Opera",
    "Vivaldi",
    "Brave",
    "Chromium",
    "Windows 10",
    "Windows 11",
    "macOS Big Sur",
    "macOS Monterey",
    "macOS Ventura",
    "Ubuntu 20.04",
    "Ubuntu 22.04",
    "Fedora 35",
    "Fedora 36",
    "Debian 11",
];

/// Screen sizes used to randomize user agent.
pub const SCREEN_SIZES: &[&str] = &[
    "1920x1080 1.0x",
    "1366x768 1.0x",
    "1440x900 1.0x",
    "1536x864 1.0x",
    "1280x720 1.0x",
    "1600x900 1.0x",
    "1680x1050 1.0x",
    "2560x1440 1.0x",
    "3840x2160 1.0x",
];

/// OS versions used to randomize user agent.
pub const OS_VERSIONS: &[&str] = &[
    "Windows 10",
    "Windows 11",
    "macOS Big Sur",
    "macOS Monterey",
    "macOS Ventura",
    "Ubuntu 20.04",
    "Ubuntu 22.04",
    "Fedora 35",
    "Fedora 36",
    "Debian 11",
];

/// Timezones used to randomize user agent.
pub const TIMEZONES: &[&str] = &[
    "Europe/Moscow",
    "Europe/Kaliningrad",
    "Europe/Samara",
    "Asia/Yekaterinburg",
    "Asia/Omsk",
    "Asia/Krasnoyarsk",
    "Asia/Irkutsk",
    "Asia/Yakutsk",
    "Asia/Vladivostok",
    "Asia/Kamchatka",
];
