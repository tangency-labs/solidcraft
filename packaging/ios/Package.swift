// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "SolidCraft",
    platforms: [.iOS("16.4")],
    products: [.library(name: "SolidCraft", targets: ["SolidCraft"])],
    targets: [
        .target(name: "SolidCraftBridge", publicHeadersPath: "include"),
        .target(
            name: "SolidCraft",
            dependencies: ["SolidCraftBridge"],
            linkerSettings: [
                .unsafeFlags([
                    "-L", Context.packageDirectory + "/Libs", "-lsolidcraft_ios",
                    "-Xlinker", "-no_adhoc_codesign",
                ]),
                .linkedFramework("UIKit"),
                .linkedFramework("Metal"),
                .linkedFramework("QuartzCore"),
                .linkedFramework("Foundation"),
                .linkedFramework("CoreGraphics"),
                .linkedFramework("CoreFoundation"),
                .linkedFramework("UniformTypeIdentifiers"),
                .linkedFramework("Security"),
                .linkedLibrary("objc"),
                .linkedLibrary("c++"),
                .linkedLibrary("iconv"),
            ]
        ),
    ],
    swiftLanguageModes: [.v5]
)
