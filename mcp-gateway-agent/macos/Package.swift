// swift-tools-version: 6.0
import PackageDescription

// The app is built with SwiftPM rather than an .xcodeproj so that a checkout
// builds with nothing but the Command Line Tools, and so the build is a script
// anyone can read. `build.sh` compiles the Rust staticlib, runs `swift build`,
// and assembles the .app bundle around the product.
//
// macOS 26 is a hard floor: the whole chrome is Liquid Glass, and `.glassEffect`
// has no pre-26 equivalent worth shimming.
let package = Package(
    name: "MCPGatewayAgent",
    platforms: [.macOS("26.0")],
    targets: [
        // The C ABI of ../ffi. Header only — the implementation is the Rust
        // staticlib, linked in below.
        .target(name: "MCPGatewayAgentFFI"),

        .executableTarget(
            name: "MCPGatewayAgent",
            dependencies: ["MCPGatewayAgentFFI"],
            linkerSettings: [
                // `build.sh` supplies the -L search path with
                // `-Xlinker -L<dir>`, because it depends on the Rust target
                // triple and cannot be written down here.
                .linkedLibrary("mcp_gateway_agent_ffi"),
                .linkedFramework("AppKit"),
                .linkedFramework("Security"),
                .linkedFramework("CoreFoundation"),
                .linkedFramework("SystemConfiguration"),
            ]
        ),
    ]
)
