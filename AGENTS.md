# Notes for coding agents

Start with [README.md](README.md) for the layout and the build commands.

- **Machine-specific values.** If `AGENTS.local.md` exists next to this file, read it. It holds what cannot be in a public repository: where the Play service-account key is, the Play Console URL, local emulator names. Never print or commit a key.
- **Releasing the watch app to Google Play:** [docs/play-release.md](docs/play-release.md). It lists the track, the review rules the app was rejected for, and how to check a build on round emulators before uploading.
- **Watch UI.** Every screen has to fit a 192dp round display at the largest font size. Read the "What the Wear OS review checks" table in that file before changing `wearos/app/src/main/kotlin/dev/wearpointer/ui/RemoteScreen.kt`.
- **Gradle.** `wearos/` has no `local.properties`; export `ANDROID_HOME` and a JDK 17 `JAVA_HOME` in the command.
- **Swipe-to-dismiss on the main screen is off on purpose** (touchpad mode). Do not turn it back on to satisfy lint.
- **Store listing images:** [tools/store-assets/README.md](tools/store-assets/README.md).
- **Emulator and desktop harnesses:** [docs/emulator-harness.md](docs/emulator-harness.md), [docs/desktop-harness.md](docs/desktop-harness.md).
