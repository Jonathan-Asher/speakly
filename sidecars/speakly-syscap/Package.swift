// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "speakly-syscap",
    platforms: [.macOS(.v13)],
    targets: [
        .executableTarget(name: "speakly-syscap", path: "Sources/speakly-syscap")
    ]
)
