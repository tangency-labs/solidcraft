import SolidCraftBridge

// Packaging glue only. Rust/winit owns UIApplication, windows, input and the CAD app.
@main
enum SolidCraftLauncher {
    static func main() {
        solidcraft_ios_main()
    }
}
