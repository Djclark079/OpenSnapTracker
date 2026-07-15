#!/usr/bin/env bash
set -euo pipefail

snap_root="${1:-$HOME/.steam/steam/steamapps/compatdata/1997040/pfx/drive_c/users/steamuser/AppData/LocalLow/Second Dinner/SNAP}"
interval="${OST_PROBE_INTERVAL_SECONDS:-1}"

if [[ ! -d "$snap_root" ]]; then
  echo "Snap root not found: $snap_root" >&2
  exit 1
fi

declare -A seen

fingerprint_file() {
  local file="$1"
  local stat_line hash
  stat_line="$(stat --printf '%Y:%s' "$file" 2>/dev/null || true)"
  [[ -n "$stat_line" ]] || return 0

  if [[ "${OST_PROBE_HASH:-1}" == "1" ]]; then
    hash="$(sha256sum "$file" 2>/dev/null | awk '{print substr($1, 1, 16)}')"
  else
    hash="hash-disabled"
  fi

  printf '%s:%s' "$stat_line" "$hash"
}

echo "[snap-file-activity-probe] watching $snap_root"
echo "[snap-file-activity-probe] interval=${interval}s hash=${OST_PROBE_HASH:-1}"
echo "[snap-file-activity-probe] press Ctrl+C to stop"

while true; do
  while IFS= read -r -d '' file; do
    fingerprint="$(fingerprint_file "$file")"
    [[ -n "$fingerprint" ]] || continue

    previous="${seen[$file]:-}"
    if [[ -z "$previous" ]]; then
      seen[$file]="$fingerprint"
      continue
    fi

    if [[ "$previous" != "$fingerprint" ]]; then
      seen[$file]="$fingerprint"
      printf '%(%Y-%m-%dT%H:%M:%S%z)T changed %s %s\n' -1 "$fingerprint" "$file"
    fi
  done < <(
    find "$snap_root" -type f \
      ! -path '*/Cache/*' \
      ! -path '*/GPUCache/*' \
      ! -path '*/ShaderCache/*' \
      ! -path '*/Unity/*' \
      -print0 2>/dev/null
  )

  sleep "$interval"
done
