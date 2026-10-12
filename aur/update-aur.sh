#!/bin/bash
# Update AUR package for a new release
# Usage: ./update-aur.sh v0.2.0

set -e

VERSION="${1:-$(gh release view --repo hyperb1iss/unifly --json tagName -q .tagName 2>/dev/null)}"
if [ -z "$VERSION" ]; then
    echo "Usage: $0 <version>"
    echo "Example: $0 v0.2.0"
    exit 1
fi

VERSION_NUM="${VERSION#v}"
REPO="hyperb1iss/unifly"

echo "Updating AUR package to ${VERSION}..."

# Calculate SHA256 checksums
echo "Calculating checksums..."
SHA_X86_64=$(curl -fsSL "https://github.com/${REPO}/releases/download/${VERSION}/unifly-linux-amd64" | sha256sum | cut -d' ' -f1)
SHA_AARCH64=$(curl -fsSL "https://github.com/${REPO}/releases/download/${VERSION}/unifly-linux-arm64" | sha256sum | cut -d' ' -f1)

echo "  x86_64:  ${SHA_X86_64}"
echo "  aarch64: ${SHA_AARCH64}"

# Update PKGBUILD
cat > PKGBUILD << EOF
# Maintainer: Stefanie Jane <stef@hyperbliss.tech>
pkgname=unifly-bin
pkgver=${VERSION_NUM}
pkgrel=1
pkgdesc="CLI + TUI for managing UniFi network controllers"
arch=('x86_64' 'aarch64')
url="https://github.com/hyperb1iss/unifly"
license=('Apache-2.0')
provides=('unifly')
conflicts=('unifly')
depends=('gcc-libs' 'openssl' 'dbus')
install=unifly-bin.install

source_x86_64=("\${pkgname}-\${pkgver}-x86_64::https://github.com/hyperb1iss/unifly/releases/download/v\${pkgver}/unifly-linux-amd64")
source_aarch64=("\${pkgname}-\${pkgver}-aarch64::https://github.com/hyperb1iss/unifly/releases/download/v\${pkgver}/unifly-linux-arm64")

sha256sums_x86_64=('${SHA_X86_64}')
sha256sums_aarch64=('${SHA_AARCH64}')

package() {
    install -Dm755 "\${srcdir}/\${pkgname}-\${pkgver}-\${CARCH}" "\${pkgdir}/usr/bin/unifly"
}
EOF

# Update .SRCINFO
cat > .SRCINFO << EOF
pkgbase = unifly-bin
	pkgdesc = CLI + TUI for managing UniFi network controllers
	pkgver = ${VERSION_NUM}
	pkgrel = 1
	url = https://github.com/hyperb1iss/unifly
	install = unifly-bin.install
	arch = x86_64
	arch = aarch64
	license = Apache-2.0
	provides = unifly
	conflicts = unifly
	depends = gcc-libs
	depends = openssl
	depends = dbus
	source_x86_64 = unifly-bin-${VERSION_NUM}-x86_64::https://github.com/hyperb1iss/unifly/releases/download/v${VERSION_NUM}/unifly-linux-amd64
	source_aarch64 = unifly-bin-${VERSION_NUM}-aarch64::https://github.com/hyperb1iss/unifly/releases/download/v${VERSION_NUM}/unifly-linux-arm64
	sha256sums_x86_64 = ${SHA_X86_64}
	sha256sums_aarch64 = ${SHA_AARCH64}

pkgname = unifly-bin
EOF

echo ""
echo "Updated PKGBUILD and .SRCINFO for ${VERSION}"
echo ""
echo "Next steps:"
echo "  1. Clone the AUR repo (first time only):"
echo "     git clone ssh://aur@aur.archlinux.org/unifly-bin.git ~/dev/aur-unifly"
echo ""
echo "  2. Copy files and push:"
echo "     cp PKGBUILD .SRCINFO unifly-bin.install ~/dev/aur-unifly/"
echo "     cd ~/dev/aur-unifly"
echo "     git add PKGBUILD .SRCINFO unifly-bin.install"
echo "     git commit -m \"Update to ${VERSION_NUM}\""
echo "     git push"
echo ""
