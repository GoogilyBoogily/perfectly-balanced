#!/bin/bash
# Build and package the Perfectly Balanced plugin for Unraid
set -euo pipefail

VERSION="${1:-$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')}"
PLUGIN="perfectly-balanced"
ARCH="x86_64"
BUILD_DIR="$(mktemp -d)"
PKG_NAME="${PLUGIN}-${VERSION}-${ARCH}-1"

echo "=== Building Perfectly Balanced v${VERSION} ==="

# Step 1: Build the Rust binary for musl target
echo "Building Rust binary..."
cross build --release --target x86_64-unknown-linux-musl 2>/dev/null || {
    echo "Note: 'cross' not found or build failed, trying native cargo..."
    cargo build --release --target x86_64-unknown-linux-musl
}

BINARY="target/x86_64-unknown-linux-musl/release/${PLUGIN}"
if [ ! -f "$BINARY" ]; then
    echo "Error: Binary not found at $BINARY"
    echo "If cross-compiling is not set up, build on a Linux system with musl toolchain."
    exit 1
fi

echo "Binary size: $(du -h "$BINARY" | cut -f1)"

# Step 2: Build the Slackware package directory structure
echo "Building package structure..."

PLUGDIR="${BUILD_DIR}/usr/local/emhttp/plugins/${PLUGIN}"
mkdir -p "${PLUGDIR}/event"
mkdir -p "${BUILD_DIR}/install"

# Copy binary
cp "$BINARY" "${PLUGDIR}/${PLUGIN}"
chmod 755 "${PLUGDIR}/${PLUGIN}"

# Copy plugin files
cp plugin/pages/perfectly-balanced.page "${PLUGDIR}/perfectly-balanced.page"
cp plugin/rc.perfectly-balanced "${PLUGDIR}/rc.perfectly-balanced"
cp plugin/event/started "${PLUGDIR}/event/started"
cp plugin/event/stopping_svcs "${PLUGDIR}/event/stopping_svcs"

# Make scripts executable
chmod 755 "${PLUGDIR}/rc.perfectly-balanced"
chmod 755 "${PLUGDIR}/event/"*

# Create slack-desc
cat > "${BUILD_DIR}/install/slack-desc" <<EOF
${PLUGIN}: ${PLUGIN}
${PLUGIN}:
${PLUGIN}: Unraid plugin to balance disk usage across array drives.
${PLUGIN}: Uses parallel filesystem scanning, a greedy bin-packing algorithm,
${PLUGIN}: and rsync-based file transfers with real-time progress.
${PLUGIN}:
${PLUGIN}: https://github.com/googboog/perfectly-balanced
${PLUGIN}:
EOF

# Step 3: Create the .txz package
echo "Creating Slackware package..."
cd "$BUILD_DIR"
OUTPUT_DIR="$(cd - > /dev/null && pwd)/packaging"
mkdir -p "$OUTPUT_DIR"

# Use makepkg if available, otherwise tar+xz
if command -v makepkg &> /dev/null; then
    makepkg -l y -c n "${OUTPUT_DIR}/${PKG_NAME}.txz"
else
    tar cJf "${OUTPUT_DIR}/${PKG_NAME}.txz" .
fi

cd - > /dev/null

# Cleanup
rm -rf "$BUILD_DIR"

# Step 4: Compute MD5 and inject into PLG
echo "Computing MD5 hash..."
PKG_PATH="packaging/${PKG_NAME}.txz"
MD5SUM=$(md5sum "$PKG_PATH" | awk '{print $1}')

PLG_FILE="plugin/pkg/${PLUGIN}.plg"
if [ -f "$PLG_FILE" ]; then
    sed -i.bak "s|<MD5>[^<]*</MD5>|<MD5>${MD5SUM}</MD5>|" "$PLG_FILE"
    rm -f "${PLG_FILE}.bak"
    echo "  MD5 injected into PLG: ${MD5SUM}"
fi

echo ""
echo "=== Package built successfully ==="
echo "  Package: ${PKG_PATH}"
echo "  Size: $(du -h "${PKG_PATH}" | cut -f1)"
echo "  MD5: ${MD5SUM}"
echo ""
echo "To install on Unraid:"
echo "  1. Upload the .plg file to your Unraid server"
echo "  2. Install via Plugins > Install Plugin"
echo "  3. Or copy .txz to /boot/config/plugins/${PLUGIN}/ and install the .plg"
