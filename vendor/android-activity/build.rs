fn build_glue_for_game_activity() {
    let android_games_sdk =
        std::env::var("ANDROID_GAMES_SDK").unwrap_or_else(|_err| "android-games-sdk".to_string());

    let activity_path = |src_inc, name| {
        format!("{android_games_sdk}/game-activity/prefab-src/modules/game-activity/{src_inc}/game-activity/{name}")
    };
    let textinput_path = |src_inc, name| {
        format!("{android_games_sdk}/game-text-input/prefab-src/modules/game-text-input/{src_inc}/game-text-input/{name}")
    };

    for f in [
        "GameActivity.cpp",
        "GameActivityEvents.cpp",
        "GameActivityEvents_internal.h",
    ] {
        println!("cargo:rerun-if-changed={}", activity_path("src", f));
    }

    for f in [
        "GameActivity.h",
        "GameActivityEvents.h",
        "GameActivityLog.h",
    ] {
        println!("cargo:rerun-if-changed={}", activity_path("include", f));
    }

    cc::Build::new()
        .cpp(true)
        .include("android-games-sdk/src/common")
        .file("android-games-sdk/src/common/system_utils.cpp")
        .extra_warnings(false)
        .cpp_link_stdlib("c++_static")
        .compile("libgame_common.a");

    println!("cargo:rerun-if-changed=android-games-sdk/src/common/system_utils.cpp");
    println!("cargo:rerun-if-changed=android-games-sdk/src/common/system_utils.h");

    cc::Build::new()
        .cpp(true)
        .include("android-games-sdk/src/common")
        .include("android-games-sdk/include")
        .include("android-games-sdk/game-activity/prefab-src/modules/game-activity/include")
        .include("android-games-sdk/game-text-input/prefab-src/modules/game-text-input/include")
        .file(activity_path("src", "GameActivity.cpp"))
        .file(activity_path("src", "GameActivityEvents.cpp"))
        .extra_warnings(false)
        .cpp_link_stdlib("c++_static")
        .compile("libgame_activity.a");

    println!(
        "cargo:rerun-if-changed={}",
        textinput_path("include", "gametextinput.h")
    );
    println!(
        "cargo:rerun-if-changed={}",
        textinput_path("src", "gametextinput.cpp")
    );

    cc::Build::new()
        .cpp(true)
        .include("android-games-sdk/src/common")
        .include("android-games-sdk/include")
        .include("android-games-sdk/game-text-input/prefab-src/modules/game-text-input/include")
        .file(textinput_path("src", "gametextinput.cpp"))
        .cpp_link_stdlib("c++_static")
        .compile("libgame_text_input.a");

    println!(
        "cargo:rerun-if-changed={}",
        activity_path("src", "native_app_glue/android_native_app_glue.c")
    );
    println!(
        "cargo:rerun-if-changed={}",
        activity_path("include", "native_app_glue/android_native_app_glue.h")
    );

    cc::Build::new()
        .include("android-games-sdk/src/common")
        .include("android-games-sdk/include")
        .include("android-games-sdk/game-activity/prefab-src/modules/game-activity/include")
        .include("android-games-sdk/game-text-input/prefab-src/modules/game-text-input/include")
        .include(activity_path("include", ""))
        .file(activity_path(
            "src",
            "native_app_glue/android_native_app_glue.c",
        ))
        .extra_warnings(false)
        .cpp_link_stdlib("c++_static")
        .compile("libnative_app_glue.a");

    // We need to link to both c++_static and c++abi for the static C++ library.
    // Ideally we'd link directly to libc++.a.
    println!("cargo:rustc-link-lib=c++abi");
}

fn main() {
    // Enable Cargo's change-detection to avoid re-running build script if
    // irrelvant parts changed. Using build.rs here is just a dummy used to
    // disable the default "rerun on every change" behaviour Cargo has.
    println!("cargo:rerun-if-changed=build.rs");

    if cfg!(feature = "game-activity") {
        build_glue_for_game_activity();
    }

    // Whether this is used directly in or as a dependency on docs.rs.
    //
    // `cfg(docsrs)` cannot be used, since it's only set for the crate being
    // built, and not for any dependent crates.
    println!("cargo:rustc-check-cfg=cfg(used_on_docsrs)");
    if std::env::var("DOCS_RS").is_ok() {
        println!("cargo:rustc-cfg=used_on_docsrs");
    }
}
