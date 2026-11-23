# Google Play release (Wear OS app)

How the watch app gets to Google Play, and what the Wear OS review looks at. Written after the version code 7 rejection (November 2026) so the next release does not repeat it.

## Facts

| | |
| --- | --- |
| Play package | `dev.wearpointer.app` (release build type; `applicationIdSuffix = ".app"`) |
| Debug package | `dev.wearpointer`. A Play listing with this id exists as an unused draft. Do not upload to it. |
| Track | `wear:production`. The plain `production` track has no release. |
| Version | `versionCode` / `versionName` in `wearos/app/build.gradle.kts`. Check the codes already uploaded before choosing the next one; a code cannot be reused, even for a bundle that was replaced. |
| Signing | `wearos/key.properties` and the upload keystore, both outside git. |

Machine-specific values (where the Play service-account key is, the Console URL, emulator names) are in `AGENTS.local.md` at the repository root, which is not in git.

## Build

```sh
cd wearos
export ANDROID_HOME="$HOME/Library/Android/sdk" JAVA_HOME=$(/usr/libexec/java_home -v 17)
./gradlew :app:testDebugUnitTest :app:lintRelease :app:assembleRelease :app:bundleRelease
```

`wearos/` has no `local.properties`, so `ANDROID_HOME` has to be exported. Test the minified release APK, not the debug build: R8 and resource shrinking are where the launch screen and the Ongoing Activity would break.

## Upload and review

1. Upload the bundle with the Play Developer API: `edits.insert`, `edits.bundles.upload`, `edits.tracks.update` on `wear:production` (status `completed`, release notes for `en-US` and `zh-TW`, under 500 characters each), `edits.commit`.
2. After a rejection, `edits.commit` and `edits.validate` fail with "Changes cannot be sent for review automatically". Commit with `changesNotSentForReview=true`, then open **Publishing overview** in Play Console and press **Send N changes for review**. Read the list of changes first. The button is disabled while Play runs its quick checks.
3. When nothing is pending from a rejection, the opposite holds: the API refuses `changesNotSentForReview` and sends changes for review itself.
4. Store listing images cannot be changed through the API with the service account used so far (403 at commit). Upload them in Play Console.

## What the Wear OS review checks

From the [Wear app quality guidelines](https://developer.android.com/docs/quality-guidelines/wear-app-quality). Version code 7 was rejected for the first two; the unused draft listing was rejected for the third.

| Rule | In this app |
| --- | --- |
| Nothing cut off by a round display, at the default and at larger font sizes | The status capsule is capped to the chord of the circle at its height (`pillMaxWidth`) and shows the longest label that fits. Settings, Privacy and the permission notices are `ScalingLazyColumn` pages. Dock labels and pad hints are dropped or shortened when they do not fit. Do not put a fixed-size row near the top or bottom of a non-scrolling screen. |
| Launch screen: 48dp app icon on black | `Theme.WearPointer.Starting` (core-splashscreen) with `drawable-nodpi/splash_icon.png`. The system enlarges and masks the launch drawable, so the icon sits inside a transparent margin; see the comment in `themes.xml` before changing it. |
| Ongoing Activity for a running session | `PointerService` builds the foreground notification with `androidx.wear.ongoing`. It needs the notification permission to show. |
| Text at least 12sp (10sp for secondary text) | All text in `RemoteScreen.kt` is 12sp or larger. |
| Swipe to close | Settings, Privacy and the notices close with a swipe. The main screen does not, on purpose: touchpad mode reads horizontal slides. Settings has a **Close app** row. A reviewer may still object to this. |
| 48dp touch targets | Not met on the main screen (status capsule, dock buttons, Start/Stop). Not cited so far. |

## Checking a build before upload

Two round emulators cover the sizes the review uses: `wearos_small_round` (192dp) and `wearos_large_round` (227dp), on a Wear OS 5 image. On each, at font scale 1.0 and at the largest setting (`adb shell settings put system font_scale 1.24`):

- Walk every screen and state: standby, paused, resumed, stopped, Settings scrolled top to bottom, each link type, Privacy, the permission notice (revoke the Bluetooth permissions with `pm revoke`), the blocked notice (`pm set-permission-flags … user-fixed`).
- Take `adb exec-out screencap -p` at each step and test the pixels: nothing brighter than the background may lie outside the circle inscribed in the square capture. The emulator does not mask its screenshots, so the eye misses this. `uiautomator dump` gives nodes by content description for tapping.
- Connected states need a peer: run `wearos/probe` on a phone emulator (see [emulator-harness.md](emulator-harness.md)).
- Launch screen: `screenrecord` drops it, and on a start from the launcher the icon that flies to the centre is the launcher's own animation. To measure the app's launch icon, hold it with a temporary `setKeepOnScreenCondition` in a debug build and start with `am start --splashscreen-show-icon`.
- Ongoing Activity: go to the watch face and look for the indicator (`uiautomator dump` shows "Ongoing Wear Pointer running in the background"), tap it, then Stop and confirm it is gone.
- Bezel: `adb shell input rotaryencoder scroll --axis SCROLL,3`. It steps DPI on Settings and scrolls the Privacy and notice pages.

## Listing images

See [tools/store-assets/README.md](../tools/store-assets/README.md). Refresh `tools/store-assets/screens/` after a UI change and run `generate.py`.
