#!/bin/bash
# Builds the ffmpeg sidecar for this Mac into src-tauri/binaries, where Tauri's
# externalBin expects it. Static and LGPL, with only what conversion needs. Homebrew's
# build can't be bundled, as it links dozens of libraries outside the app.
set -euo pipefail

# Keep licenses/ffmpeg-NOTICE.txt in step with these.
VERSION=8.0.1
SHA256=05ee0b03119b45c0bdb4df654b96802e909e0a752f72e4fe3794f487229e5a41

cd "$(dirname "$0")/.."
TRIPLE="$(uname -m | sed 's/arm64/aarch64/')-apple-darwin"
OUT="$PWD/src-tauri/binaries/ffmpeg-$TRIPLE"
if [ -x "$OUT" ]; then
  echo "already built: $OUT"
  exit 0
fi

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
curl -fsSL -o "$WORK/ffmpeg.tar.xz" "https://ffmpeg.org/releases/ffmpeg-$VERSION.tar.xz"
echo "$SHA256  $WORK/ffmpeg.tar.xz" | shasum -a 256 -c -
tar -xf "$WORK/ffmpeg.tar.xz" -C "$WORK"
cd "$WORK/ffmpeg-$VERSION"

./configure \
  --disable-everything --disable-autodetect --disable-network --disable-doc \
  --disable-ffplay --disable-ffprobe --disable-avdevice \
  --enable-static --disable-shared \
  --enable-protocol=file,pipe \
  --enable-demuxer=aiff,flac \
  --enable-muxer=flac,md5 \
  --enable-decoder=flac,pcm_s8,pcm_s16be,pcm_s16le,pcm_s24be,pcm_s32be,pcm_f32be,pcm_f64be,pcm_alaw,pcm_mulaw \
  --enable-encoder=flac,pcm_s32le \
  --enable-filter=aresample,aformat,anull
make -j"$(sysctl -n hw.ncpu)" ffmpeg

mkdir -p "$(dirname "$OUT")"
strip ffmpeg
cp ffmpeg "$OUT"
echo "built: $OUT"
