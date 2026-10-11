#!/usr/bin/env bash
set -euo pipefail
# This runs native CI container tools. No emulated Rust or target binary.
docker build --target media-runtime-builder --build-arg TARGETARCH=amd64 -t local/sproyt-media-runtime .
docker run --rm --memory=512m --cpus=1 --entrypoint /bin/sh local/sproyt-media-runtime -ec '
  mkdir -p /worker-root/tmp
  chroot /worker-root /usr/bin/vips black /tmp/large.v 10000 10000 --bands 3
  chroot /worker-root /usr/bin/vips jpegsave /tmp/large.v /tmp/original.jpg
  before=$(sha256sum /worker-root/tmp/original.jpg)
  chroot /worker-root /usr/bin/vipsthumbnail /tmp/original.jpg --size 720x720 --output-profile srgb --output "/tmp/preview.jpg[Q=85,keep=none,optimize_coding]"
  test "$(chroot /worker-root /usr/bin/vipsheader -f width /tmp/preview.jpg)" = 720
  test "$(chroot /worker-root /usr/bin/vipsheader -f height /tmp/preview.jpg)" = 720
  test "$before" = "$(sha256sum /worker-root/tmp/original.jpg)"
  test "$(wc -c < /worker-root/tmp/preview.jpg)" -lt 4194304
  printf "not an image" >/worker-root/tmp/broken
  if chroot /worker-root /usr/bin/vipsthumbnail /tmp/broken --output /tmp/bad.jpg; then exit 1; fi
'
