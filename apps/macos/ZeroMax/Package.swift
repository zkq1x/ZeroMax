// swift-tools-version: 5.9

import PackageDescription

// Path to the Rust static library built by build-rust.sh.
let rustLibPath = "../../../target/aarch64-apple-darwin/debug"

let package = Package(
    name: "ZeroMax",
    platforms: [
        .macOS(.v13),
    ],
    targets: [
        // C headers + modulemap for the Rust FFI.
        // Target name must match the module name in the generated Swift bindings.
        .systemLibrary(
            name: "zeromax_ffiFFI",
            path: "Sources/ZeroMaxFFI"
        ),
        // Main macOS app.
        .executableTarget(
            name: "ZeroMax",
            dependencies: ["zeromax_ffiFFI"],
            path: "Sources/ZeroMax",
            linkerSettings: [
                .unsafeFlags([
                    "-L\(rustLibPath)",
                    "-lzeromax_ffi",
                ]),
                .linkedFramework("Security"),
                .linkedFramework("SystemConfiguration"),
                .linkedLibrary("sqlite3"),
            ]
        ),
    ]
)
