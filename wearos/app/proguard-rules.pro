# wear-pointer release shrinking.
# The app uses no reflection or serialization; Compose, AndroidX and
# kotlinx-coroutines ship their own consumer rules. Keep line numbers so
# Play Console / logcat stack traces stay readable.
-keepattributes SourceFile,LineNumberTable
-renamesourcefileattribute SourceFile
