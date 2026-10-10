#!/usr/bin/env bash
set -Eeuo pipefail

REPOSITORY="sdf123098/spm-cloud"
SERVICE="${SPM_CLOUD_SYSTEMD_SERVICE:-spm-cloud.service}"
BINARY="${SPM_CLOUD_BINARY:-/usr/local/bin/spm-cloud}"
STATE_DIR="${SPM_CLOUD_UPDATE_STATE_DIR:-/var/lib/spm-cloud/updates}"
HEALTH_URL="${SPM_CLOUD_AUTO_UPDATE_HEALTH_URL:-http://127.0.0.1:8787/health}"

case "$(uname -m)" in
  x86_64|amd64) target="x86_64-unknown-linux-gnu" ;;
  aarch64|arm64) target="aarch64-unknown-linux-gnu" ;;
  *) echo "Unsupported native Linux architecture: $(uname -m)" >&2; exit 1 ;;
esac

release_json="$(curl --fail --silent --show-error --location \
  -H 'Accept: application/vnd.github+json' \
  "https://api.github.com/repos/$REPOSITORY/releases/latest")"
tag="$(jq -er '.tag_name' <<<"$release_json")"
[[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "Invalid latest release tag: $tag" >&2; exit 1; }
version="${tag#v}"
current_version="$("$BINARY" --version 2>/dev/null || true)"
[[ "$current_version" == "spm-cloud $version" ]] && exit 0
systemctl is-active --quiet "$SERVICE" || { echo "$SERVICE is not active; refusing a background replacement" >&2; exit 1; }

mkdir -p "$STATE_DIR"
tmp="$(mktemp -d "$STATE_DIR/.update.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT
archive_name="spm-cloud-$target.tar.xz"
archive="$tmp/$archive_name"
checksum="$archive.sha256"
release_url="https://github.com/$REPOSITORY/releases/download/$tag"
curl --fail --silent --show-error --location "$release_url/$archive_name" -o "$archive"
curl --fail --silent --show-error --location "$release_url/$archive_name.sha256" -o "$checksum"
expected="$(awk '{print $1}' "$checksum")"
actual="$(sha256sum "$archive" | awk '{print $1}')"
[[ "$expected" =~ ^[[:xdigit:]]{64}$ && "$actual" == "$expected" ]] || {
  echo "Checksum verification failed for $archive_name" >&2
  exit 1
}

mkdir "$tmp/unpacked"
tar -xJf "$archive" -C "$tmp/unpacked"
new_binary="$tmp/unpacked/spm-cloud"
[[ -x "$new_binary" ]] || { echo "Release archive has no executable spm-cloud" >&2; exit 1; }
actual_version="$("$new_binary" --version)"
[[ "$actual_version" == "spm-cloud $version" ]] || {
  echo "Release binary reports $actual_version; expected spm-cloud $version" >&2
  exit 1
}

backup="$STATE_DIR/spm-cloud.previous"
staged="$BINARY.new.$$"
install -m 0755 "$new_binary" "$staged"
cp -p "$BINARY" "$backup"
if ! mv -f "$staged" "$BINARY" || ! systemctl restart "$SERVICE"; then
  install -m 0755 "$backup" "$BINARY"
  systemctl restart "$SERVICE" || true
  exit 1
fi

healthy=false
for _ in $(seq 1 30); do
  if systemctl is-active --quiet "$SERVICE" && curl --fail --silent "$HEALTH_URL" >/dev/null; then
    healthy=true
    break
  fi
  sleep 1
done
if [[ "$healthy" != true ]]; then
  echo "Updated service failed its health check; restoring the previous binary" >&2
  systemctl stop "$SERVICE" || true
  install -m 0755 "$backup" "$BINARY"
  systemctl restart "$SERVICE" || true
  exit 1
fi

printf '%s\n' "$tag" >"$STATE_DIR/last-release-tag"
echo "Updated native spm-cloud to $tag from the verified prebuilt $target archive"
