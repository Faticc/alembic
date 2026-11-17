#!/usr/bin/env bash
# Сборка Alembic для Android: libalembic_android.so -> dist/alembic.apk.
# Нужны Android SDK (build-tools, platform, NDK), JDK и `rustup target add aarch64-linux-android`.
# Запуск из Git Bash:  scripts/android.sh            собрать
#                      scripts/android.sh install    собрать, поставить на телефон по USB и запустить
set -euo pipefail
cd "$(dirname "$0")/.."

SDK="${ANDROID_HOME:-$LOCALAPPDATA/Android/Sdk}"
SDK="$(cygpath -m "$SDK")"
# Самый новый NDK, в котором есть компилятор (недокачанные версии пропускаются).
NDK="${ANDROID_NDK_HOME:-$(ls -d "$SDK"/ndk/*/toolchains/llvm/prebuilt/windows-x86_64/bin | sort -V | tail -1 | sed 's|/toolchains/.*||')}"
BT="$(ls -d "$SDK"/build-tools/* | sort -V | tail -1)"
JAR="$(ls -d "$SDK"/platforms/android-* | sort -V | tail -1)/android.jar"
BIN="$NDK/toolchains/llvm/prebuilt/windows-x86_64/bin"
API=26
PKG=com.faticc.alembic

# Скрипты сборки зависимостей собираются под Windows; тулчейну GNU для них нужен dlltool,
# он лежит внутри самого Rust.
PATH="$PATH:$(cygpath -u "$(rustc --print sysroot)")/lib/rustlib/x86_64-pc-windows-gnu/bin/self-contained"

# Все архитектуры телефонов: rust-цель | префикс clang в NDK | папка ABI в APK.
# Только одну можно выбрать так: ALEMBIC_ABIS="arm64-v8a" scripts/android.sh
TARGETS="aarch64-linux-android:aarch64-linux-android:arm64-v8a
armv7-linux-androideabi:armv7a-linux-androideabi:armeabi-v7a
x86_64-linux-android:x86_64-linux-android:x86_64
i686-linux-android:i686-linux-android:x86"

out=target/android-apk
rm -rf "$out"
mkdir -p "$out" dist
"$BT/aapt2.exe" link -o "$out/base.apk" --manifest android/AndroidManifest.xml -I "$JAR"

while IFS=: read -r T CL ABI; do
    case " ${ALEMBIC_ABIS:-arm64-v8a armeabi-v7a x86_64 x86} " in *" $ABI "*) ;; *) continue ;; esac
    rustup ${ALEMBIC_TOOLCHAIN:+--toolchain "$ALEMBIC_TOOLCHAIN"} target add "$T" >/dev/null 2>&1 || true
    U="$(echo "$T" | tr 'a-z-' 'A-Z_')"
    L="$(echo "$T" | tr '-' '_')"
    export "CARGO_TARGET_${U}_LINKER=$BIN/$CL$API-clang.cmd"
    export "CC_$L=$BIN/$CL$API-clang.cmd"
    export "AR_$L=$BIN/llvm-ar.exe"
    # Страницы памяти по 16 КБ: так требуют новые версии Android.
    export "CARGO_TARGET_${U}_RUSTFLAGS=-C link-arg=-Wl,-z,max-page-size=16384"
    cargo ${ALEMBIC_TOOLCHAIN:+"+$ALEMBIC_TOOLCHAIN"} build --lib --release --target "$T"
    mkdir -p "$out/lib/$ABI"
    cp "target/$T/release/libalembic_android.so" "$out/lib/$ABI/"
    (cd "$out" && "$BT/aapt.exe" add base.apk "lib/$ABI/libalembic_android.so" >/dev/null)
done <<< "$TARGETS"
"$BT/zipalign.exe" -f -p 16 "$out/base.apk" "$out/aligned.apk"

# Подпись отладочным ключом Android SDK (создаётся при первой сборке).
KS="$HOME/.android/debug.keystore"
if [ ! -f "$KS" ]; then
    mkdir -p "$HOME/.android"
    keytool -genkeypair -keystore "$KS" -storepass android -keypass android -alias androiddebugkey \
        -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=Android Debug,O=Android,C=US"
fi
"$BT/apksigner.bat" sign --ks "$KS" --ks-pass pass:android --key-pass pass:android --out dist/alembic.apk "$out/aligned.apk"
echo "Готово: dist/alembic.apk"

if [ "${1:-}" = install ]; then
    ADB="$SDK/platform-tools/adb.exe"
    "$ADB" install -r dist/alembic.apk
    "$ADB" shell am start -n "$PKG/android.app.NativeActivity"
fi
