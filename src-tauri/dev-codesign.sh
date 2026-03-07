#!/bin/bash
# 构建后签名二进制文件和 .app，使 Entitlements 生效
# 用法: ./src-tauri/dev-codesign.sh [debug|release]
#
# 在 `cargo tauri dev` 或 `npx tauri build` 完成后运行此脚本，
# 然后重新启动应用（不重新编译）即可生效。
#
# HAL Tap (应用音频捕获) 需要以下 Entitlements:
#   - com.apple.security.device.audio-input
#   - com.apple.security.cs.allow-unsigned-executable-memory
#   - com.apple.security.cs.disable-library-validation

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ENTITLEMENTS="$SCRIPT_DIR/Entitlements.plist"

# 支持指定 debug/release 模式
MODE="${1:-debug}"
BINARY="$SCRIPT_DIR/target/$MODE/aitrans"
APP_BUNDLE="$SCRIPT_DIR/target/$MODE/bundle/macos/aitrans.app"

# 如果指定路径不存在，尝试在 Tauri dev 运行时的路径找
if [ ! -f "$BINARY" ]; then
    # cargo tauri dev 有时也把二进制放在 target/debug
    ALT_BINARY="$SCRIPT_DIR/target/debug/aitrans"
    if [ -f "$ALT_BINARY" ]; then
        BINARY="$ALT_BINARY"
        MODE="debug"
        APP_BUNDLE="$SCRIPT_DIR/target/debug/bundle/macos/aitrans.app"
    else
        echo "❌ 找不到二进制文件: $BINARY"
        echo "   请先运行 'npx tauri dev' 或 'npx tauri build --debug' 构建项目"
        exit 1
    fi
fi

if [ ! -f "$ENTITLEMENTS" ]; then
    echo "❌ 找不到 Entitlements 文件: $ENTITLEMENTS"
    exit 1
fi

# 签名函数
sign_target() {
    local TARGET="$1"
    local LABEL="$2"
    local IS_APP="$3"

    echo ""
    echo "🔏 正在签名 $LABEL: $TARGET ..."

    if [ -n "$DEV_IDENTITY" ]; then
        echo "   使用 Apple Developer 证书: $DEV_IDENTITY"
        if [ "$IS_APP" = "app" ]; then
            codesign --force --deep --sign "$DEV_IDENTITY" \
                --entitlements "$ENTITLEMENTS" \
                --options runtime \
                "$TARGET"
        else
            codesign --force --sign "$DEV_IDENTITY" \
                --entitlements "$ENTITLEMENTS" \
                --options runtime \
                "$TARGET"
        fi
    else
        if [ "$IS_APP" = "app" ]; then
            codesign --force --deep --sign - \
                --entitlements "$ENTITLEMENTS" \
                --options runtime \
                "$TARGET"
        else
            codesign --force --sign - \
                --entitlements "$ENTITLEMENTS" \
                --options runtime \
                "$TARGET"
        fi
    fi
}

# 验证函数
verify_target() {
    local TARGET="$1"
    local LABEL="$2"

    echo ""
    echo "📋 验证 $LABEL 的 Entitlements:"
    local ENTITLEMENTS_OUTPUT
    ENTITLEMENTS_OUTPUT=$(codesign -d --entitlements - "$TARGET" 2>&1 || true)
    if echo "$ENTITLEMENTS_OUTPUT" | grep -q "device.audio-input"; then
        echo "   ✅ com.apple.security.device.audio-input"
    else
        echo "   ❌ com.apple.security.device.audio-input 未嵌入！"
    fi
    if echo "$ENTITLEMENTS_OUTPUT" | grep -q "allow-unsigned-executable-memory"; then
        echo "   ✅ com.apple.security.cs.allow-unsigned-executable-memory"
    else
        echo "   ❌ com.apple.security.cs.allow-unsigned-executable-memory 未嵌入！"
    fi
    if echo "$ENTITLEMENTS_OUTPUT" | grep -q "disable-library-validation"; then
        echo "   ✅ com.apple.security.cs.disable-library-validation"
    else
        echo "   ❌ com.apple.security.cs.disable-library-validation 未嵌入！"
    fi

    echo "   签名信息:"
    codesign -dvvv "$TARGET" 2>&1 | grep -E "Authority|Entitlements|Signature|Flags" | sed 's/^/   /'
}

echo "========================================="
echo "  aitrans 代码签名 + Entitlements 工具"
echo "========================================="
echo "   Entitlements: $ENTITLEMENTS"
echo "   模式: $MODE"

# 检查是否有 Apple Developer 证书可用
DEV_IDENTITY=$(security find-identity -v -p codesigning 2>/dev/null | grep "Apple Development" | head -1 | sed 's/.*"\(.*\)"/\1/' || true)

if [ -z "$DEV_IDENTITY" ]; then
    echo ""
    echo "   ⚠️  未找到 Apple Developer 证书，使用 ad-hoc 签名"
    echo "   如果 HAL Tap 仍然报 'who?' 错误，请使用 Apple Developer 证书"
fi

# 1. 签名裸二进制
sign_target "$BINARY" "裸二进制文件" "bin"

# 2. 签名 .app 包（如果存在）
if [ -d "$APP_BUNDLE" ]; then
    sign_target "$APP_BUNDLE" ".app 应用包" "app"
else
    echo ""
    echo "ℹ️  未找到 .app 包: $APP_BUNDLE (跳过)"
fi

echo ""
echo "========================================="
echo "✅ 签名完成！"
echo "========================================="

# 验证
verify_target "$BINARY" "裸二进制"
if [ -d "$APP_BUNDLE" ]; then
    APP_BINARY="$APP_BUNDLE/Contents/MacOS/aitrans"
    if [ -f "$APP_BINARY" ]; then
        verify_target "$APP_BINARY" ".app 内二进制"
    fi
fi

echo ""
echo "现在可以运行:"
echo "  裸二进制: $BINARY"
if [ -d "$APP_BUNDLE" ]; then
    echo "  .app 包:  open $APP_BUNDLE"
fi
