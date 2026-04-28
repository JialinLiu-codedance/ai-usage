#!/usr/bin/env sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)

PRODUCT_NAME=$(cd "$REPO_ROOT" && node -e "const fs=require('fs'); const config=JSON.parse(fs.readFileSync('src-tauri/tauri.conf.json','utf8')); process.stdout.write(config.productName);")
APP_VERSION=$(cd "$REPO_ROOT" && node -e "const fs=require('fs'); const config=JSON.parse(fs.readFileSync('src-tauri/tauri.conf.json','utf8')); process.stdout.write(config.version);")

case "$(uname -m)" in
  arm64)
    TARGET_ARCH="aarch64"
    ;;
  x86_64)
    TARGET_ARCH="x64"
    ;;
  *)
    TARGET_ARCH="$(uname -m)"
    ;;
esac

APP_PATH="$REPO_ROOT/src-tauri/target/release/bundle/macos/$PRODUCT_NAME.app"
DMG_PATH="$REPO_ROOT/src-tauri/target/release/bundle/dmg/${PRODUCT_NAME}_${APP_VERSION}_${TARGET_ARCH}.dmg"
VOLUME_PATH="/Volumes/$PRODUCT_NAME"
MOUNTED_DEVICE=""

log() {
  printf '\n==> %s\n' "$*"
}

fail() {
  printf 'ERROR: %s\n' "$*" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || fail "缺少命令：$1"
}

whole_disk() {
  printf '%s\n' "$1" | sed 's/s[0-9][0-9]*$//'
}

mounted_device_for_volume() {
  hdiutil info | awk -v mount="$1" 'index($0, mount) { print $1; exit }'
}

detach_volume_if_mounted() {
  DEVICE=$(mounted_device_for_volume "$VOLUME_PATH")
  if [ -n "$DEVICE" ]; then
    DEVICE=$(whole_disk "$DEVICE")
    log "卸载旧 DMG 挂载卷：$DEVICE ($VOLUME_PATH)"
    hdiutil detach "$DEVICE"
  fi
}

cleanup() {
  if [ -n "$MOUNTED_DEVICE" ]; then
    log "卸载校验挂载卷：$MOUNTED_DEVICE"
    hdiutil detach "$MOUNTED_DEVICE" >/dev/null 2>&1 || true
  fi
}

trap cleanup EXIT HUP INT TERM

require_command node
require_command npm
require_command cargo
require_command hdiutil
require_command codesign

log "项目目录：$REPO_ROOT"
log "目标产物：$PRODUCT_NAME $APP_VERSION ($TARGET_ARCH)"

detach_volume_if_mounted

log "执行 Tauri 打包"
cd "$REPO_ROOT"
npm run tauri -- build

[ -d "$APP_PATH" ] || fail "未找到 app 产物：$APP_PATH"
[ -f "$DMG_PATH" ] || fail "未找到 dmg 产物：$DMG_PATH"

log "校验 app 签名"
codesign --verify --deep --strict --verbose=2 "$APP_PATH"
codesign -dv --verbose=4 "$APP_PATH"

log "校验 app 图标资源"
ICON_FILE=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIconFile' "$APP_PATH/Contents/Info.plist")
[ "$ICON_FILE" = "icon.icns" ] || fail "CFBundleIconFile 不是 icon.icns：$ICON_FILE"
[ -f "$APP_PATH/Contents/Resources/icon.icns" ] || fail "缺少图标资源：$APP_PATH/Contents/Resources/icon.icns"

log "校验 DMG checksum"
hdiutil verify "$DMG_PATH"

log "挂载 DMG 并检查内部 app"
ATTACH_OUTPUT=$(hdiutil attach -nobrowse -readonly "$DMG_PATH")
printf '%s\n' "$ATTACH_OUTPUT"
MOUNTED_DEVICE=$(printf '%s\n' "$ATTACH_OUTPUT" | awk -v mount="$VOLUME_PATH" 'index($0, mount) { print $1; exit }')
[ -n "$MOUNTED_DEVICE" ] || fail "DMG 已挂载但未找到挂载卷：$VOLUME_PATH"
MOUNTED_DEVICE=$(whole_disk "$MOUNTED_DEVICE")

[ -d "$VOLUME_PATH/$PRODUCT_NAME.app" ] || fail "DMG 内缺少 $PRODUCT_NAME.app"
[ -f "$VOLUME_PATH/$PRODUCT_NAME.app/Contents/Resources/icon.icns" ] || fail "DMG 内 app 缺少 icon.icns"
DMG_BUNDLE_ID=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$VOLUME_PATH/$PRODUCT_NAME.app/Contents/Info.plist")
[ "$DMG_BUNDLE_ID" = "com.liujialin.ai-usage" ] || fail "DMG 内 app bundle id 异常：$DMG_BUNDLE_ID"

cleanup
MOUNTED_DEVICE=""

log "打包完成"
printf 'APP: %s\n' "$APP_PATH"
printf 'DMG: %s\n' "$DMG_PATH"
