# `android-activity`

[![ci](https://github.com/rust-mobile/android-activity/actions/workflows/ci.yml/badge.svg)](https://github.com/rust-mobile/android-activity/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/android-activity.svg)](https://crates.io/crates/android-activity)
[![Docs](https://docs.rs/android-activity/badge.svg)](https://docs.rs/android-activity)
[![MSRV](https://img.shields.io/badge/rustc-1.85.0+-ab6000.svg)](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0/)

## Overview

`android-activity` provides a "glue" layer for building native Rust
applications on Android, supporting multiple [`Activity`] base classes.
It's comparable to [`android_native_app_glue.c`][ndk_concepts]
for C/C++ applications and is an alternative to the [ndk-glue] crate.

`android-activity` provides a way to load your crate as a `cdylib` library via
the `onCreate` method of your Android `Activity` class; run an `android_main`
function in a separate thread from the Java main thread and marshal events (such
as lifecycle events and input events) between Java and your native thread.

So far it supports [`NativeActivity`] or [`GameActivity`] (from the
[Android Game Development Kit][agdk]) and there's also interest in supporting a first-party
`RustActivity` base class that could be better tailored to the needs of Rust
applications.

[`Activity`]: https://developer.android.com/reference/android/app/Activity
[`NativeActivity`]: https://developer.android.com/reference/android/app/NativeActivity
[ndk_concepts]: https://developer.android.com/ndk/guides/concepts#naa
[`GameActivity`]: https://developer.android.com/games/agdk/game-activity
[ndk-glue]: https://crates.io/crates/ndk-glue
[agdk]: https://developer.android.com/games/agdk/overview

## Quick Start

**Cargo.toml:**

```toml
[dependencies]
log = "0.4"
android_logger = "0.13"
android-activity = { version = "0.6", features = [ "native-activity" ] }

[lib]
crate-type = ["cdylib"]
```

_Note: that you will need to either specify the **"native-activity"** feature or
**"game-activity"** feature to identify which `Activity` base class your
application is based on_

**lib.rs:**

```rust
use std::sync::OnceLock;
use android_activity::{AndroidApp, InputStatus, MainEvent, PollEvent};

// - Called on a dedicated Activity main loop thread, spawned after `android_on_create` returns
// - May be called multiple times if your Activity is destroyed and recreated.
// - Note: this symbol has a "Rust" ABI (default), not "C" ABI.
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {

    // `android_main` is tied to your `Activity` lifecycle, not your application lifecycle
    // and so it may be called multiple times if your Activity is destroyed and recreated.
    //
    // Use a `OnceLock` or similar to ensure that you don't attempt to initialize global state
    // multiple times.
    static APP_ONCE: OnceLock<()> = OnceLock::new();
    APP_ONCE.get_or_init(|| {
        android_logger::init_once(android_logger::Config::default().with_min_level(log::Level::Info));
    });

    loop {
        app.poll_events(Some(std::time::Duration::from_millis(500)) /* timeout */, |event| {
            match event {
                PollEvent::Wake => { log::info!("Early wake up"); },
                PollEvent::Timeout => { log::info!("Hello, World!"); },
                PollEvent::Main(main_event) => {
                    log::info!("Main event: {:?}", main_event);
                    match main_event {
                        // Once you receive a `Destroy` event, your `AndroidApp` will no longer
                        // be associated with any `Activity` and it's methods will effectively be no-ops.
                        //
                        // You should return from `android_main` and if your `Activity` gets recreated then
                        // a new `AndroidApp` will be passed to a new invocation of `android_main`.
                        MainEvent::Destroy => { return; }
                        _ => {}
                    }
                },
                _ => {}
            }

            app.input_events(|event| {
                log::info!("Input Event: {event:?}");
                InputStatus::Unhandled
            });
        });
    }
}
```

```sh
rustup target add aarch64-linux-android
cargo install cargo-apk
cargo apk run
adb logcat example:V *:S
```

_Note: although `cargo apk` is convenient for this quick start example, it's
generally recommended that you should use a more-standard, Gradle-based build
system for your Android application and use something like `cargo ndk` for
building your Rust code into a `cdylib` that is then packaged via Gradle._

## Full Examples

See [this collection of
examples](https://github.com/rust-mobile/rust-android-examples) (based on both
`GameActivity` and `NativeActivity`).

Each example is a standalone Android Studio project that can serve as a
convenient template for starting a new project.

For the examples based on middleware frameworks (Winit or Egui) they also
aim to demonstrate how it's possible to write portable code that will run on
Android and other systems.

## Optional `android_on_create` entry point

`android-activity` also supports an optional `android_on_create` entry point
that gets called from the `Activity.onCreate()` callback before `android_main()`
is called.

`android_on_create` is called from the Java main / UI thread before the
`android_main` thread is spawned.

Considering that many Android SDK APIs (such as `android.view.View`) must be
accessed from the main thread, `android_on_create` can be a good place to do any
setup work that needs to be done on the Java main thread.

For example:

```rust
use std::sync::OnceLock;
use jni::{JavaVM, objects::JObject};

#[unsafe(no_mangle)]
fn android_on_create(state: &android_activity::OnCreateState) {

    // `android_on_create` is tied to your `Activity` lifecycle, not your application lifecycle
    // and so it may be called multiple times if your activity is destroyed and recreated.
    //
    // Use a `OnceLock` or similar to ensure that you don't attempt to initialize global state
    // multiple times.
    static APP_ONCE: OnceLock<()> = OnceLock::new();
    APP_ONCE.get_or_init(|| {
        // Initialize logging...
    });
    let vm = unsafe { JavaVM::from_raw(state.vm_as_ptr().cast()) };
    let activity = state.activity_as_ptr() as jni::sys::jobject;
    // Do some other setup work on the Java main thread before `android_main` starts running
}
```

_(Note: there is also an `AndroidApp::run_on_java_main_thread()` method that
gives another way to run code on the Java main thread for some use cases)_

## Should I use NativeActivity or GameActivity?

To learn more about the `NativeActivity` class that's shipped with Android see
[here](https://developer.android.com/ndk/guides/concepts#naa).

To learn more about the `GameActivity` class that's part of the [Android Game
Developer's Kit][agdk] and also see a comparison with `NativeActivity` see
[here](https://developer.android.com/games/agdk/game-activity)

Generally speaking, if unsure, `NativeActivity` may be more convenient to start
with since you may not need to compile/link any Java or Kotlin code, but
GameActivity is likely to be the better longer-term choice, due to being based
on `AppCompatActivity` and having built in support for input methods (such as
onscreen keyboards).

### NativeActivity

- Good for: Simple apps, quick prototyping, limited text input support
- Setup: Just add the feature flag
- Limitations: No built-in input method support (can only receive physical key
  events from soft keyboards that typically only allows basic ascii input)

The unique advantage of the `NativeActivity` class is that it's shipped as part
of the Android OS and so you can use it without needing to compile or link any
Java or Kotlin code.

`NativeActivity` is technically the only way to build a native Android
application purely in Rust without any Java or Kotlin code at all.

The most significant limitation of `NativeActivity` is that it doesn't have
built-in support for input methods (such as onscreen keyboards) and so it's
often not a good choice for applications that need to support text input.

Since some soft keyboards will deliver physical key events for basic ascii input
then `NativeActivity` can enable basic text input for prototyping but this is
unlikely to be sufficient for production applications.

For advanced use cases, it would be possible to provide custom `InputConnection`
support in conjunction with `NativeActivity` but this isn't something that
`android-activity` provides out of the box currently.

### GameActivity

- Good for: Apps needing text input, modern AndroidX features
- Setup requirements:
  - Add gradle dependency: `androidx.games:games-activity:4.4.0`
  - Enable the `game-activity` feature in Cargo.toml
  - **Important**: Do NOT enable prefab support [details here](#don't-compile-and-link-the-upstream-gameactivity-prefab-c-glue-layer)
- Provides: IME support, AppCompatActivity features

`GameActivity` has built in support for input methods via the `GameTextInput`
library and so is a better choice for applications that need to support text
input.

`GameActivity` allows you to update the `ImeOptions` and actions associated with
the soft keyboard as well as receive IME span updates for tracking the user's
text input state.

`GameActivity` is based on the [`AppCompatActivity`] class, which is a standard
Jetpack / AndroidX class that offers a lot of built-in functionality to help
with compatibility across different Android versions and devices.

### Game Activity Library Version

`android-activity` currently supports the [`GameActivity` 4.4.0 Jetpack
library](https://developer.android.com/jetpack/androidx/releases/games) and is
backwards compatible with the previous `4.0.0` stable release. We can't
guarantee that the next 4.x stable release will be compatible, but it's fairly
likely that it will be.

Your Android package should depend on `androidx.games:games-activity:4.4.0` from
Google's Maven repository.

Read the upstream [GameActivity getting
started](https://developer.android.com/games/agdk/game-activity/get-started)
guide for more details on how to add the GameActivity library to your project.

#### Don't compile and link the upstream GameActivity 'prefab' (C++ glue) layer

**Important**: Do _not_ follow upstream instructions to enable native prefab
support for `GameActivity` that will compile and link the upstream C++ glue
layer as part of your build. The upstream glue layer is not directly compatible
with `android-activity` which provides its own native glue layer that integrates
with Rust.

I.e. you do _not_ need to enable prefabs via your `build.gradle` file:

```gradle
buildFeatures {
  prefab true
}
```

and do _not_ add a snippet like this to your `CMakeLists.txt` file:

```cmake
find_package(game-activity REQUIRED CONFIG)
target_link_libraries(${PROJECT_NAME} PUBLIC log android
game-activity::game-activity_static)
```

### Planning to Implement an Activity Subclass

It's not possible to subclass an Activity from Rust / JNI code alone.

Keep in mind that Android's design directs many events via the `Activity` class
which can only be processed by overloading some associated `Activity` method, so
if you want to handle those events then you will need to implement an `Activity`
subclass and overload the relevant methods.

Most moderately complex applications will eventually need to define their own
`Activity` subclass (either subclassing `NativeActivity` or `GameActivity`)
which will require compiling at least a small amount of Java or Kotlin code.

_At the end of the day, Android's application programming model is fundamentally
based around a Java VM running Java/Kotlin code that can optionally call into
native code (not the other way around)._

## Design Summary / Motivation behind android-activity

Prior to working on `android-activity`, the existing glue crates available for
building standalone Rust applications on Android were found to have a number of
technical limitations that this crate aimed to solve:

1. **Support alternative Activity classes**: Prior glue crates were based on
   `NativeActivity` and their API precluded supporting alternatives. In
   particular there was an interest in the [`GameActivity`] class in conjunction
   with its [`GameTextInput`] library that can facilitate onscreen keyboard
   support. This also allows building applications based on the standard
   [`AppCompatActivity`] base class which isn't possible with `NativeActivity`.
   Finally there was interest in paving the way towards supporting a first-party
   `RustActivity` that could be best tailored towards the needs of Rust
   applications on Android.
2. **Encapsulate IPC + synchronization between the native thread and the JVM thread**:
   For example with `ndk-glue` the application itself needs to avoid
   race conditions between the native and Java thread by following a locking
   convention) and it wasn't clear how this would extend to support other
   requests (like state saving) that also require synchronization.
3. **Avoid static global state**: Keeping in mind the possibility of supporting
   applications with multiple native activities there was interest in having an
   API that didn't rely on global statics to track top-level state. Instead of
   having global getters for state then `android-activity` passes an explicit
   `app: AndroidApp` argument to the entry point that encapsulates the state
   connected with a single `Activity`.

    It's possible to write an application with `android-activity` that can
    gracefully handle repeated create -> run -> destroy cycles of the `Activity`
    due to its avoidance of global state. Theoretically you could even run
    multiple `Activity` instances at the same (though since `NativeActivity` and
    `GameActivity` were designed for fullscreen games, that only need a single
    Activity, this is not a common use case).

[`GameTextInput`]: https://developer.android.com/games/agdk/add-support-for-text-input
[`AppCompatActivity`]: https://developer.android.com/reference/androidx/appcompat/app/AppCompatActivity

## MSRV

We aim to (at least) support stable releases of Rust from the last three months.
Rust has a 6 week release cycle which means we will support the last three
stable releases. For example, when Rust 1.69 is released we would limit our
`rust-version` to 1.67.

We will only bump the `rust-version` at the point where we either depend on a
new features or a dependency has increased its MSRV, and we won't be greedy. In
other words we will only set the MSRV to the lowest version that's _needed_.

MSRV updates are not considered to be inherently semver breaking (unless a new
feature is exposed in the public API) and so a `rust-version` change may happen
in patch releases.

## Game Activity Library Versioning Policy

Any single release of `android-activity` will support a specific version of the
Game Activity Jetpack / AndroidX library (documented above).

The required version of the Game Activity library does not form part of our Rust
semver contract, since it doesn't affect the public Rust API of
`android-activity`.

This means that a new patch release of `android-activity` may update the
required version of `GameActivity`, which may require users to update how they
package their application.

This is similar to how MSRV updates work, where new toolchain requirements can
affect how you build your application but that change is orthogonal to the
public API of the crate.
