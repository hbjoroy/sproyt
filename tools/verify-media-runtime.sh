#!/usr/bin/env bash
set -euo pipefail
# This runs native CI container tools. No emulated Rust or target binary.
# Check ARM64 package signature verification and libraries without executing
# target binaries. /etc/apk/keys only contains the native architecture's keys.
docker build --target media-runtime-builder --build-arg TARGETARCH=arm64 -t local/sproyt-media-runtime-arm64 .
docker run --rm --entrypoint /bin/sh local/sproyt-media-runtime-arm64 -ec '
  test -x /worker-root/usr/bin/vipsthumbnail
  test -f /worker-root/usr/lib/libvips.so.42
  test -f /worker-root/etc/os-release
  test -f /worker-root/etc/alpine-release
  grep -q "^ID=alpine$" /worker-root/etc/os-release
  grep -q "^3\.24\." /worker-root/etc/alpine-release
  apk --root /worker-root --no-network info -e "glib=2.90.0-r0"
  test -n "$(find /worker-root/usr/lib -name "*heif*.so*" -print -quit)"
'
docker build --target media-runtime-builder --build-arg TARGETARCH=amd64 -t local/sproyt-media-runtime .
docker run --rm --memory=512m --cpus=1 --entrypoint /bin/sh local/sproyt-media-runtime -ec '
  apk --root /worker-root --no-network info -e "glib=2.90.0-r0"
  mkdir -p /worker-root/tmp
  chroot /worker-root /usr/bin/vips black /tmp/large.v 10000 10000 --bands 3
  chroot /worker-root /usr/bin/vips jpegsave /tmp/large.v /tmp/original.jpg
  before=$(sha256sum /worker-root/tmp/original.jpg)
  chroot /worker-root /usr/bin/vipsthumbnail /tmp/original.jpg --size 720x720 --output-profile srgb --output "/tmp/preview.jpg[Q=85,keep=none,optimize_coding]"
  test "$(chroot /worker-root /usr/bin/vipsheader -f width /tmp/preview.jpg)" = 720
  test "$(chroot /worker-root /usr/bin/vipsheader -f height /tmp/preview.jpg)" = 720
  test "$before" = "$(sha256sum /worker-root/tmp/original.jpg)"
  test "$(wc -c < /worker-root/tmp/preview.jpg)" -lt 4194304
  chroot /worker-root /usr/bin/vips black /tmp/portrait.v 1200 800 --bands 3
  for format in png webp heic; do
    if test "$format" = heic; then
      # Alpine splits HEIC encoding from decoding. Install the encoder only to
      # create the fixture, then remove it before testing the shipped decoder.
      apk --root /worker-root --arch x86_64 --no-scripts --no-cache \
        --repository https://dl-cdn.alpinelinux.org/alpine/v3.24/main \
        --repository https://dl-cdn.alpinelinux.org/alpine/v3.24/community \
        add libheif-x265=1.23.4-r0
    fi
    chroot /worker-root /usr/bin/vips copy /tmp/portrait.v /tmp/sample.$format
    if test "$format" = heic; then
      apk --root /worker-root --no-scripts --no-network del libheif-x265
    fi
    original_hash=$(sha256sum /worker-root/tmp/sample.$format)
    chroot /worker-root /usr/bin/vipsthumbnail /tmp/sample.$format --size 720x720 --output-profile srgb --output "/tmp/$format.jpg[Q=85,keep=none]"
    test "$(chroot /worker-root /usr/bin/vipsheader -f width /tmp/$format.jpg)" = 720
    test "$(chroot /worker-root /usr/bin/vipsheader -f height /tmp/$format.jpg)" = 480
    test "$original_hash" = "$(sha256sum /worker-root/tmp/sample.$format)"
  done
  chroot /worker-root /usr/bin/vips copy /tmp/portrait.v /tmp/plain.jpg
  # Inject a tiny standard EXIF APP1 record, orientation 6, after JPEG SOI.
  head -c 2 /worker-root/tmp/plain.jpg > /worker-root/tmp/rotated.jpg
  printf "%s" "/+EAIkV4aWYAAElJKgAIAAAAAQASAQMAAQAAAAYAAAAAAAAA" | base64 -d >> /worker-root/tmp/rotated.jpg
  tail -c +3 /worker-root/tmp/plain.jpg >> /worker-root/tmp/rotated.jpg
  original_hash=$(sha256sum /worker-root/tmp/rotated.jpg)
  chroot /worker-root /usr/bin/vipsthumbnail /tmp/rotated.jpg --size 720x720 --output-profile srgb --output "/tmp/rotated-preview.jpg[Q=85,keep=none]"
  test "$(chroot /worker-root /usr/bin/vipsheader -f width /tmp/rotated-preview.jpg)" = 480
  test "$(chroot /worker-root /usr/bin/vipsheader -f height /tmp/rotated-preview.jpg)" = 720
  test "$original_hash" = "$(sha256sum /worker-root/tmp/rotated.jpg)"
  printf "not an image" >/worker-root/tmp/broken
  if chroot /worker-root /usr/bin/vipsthumbnail /tmp/broken --output /tmp/bad.jpg; then exit 1; fi
'
