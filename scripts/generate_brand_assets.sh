#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd "$script_dir/.." && pwd)"
brand_dir="$project_dir/assets/brand"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/corbit-desktop-brand.XXXXXX")"

cleanup() {
  rm -rf "$work_dir"
}
trap cleanup EXIT

for required_command in xcrun sips iconutil; do
  if ! command -v "$required_command" >/dev/null 2>&1; then
    echo "Missing required command: $required_command" >&2
    exit 1
  fi
done

xcrun swiftc "$script_dir/render-svg.swift" -o "$work_dir/render-svg"

render_svg() {
  local source_file="$1"
  local output_file="$2"
  local render_size="${3:-1024}"
  "$work_dir/render-svg" "$source_file" "$output_file" "$render_size"
}

generate_icns() {
  local source_file="$1"
  local output_file="$2"
  local iconset_name
  iconset_name="$(basename "$output_file" .icns).iconset"
  local iconset_dir="$work_dir/$iconset_name"

  mkdir -p "$iconset_dir"
  resize_png "$source_file" 16 "$iconset_dir/icon_16x16.png"
  resize_png "$source_file" 32 "$iconset_dir/icon_16x16@2x.png"
  resize_png "$source_file" 32 "$iconset_dir/icon_32x32.png"
  resize_png "$source_file" 64 "$iconset_dir/icon_32x32@2x.png"
  resize_png "$source_file" 128 "$iconset_dir/icon_128x128.png"
  resize_png "$source_file" 256 "$iconset_dir/icon_128x128@2x.png"
  resize_png "$source_file" 256 "$iconset_dir/icon_256x256.png"
  resize_png "$source_file" 512 "$iconset_dir/icon_256x256@2x.png"
  resize_png "$source_file" 512 "$iconset_dir/icon_512x512.png"
  resize_png "$source_file" 1024 "$iconset_dir/icon_512x512@2x.png"
  iconutil -c icns "$iconset_dir" -o "$output_file"
}

resize_png() {
  local source_file="$1"
  local size="$2"
  local output_file="$3"

  sips -z "$size" "$size" "$source_file" --out "$output_file" >/dev/null
}

render_svg "$brand_dir/corbit-mark.svg" "$work_dir/corbit-mark-1024.png"
resize_png \
  "$work_dir/corbit-mark-1024.png" \
  512 \
  "$brand_dir/corbit-mark-512.png"

render_svg "$brand_dir/corbit-symbol-light.svg" "$work_dir/corbit-symbol-light-1024.png"
resize_png \
  "$work_dir/corbit-symbol-light-1024.png" \
  512 \
  "$brand_dir/corbit-symbol-light-512.png"

render_svg "$brand_dir/corbit-symbol-dark.svg" "$work_dir/corbit-symbol-dark-1024.png"
resize_png \
  "$work_dir/corbit-symbol-dark-1024.png" \
  512 \
  "$brand_dir/corbit-symbol-dark-512.png"

render_svg "$brand_dir/corbit-app-icon.svg" "$brand_dir/corbit-app-icon-1024.png"
render_svg "$brand_dir/corbit-app-icon-dark.svg" "$brand_dir/corbit-app-icon-dark-1024.png"
render_svg "$brand_dir/corbit-brand-preview.svg" "$brand_dir/corbit-brand-preview.png" 1800

generate_icns "$brand_dir/corbit-app-icon-1024.png" "$brand_dir/corbit.icns"
generate_icns "$brand_dir/corbit-app-icon-dark-1024.png" "$brand_dir/corbit-dark.icns"

resize_png "$brand_dir/corbit-app-icon-1024.png" 256 "$work_dir/corbit-app-icon-256.png"
sips \
  -s format ico \
  "$work_dir/corbit-app-icon-256.png" \
  --out "$brand_dir/corbit.ico" \
  >/dev/null

echo "Generated Corbit desktop brand assets in $brand_dir"
