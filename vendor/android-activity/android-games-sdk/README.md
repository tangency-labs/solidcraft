# android-games-sdk

This is an imported copy of the native "prefab" source for `GameActivity` and
`GameTextInput`, from our fork of Google's
[android-games-sdk](https://github.com/rust-mobile/android-games-sdk).

We use an external fork to track our integration patches on top of the Android
Game Development Kit (AGDK) in a way that it is easier to update to new upstream
versions. It also makes it easier to try and upstream changes when we fix bugs.

## Updating to new agdk version checklist

This is a basic checklist for things that need to be done when updating to a new
agdk version:

- [ ] Create a new integration branch based on our last integrated branch and
  rebase that on the latest *release* branch from Google:

    ```bash
    git clone git@github.com:rust-mobile/android-games-sdk.git
    cd android-games-sdk
    git remote add google https://android.googlesource.com/platform/frameworks/opt/gamesdk
    git fetch google
    git checkout -b android-activity-5.0.0 origin/android-activity-4.0.0
    git rebase --onto google/android-games-sdk-game-activity-release <base>
    # (where <base> is the upstream commit ID below our stack of integration patches)
    ```

- [ ] Set the `ANDROID_GAMES_SDK` environment variable so you can build
  android-activity against your external games-sdk branch while updating.
- [ ] Re-generate the `GameActivity` FFI bindings with `./generate-bindings.sh`
  (this can be done with `ANDROID_GAMES_SDK` set in your environment and also
  repeated after importing)
- [ ] Update [build.rs](../build.rs) with any new includes and src files
- [ ] Update the `src/game-activity` backend as needed
- [ ] Push a new `android-games-sdk` branch like `android-activity-5.0.0` that
  can be referenced when importing a copy into `android-activity`
- [ ] Review and run `./import-games-sdk.sh` when ready to copy external AGDK
  code into this repo
- [ ] Clearly reference the branch name and commit hash from the
  `android-games-sdk` repo in the `android-activity` commit that imports new
  games-sdk source.
- [ ] Update CHANGELOG.md as required
