#!/usr/bin/env bash
# Сборка APK без Gradle: cargo -> aapt2 -> zipalign -> apksigner.
#
# Нужны переменные окружения:
#   ANDROID_NDK_HOME  — каталог NDK (r25+)
#   BUILD_TOOLS       — каталог build-tools (aapt2, zipalign, apksigner)
#   ANDROID_JAR       — android.jar платформы (например, android-34)
# и цели Rust: rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
set -euo pipefail
cd "$(dirname "$0")"

: "${ANDROID_NDK_HOME:?}" "${BUILD_TOOLS:?}" "${ANDROID_JAR:?}"
API=23
BIN="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin"
OUT=build
rm -rf "$OUT" && mkdir -p "$OUT"

build() { # <rust target> <clang prefix> <abi>
    local target=$1 clang="$BIN/$2$API-clang" abi=$3
    local env_name=${target//-/_}
    env "CC_$env_name=$clang" "AR_$env_name=$BIN/llvm-ar" \
        "CARGO_TARGET_${env_name^^}_LINKER=$clang" \
        "CARGO_TARGET_${env_name^^}_RUSTFLAGS=-C link-arg=-Wl,-z,max-page-size=16384" \
        cargo build --release --target "$target"
    mkdir -p "$OUT/apk/lib/$abi"
    cp "target/$target/release/libvsbm.so" "$OUT/apk/lib/$abi/"
}
build aarch64-linux-android aarch64-linux-android arm64-v8a
build armv7-linux-androideabi armv7a-linux-androideabi armeabi-v7a
build x86_64-linux-android x86_64-linux-android x86_64

"$BUILD_TOOLS/aapt2" link -o "$OUT/unsigned.apk" --manifest AndroidManifest.xml -I "$ANDROID_JAR"
(cd "$OUT/apk" && zip -q -r ../unsigned.apk lib)
"$BUILD_TOOLS/zipalign" -f -p 16 "$OUT/unsigned.apk" "$OUT/aligned.apk"

KS=${KEYSTORE:-debug.keystore}
if [ ! -f "$KS" ]; then
    keytool -genkeypair -keystore "$KS" -storepass android -keypass android -alias vsbm \
        -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=vsbm" >/dev/null 2>&1
fi
"$BUILD_TOOLS/apksigner" sign --ks "$KS" --ks-pass pass:android --key-pass pass:android \
    --out vsbm.apk "$OUT/aligned.apk"
"$BUILD_TOOLS/apksigner" verify --verbose vsbm.apk | head -5
ls -l vsbm.apk
