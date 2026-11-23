#!/bin/sh
# Build the upstream preview codec for the target ABI without executing target code.
set -eu
case "$1" in
    arm64) arch=aarch64 ;;
    amd64) arch=x86_64 ;;
    *) echo "Unsupported media architecture: $1" >&2; exit 1 ;;
esac
version=8.18.2
# SHA-512 from Alpine 3.24 community/vips/APKBUILD for the upstream release.
sum=202ea484f776d95792339b6d140cd0d8c248c81895a3421896fe5448f31460eefba7273477b66ee8b15538d6ae61a0254cdd84427b1e47a39309e68c5f250d29
apk add --no-cache alpine-keys=2.6-r0 binutils clang lld meson ninja pkgconf glib-dev openssl curl xz patchelf
for root in /sysroot /worker-root; do
    mkdir -p "$root/etc/apk/keys"
    cp /usr/share/apk/keys/"$arch"/*.pub "$root/etc/apk/keys/"
    cp /etc/apk/repositories "$root/etc/apk/"
done
apk --root /sysroot --arch "$arch" --initdb --no-scripts --no-cache add \
    g++ musl-dev glib-dev expat-dev libexif-dev lcms2-dev libjpeg-turbo-dev \
    libpng-dev libwebp-dev libheif-dev zlib-dev
cat >/tmp/media-cross.ini <<EOF
[binaries]
c = ['clang', '--target=$arch-alpine-linux-musl', '--sysroot=/sysroot', '--gcc-toolchain=/sysroot/usr']
cpp = ['clang++', '--target=$arch-alpine-linux-musl', '--sysroot=/sysroot', '--gcc-toolchain=/sysroot/usr']
pkg-config = 'pkgconf'
[built-in options]
c_link_args = ['-fuse-ld=lld']
cpp_link_args = ['-fuse-ld=lld']
[properties]
sys_root = '/sysroot'
pkg_config_libdir = ['/sysroot/usr/lib/pkgconfig', '/sysroot/usr/share/pkgconfig']
needs_exe_wrapper = true
[host_machine]
system = 'linux'
cpu_family = '$arch'
cpu = '$arch'
endian = 'little'
EOF
curl -fsSL --retry 3 -o "/tmp/vips-$version.tar.xz" "https://github.com/libvips/libvips/releases/download/v$version/vips-$version.tar.xz"
echo "$sum  /tmp/vips-$version.tar.xz" | sha512sum -c -
tar -xJf "/tmp/vips-$version.tar.xz" -C /tmp
meson setup /tmp/vips-build "/tmp/vips-$version" --cross-file=/tmp/media-cross.ini \
    --prefix=/usr --libdir=lib --buildtype=release --wrap-mode=nodownload \
    -Dnsgif=true -Dauto_features=disabled -Dcplusplus=false -Dexamples=false \
    -Djpeg=enabled -Dpng=enabled -Dwebp=enabled -Dheif=enabled \
    -Dlcms=enabled -Dexif=enabled -Dzlib=enabled
ninja -C /tmp/vips-build -j 4 tools/vips tools/vipsheader tools/vipsthumbnail

# Ship a real APK so the runtime and its SBOM preserve upstream package identity.
mkdir -p /tmp/media-apk/usr/bin /tmp/media-apk/usr/lib
for tool in vips vipsheader vipsthumbnail; do
    cp "/tmp/vips-build/tools/$tool" /tmp/media-apk/usr/bin/
done
cp -L /tmp/vips-build/libvips/libvips.so.42 /tmp/media-apk/usr/lib/libvips.so.42
# Derive APK shared-library dependencies from the actual target ELF files.
# Remove build-tree search paths before signing and packaging.
find /tmp/media-apk -type f -exec patchelf --remove-rpath {} +
needed=$(find /tmp/media-apk -type f -exec readelf -d {} + |
    sed -n 's/.*(NEEDED).*\[\([^]]*\)\].*/\1/p' | sort -u |
    grep -vx 'libvips.so.42' | sed 's/^/so:/' | tr '\n' ' ')
test -n "$needed"
install -Dm644 "/tmp/vips-$version/LICENSE" /tmp/media-apk/usr/share/licenses/vips/LICENSE
openssl genrsa -out /tmp/media.rsa 2048
openssl rsa -in /tmp/media.rsa -pubout -out /worker-root/etc/apk/keys/media.rsa.pub
apk mkpkg --sign-key /tmp/media.rsa --files /tmp/media-apk --output /tmp/vips-preview.apk \
    -I name:vips -I "version:$version-r0" -I "arch:$arch" -I license:LGPL-2.1-or-later \
    -I origin:vips -I url:https://www.libvips.org/ -I 'description:Sproyt build of the libvips preview codecs' \
    -I "depends:$needed"
printf '%s\n' '@glibfix https://dl-cdn.alpinelinux.org/alpine/edge/main' >>/worker-root/etc/apk/repositories
apk --root /worker-root --arch "$arch" --initdb --no-scripts --no-cache add \
    /tmp/vips-preview.apk 'glib@glibfix=2.90.1-r0' 'libexpat=2.10.0-r0' 'zlib=1.4-r0'
rm /tmp/media.rsa /worker-root/etc/apk/keys/media.rsa.pub
cp -L /etc/os-release /worker-root/etc/os-release
cp -L /etc/alpine-release /worker-root/etc/alpine-release
if grep -Eq '^P:(libcrypto3|libssl3|libxml2)$' /worker-root/lib/apk/db/installed; then
    echo 'Unexpected TLS or XML dependency in preview runtime' >&2
    exit 1
fi
