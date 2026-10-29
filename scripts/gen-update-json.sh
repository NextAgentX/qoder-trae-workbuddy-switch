#!/bin/bash
# 生成 tauri updater 版本清单 latest-<os>-<arch>.json
# 用法：
#   UPDATE_OS=macos UPDATE_ARCH=aarch64 sh scripts/gen-update-json.sh [owner] [repo]
#   UPDATE_OS=windows UPDATE_ARCH=x86_64 sh scripts/gen-update-json.sh [owner] [repo]
#   UPDATE_OS=linux   UPDATE_ARCH=x86_64 sh scripts/gen-update-json.sh [owner] [repo]
# 可选：BUNDLE_DIR、UPDATE_ARCHIVE_NAME
set -e
cd "$(dirname "$0")/.." || exit 1

OWNER="${1:-NextAgentX}"
REPO="${2:-qoder-trae-workbuddy-switch}"
VERSION="${UPDATE_VERSION:-$(grep '^version' src-tauri/Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')}"
UPDATE_OS="${UPDATE_OS:-macos}"
UPDATE_ARCH="${UPDATE_ARCH:-aarch64}"
UPDATE_ARCHIVE_NAME="${UPDATE_ARCHIVE_NAME:-}"

case "$UPDATE_ARCH" in
  aarch64|x86_64) ;;
  *)
    echo "gen-update-json: 不支持的架构：$UPDATE_ARCH（只支持 aarch64 或 x86_64）" >&2
    exit 1
    ;;
esac

case "$UPDATE_OS" in
  macos)
    DEFAULT_BUNDLE="macos"
    SIG_GLOB="*.app.tar.gz.sig"
    PLATFORM_KEYS="darwin-$UPDATE_ARCH"
    ;;
  windows)
    DEFAULT_BUNDLE="nsis"
    # Tauri 2 createUpdaterArtifacts=true 签的是当前版本安装包：*_VERSION_x64-setup.exe.sig
    SIG_GLOB="*_${VERSION}_x64-setup.exe.sig"
    PLATFORM_KEYS="windows-$UPDATE_ARCH-nsis windows-$UPDATE_ARCH"
    ;;
  linux)
    # 桌面端只发 deb + AppImage，其中**只有 AppImage 能自更新**：tauri-plugin-updater
    # 在 Linux 上仅支持 AppImage（deb 归系统包管理器，无法原地升级）。
    # createUpdaterArtifacts=true 时 appimage 目标会额外产出
    #   <Product>_<ver>_<arch>.AppImage.tar.gz 与同名 .sig
    DEFAULT_BUNDLE="appimage"
    SIG_GLOB="*.AppImage.tar.gz.sig"
    # 已装的旧客户端按 tauri 的平台候选顺序回退查找，第一个就是带 bundle 后缀的键。
    # 两个键都给，新老客户端都能命中（与 windows-*-nsis / windows-* 同一做法）。
    PLATFORM_KEYS="linux-$UPDATE_ARCH-appimage linux-$UPDATE_ARCH"
    ;;
  *)
    echo "gen-update-json: 不支持的系统：$UPDATE_OS（只支持 macos / linux / windows）" >&2
    exit 1
    ;;
esac

BUNDLE_DIR="${BUNDLE_DIR:-target/release/bundle/$DEFAULT_BUNDLE}"
if [ ! -d "$BUNDLE_DIR" ] && [ -d "src-tauri/target/release/bundle/$DEFAULT_BUNDLE" ]; then
  BUNDLE_DIR="src-tauri/target/release/bundle/$DEFAULT_BUNDLE"
fi

SIG_MATCHES=$(find "$BUNDLE_DIR" -maxdepth 2 -type f -name "$SIG_GLOB" | sort)
SIG_COUNT=$(printf '%s\n' "$SIG_MATCHES" | sed '/^$/d' | wc -l | tr -d ' ')
if [ "$SIG_COUNT" != 1 ]; then
  echo "gen-update-json: 期望恰好 1 个签名包（$SIG_GLOB），实际 $SIG_COUNT" >&2
  printf '%s\n' "$SIG_MATCHES" >&2
  ls -la "$BUNDLE_DIR" >&2 || true
  exit 1
fi
SIG_FILE=$SIG_MATCHES
ARCHIVE_FILE="${SIG_FILE%.sig}"
JSON_FILE="$BUNDLE_DIR/latest-$UPDATE_OS-$UPDATE_ARCH.json"

if [ ! -f "$SIG_FILE" ] || [ ! -f "$ARCHIVE_FILE" ]; then
  echo "gen-update-json: 未找到签名更新包（$SIG_GLOB in $BUNDLE_DIR）" >&2
  ls -la "$BUNDLE_DIR" >&2 || true
  exit 1
fi

SIGNATURE=$(cat "$SIG_FILE")
if [ -n "$UPDATE_ARCHIVE_NAME" ]; then
  ARCHIVE_NAME="$UPDATE_ARCHIVE_NAME"
else
  ARCHIVE_NAME=$(basename "$ARCHIVE_FILE")
fi
PUB_DATE=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
URL="https://github.com/$OWNER/$REPO/releases/latest/download/$ARCHIVE_NAME"

PLATFORMS=""
for key in $PLATFORM_KEYS; do
  [ -n "$PLATFORMS" ] && PLATFORMS="$PLATFORMS,"
  PLATFORMS="$PLATFORMS
    \"$key\": {
      \"signature\": \"$SIGNATURE\",
      \"url\": \"$URL\"
    }"
done

mkdir -p "$BUNDLE_DIR"
cat > "$JSON_FILE" << EOF
{
  "version": "$VERSION",
  "notes": "",
  "pub_date": "$PUB_DATE",
  "platforms": {$PLATFORMS
  }
}
EOF
echo "gen-update-json: 已生成 $JSON_FILE"
# ⚠️ `notes` 故意留空：updater manifest 的 notes 只能是**一整段纯文本**，做不出
# 「升级前必读 / 按主题分组 / 更早版本」。版本内容走同目录发布的
# `release-notes.json`（结构化，由 `scripts/gen-release-notes.mjs --json` 产出），
# 由 `update::update_check` 抓取并洗净后交给弹窗。别把 Markdown 塞进来。
echo "gen-update-json: 版本内容见同名 release-notes.json（结构见 docs/specs/UPDATE-NOTES-ARCH.md）"
