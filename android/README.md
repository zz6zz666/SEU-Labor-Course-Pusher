# Android 壳

这是桌面版的手机端壳：界面只做状态与配置，轮询、CAS 登录、解析、选课和去重都在
`../core` 里，通过 JNI 调用。

## 构建

需要：

- JDK 25、Android SDK Platform 36、Build Tools 36.1.0
- NDK 27.0.12077973（`app/build.gradle.kts` 固定；16 KB page 支持）
- Rust 的 `aarch64-linux-android` target 与 `cargo-ndk`
- 项目自带的 Gradle 9.8 wrapper（AGP 9.4.1，内置 Kotlin 2.2.10）

```powershell
rustup target add aarch64-linux-android
cargo install cargo-ndk
```

构建时设置 `ANDROID_HOME` 环境变量指向 SDK。Android Studio 会自行生成本地的
`local.properties`，该文件不提交；仓库里不保存任何机器绝对路径。

用 Android Studio 打开本目录，或直接构建：

```powershell
.\gradlew.bat :app:assembleDebug
```

Gradle 的 `cargoBuild` 任务会在 `preBuild` 前调用：

```text
cargo ndk -t arm64-v8a -o app/src/main/jniLibs build --profile android_release \
  -p seu-labor-core --no-default-features --features rustls,android-ffi
```

wrapper 的发行包指向腾讯云镜像；如果网络环境不同，改
`gradle/wrapper/gradle-wrapper.properties` 里的 `distributionUrl` 即可。
`settings.gradle.kts` 优先走阿里云 Maven 镜像，官方仓库作为回退。

## 结构

| 文件 | 作用 |
|---|---|
| `CoreBridge.kt` | `external fun` 声明，对应 `core/src/ffi.rs` 的 JNI 符号 |
| `MonitorService.kt` | `specialUse` 前台服务，驱动 `Core.tick()`，发通知 |
| `Notifications.kt` | 常驻通知 + 课程提醒 + 保活恢复通知 |
| `BootReceiver.kt` | 开机/更新后拉起服务；失败则发“点击恢复”通知 |
| `Watchdog.kt` | 15 分钟 WorkManager 心跳，发现服务停了就提醒 |
| `MainActivity.kt` | 镜像桌面托盘的状态与操作，每次进入检查电池豁免 |
| `LoginActivity.kt` | 系统 WebView 登录，抓取 Cookie 后交给 core |
| `CourseActivity.kt` | 注入 Cookie 后打开选课页 |
| `SettingsActivity.kt` | 账号、筛选、PushPlus、自动选课 |
| `KeepAliveAccessibilityService.kt` | 可选的进程保持器，不读取窗口内容 |

## 首次构建检查

1. `cargo ndk --version` 能跑通，`rustup target list --installed` 含
   `aarch64-linux-android`。
2. Android Studio 同步后确认 `app/src/main/jniLibs/arm64-v8a/libseu_labor_core.so`
   已生成。
3. 真机安装后依次确认：通知权限、电池不受限制、前台服务通知常驻、锁屏 10 分钟后
   仍有一次抓取、重启手机后服务自动回来。
4. 若某厂商杀掉服务，先看 `LogsActivity` 的日志，再检查该厂商的自启动/关联启动/
   锁定后台设置；无障碍只是可选的第四层。

## 已知边界

- 前台服务用 `specialUse`，避免 `dataSync` 在 Android 15+ 的 6 小时/24 小时限制。
- 常驻仍受厂商电池策略影响：需要用户开启电池不受限制、自启动/关联启动、锁定后台；
  无障碍是第四层保险，不是保证。
- 没有“退出监控”按钮，符合当前产品决定：装上即常驻。
- 首版只打 `arm64-v8a`；模拟器可自行加 `x86_64`。
