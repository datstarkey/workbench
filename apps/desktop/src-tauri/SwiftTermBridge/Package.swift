// swift-tools-version: 5.9

import PackageDescription

let package = Package(
    name: "SwiftTermBridge",
    platforms: [
        .macOS(.v13)
    ],
    products: [
        .library(
            name: "SwiftTermBridge",
            type: .static,
            targets: ["SwiftTermBridge"]
        )
    ],
    dependencies: [
        .package(url: "https://github.com/migueldeicaza/SwiftTerm.git", from: "1.0.0")
    ],
    targets: [
        .target(
            name: "SwiftTermBridge",
            dependencies: ["SwiftTerm"],
            publicHeadersPath: "include"
        )
    ]
)
