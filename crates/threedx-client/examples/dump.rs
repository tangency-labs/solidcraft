//! Prints what the 3Dconnexion driver reports: `cargo run -p solidcraft-3dx-client --example dump`.
//! The driver sends nothing to a program that is not in an app bundle, so only `DeviceAdded` shows here;
//! use SolidCraft's own `.app` to see the cap move.
// A developer tool run by hand, not a process another program starts (see print_stderr in Cargo.toml).
#![allow(clippy::print_stderr)]

fn main() {
    if let Err(error) = solidcraft_3dx_client::start(
        "SolidCraft dump",
        |event| println!("{event:?}"),
        |connected| {
            if let Err(error) = connected {
                eprintln!("{error}");
            }
        },
    ) {
        eprintln!("{error}");
        return;
    }
    std::thread::sleep(std::time::Duration::from_secs(60));
}
