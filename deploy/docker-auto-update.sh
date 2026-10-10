#!/usr/bin/env bash
set -Eeuo pipefail

COMPOSE_DIR="${SPM_CLOUD_COMPOSE_DIR:-/opt/spm-cloud}"
STATE_FILE="${SPM_CLOUD_UPDATE_STATE:-/var/lib/spm-cloud/last-docker-release-tag}"
REPOSITORY="sdf123098/spm-cloud"

case "$(uname -m)" in
  x86_64|amd64) platform="amd64" ;;
  aarch64|arm64) platform="arm64" ;;
  *) echo "Unsupported Docker host architecture: $(uname -m)" >&2; exit 1 ;;
esac

cd "$COMPOSE_DIR"
mkdir -p "$(dirname "$STATE_FILE")"
release_json="$(curl --fail --silent --show-error --location \
  -H 'Accept: application/vnd.github+json' \
  "https://api.github.com/repos/$REPOSITORY/releases/latest")"
tag="$(jq -er '.tag_name' <<<"$release_json")"
[[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "Invalid latest release tag: $tag" >&2; exit 1; }
previous_tag="$(cat "$STATE_FILE" 2>/dev/null || true)"
if [[ "$tag" == "$previous_tag" ]]; then
  exit 0
fi

asset="spm-cloud-docker-linux-$platform.tar.gz"
image="spm-cloud:$tag"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
release_url="https://github.com/$REPOSITORY/releases/download/$tag"
curl --fail --silent --show-error --location "$release_url/$asset" -o "$tmp/$asset"
curl --fail --silent --show-error --location "$release_url/$asset.sha256" -o "$tmp/$asset.sha256"
expected="$(awk '{print $1}' "$tmp/$asset.sha256")"
actual="$(sha256sum "$tmp/$asset" | awk '{print $1}')"
[[ "$expected" =~ ^[[:xdigit:]]{64}$ && "$actual" == "$expected" ]] || {
  echo "Checksum verification failed for $asset" >&2
  exit 1
}
docker load --input "$tmp/$asset"
docker image inspect "$image" >/dev/null

container_id="$(docker compose ps -q spm-cloud | head -n 1)"
previous_image=""
if [[ -n "$container_id" ]]; then
  previous_image="$(docker inspect --format '{{.Config.Image}}' "$container_id")"
fi
rollback() {
  if [[ -n "$previous_image" ]]; then
    SPM_CLOUD_IMAGE="$previous_image" docker compose up -d --no-deps spm-cloud || true
  fi
}

if ! SPM_CLOUD_IMAGE="$image" docker compose up -d --no-deps spm-cloud; then
  rollback
  exit 1
fi
sleep 8
if ! docker compose ps --status running --services | grep -Fxq spm-cloud; then
  rollback
  echo "Updated spm-cloud container did not remain running" >&2
  exit 1
fi
if ! actual_version="$(docker compose exec -T spm-cloud spm-cloud --version)"; then
  rollback
  echo "Unable to read the updated container version" >&2
  exit 1
fi
if [[ "$actual_version" != "spm-cloud ${tag#v}" ]]; then
  rollback
  echo "Updated container reports unexpected version: $actual_version" >&2
  exit 1
fi

printf '%s\n' "$tag" >"$STATE_FILE"
echo "Updated Docker spm-cloud to $tag using the verified prebuilt linux/$platform image"
