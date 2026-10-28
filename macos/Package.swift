// swift-tools-version: 6.2
import PackageDescription

let approachableConcurrency: [SwiftSetting] = [
    .enableUpcomingFeature("NonisolatedNonsendingByDefault"),
    .enableUpcomingFeature("InferIsolatedConformances"),
]

let package = Package(
    name: "VoxSplit",
    platforms: [.macOS(.v14)],
    products: [
        .executable(name: "VoxSplit", targets: ["VoxSplit"]),
    ],
    targets: [
        // Engine protocol + process runner. A library: nonisolated, callers decide where work runs.
        .target(
            name: "VoxSplitKit",
            swiftSettings: approachableConcurrency
        ),
        // The SwiftUI app. UI code lives on the main actor by default.
        .executableTarget(
            name: "VoxSplit",
            dependencies: ["VoxSplitKit"],
            swiftSettings: approachableConcurrency + [.defaultIsolation(MainActor.self)]
        ),
        .testTarget(
            name: "VoxSplitKitTests",
            dependencies: ["VoxSplitKit"],
            swiftSettings: approachableConcurrency
        ),
        .testTarget(
            name: "VoxSplitAppTests",
            dependencies: ["VoxSplit", "VoxSplitKit"],
            swiftSettings: approachableConcurrency + [.defaultIsolation(MainActor.self)]
        ),
    ]
)
