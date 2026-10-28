package com.zz6zz666.seulabor

/**
 * JNI surface of `seu-labor-core`. Every call is JSON in / JSON out; the
 * package and class name must match the exported symbols in `core/src/ffi.rs`.
 */
object CoreBridge {
    init {
        System.loadLibrary("seu_labor_core")
    }

    external fun nativeInit(dataDir: String): Boolean
    external fun nativeTick(): String
    external fun nativeStatus(): String
    external fun nativeProbe(): String
    external fun nativeGetConfig(): String
    external fun nativeSetConfig(raw: String): String
    external fun nativeImportCookies(raw: String): Boolean
    external fun nativeExportCookies(): String
    external fun nativeAutoLogin(user: String, pass: String): String
    external fun nativeReadyScript(): String
    external fun nativeFillScript(user: String, pass: String): String
}
