#!/usr/bin/env bash
# Выпуск новой версии: собирает игру и установщик, публикует релиз на GitHub.
# Перед запуском подними version в Cargo.toml и закоммить изменения.
# Запуск из Git Bash:  scripts/release.sh "что нового"
set -euo pipefail
cd "$(dirname "$0")/.."

version=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
tag="v$version"
notes="${1:-Обновление $tag}"

if gh release view "$tag" >/dev/null 2>&1; then
    echo "Релиз $tag уже есть: подними version в Cargo.toml." >&2
    exit 1
fi
if [ -n "$(git status --porcelain)" ]; then
    echo "Есть незакоммиченные изменения: сначала git commit." >&2
    exit 1
fi

cargo build --release
PATH="$PWD/target/release:$PATH" cargo test --release
cargo build --release --manifest-path installer/Cargo.toml
mkdir -p dist
cp -f installer/target/release/AlembicSetup.exe dist/AlembicSetup.exe

git push origin HEAD
# alembic.exe скачивают установленные копии игры, AlembicSetup.exe - новые игроки.
gh release create "$tag" target/release/alembic.exe dist/AlembicSetup.exe --title "Alembic $tag" --notes "$notes"
echo "Готово: $tag опубликован."
