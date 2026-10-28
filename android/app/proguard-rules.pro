# JNI entry points are looked up by name at runtime.
-keep class com.zz6zz666.seulabor.CoreBridge { *; }
-keepclasseswithmembernames class * {
    native <methods>;
}

# WorkManager/Room are instantiated reflectively; R8 otherwise removes
# WorkDatabase_Impl and the startup initializer.
-keep class androidx.work.** { *; }
-keep class androidx.room.** { *; }
-keep class * extends androidx.room.RoomDatabase { *; }
-keep class * extends androidx.work.ListenableWorker { *; }
-keep class * implements androidx.startup.Initializer { *; }
