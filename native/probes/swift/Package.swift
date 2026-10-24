// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "SwiftProbe",
    platforms: [.macOS(.v13)],
    products: [.executable(name: "swift-probe", targets: ["SwiftProbe"])],
    targets: [
        .systemLibrary(name: "CProbe"),
        .executableTarget(name: "SwiftProbe", dependencies: ["CProbe"]),
    ]
)
