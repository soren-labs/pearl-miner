// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "PearlMiner",
    platforms: [
        .macOS(.v14)
    ],
    products: [
        .executable(name: "PearlMiner", targets: ["PearlMiner"])
    ],
    targets: [
        .executableTarget(
            name: "PearlMiner",
            path: "Sources/PearlMiner"
        )
    ]
)
