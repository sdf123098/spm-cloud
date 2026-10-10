#!/usr/bin/env bash
set -Eeuo pipefail

COMPOSE_DIR="${SPM_CLOUD_COMPOSE_DIR:-/opt/spm-cloud}"
STATE_FILE="${SPM_CLOUD_UPDATE_STATE:-/var/lib/spm-cloud/last-release-tag}"
REPOSITORY="sdf123098/spm-cloud"

cd "$COMPOSE_DIR"
mkdir -p "$(dirname "$STATE_FILE")"

release_json="$(curl --fail --silent --show-error --location \
  -H 'Accept: application/vnd.github+json' \
  "https://api.github.com/repos/$REPOSITORY/releases/latest")"
tag="$(jq -er '.tag_name' <<<"$release_json")"
[[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "Invalid latest release tag: $tag" >&2; exit 1; }
previous="$(cat "$STATE_FILE" 2>/dev/null || true)"
if [[ "$tag" == "$previous" ]]; then
  exit 0
fi

version="${tag#v}"
image="spm-cloud:$tag"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
archive="$tmp/source.tar.gz"
curl --fail --silent --show-error --location \
  "https://github.com/$REPOSITORY/archive/refs/tags/$tag.tar.gz" -o "$archive"
mkdir "$tmp/source"
tar -xzf "$archive" --strip-components=1 -C "$tmp/source"
docker build --pull --tag "$image" "$tmp/source"
actual="$(docker run --rm "$image" --version)"
[[ "$actual" == "spm-cloud $version" ]] || { echo "Built image version mismatch: $actual" >&2; exit 1; }

if ! SPM_CLOUD_IMAGE="$image" docker compose up -d --no-deps spm-cloud; then
  if [[ -n "$previous" ]]; then
    SPM_CLOUD_IMAGE="spm-cloud:$previous" docker compose up -d --no-deps spm-cloud || true
  fi
  exit 1
fi
sleep 8
if ! docker compose ps --status running --services | grep -Fxq spm-cloud; then
  if [[ -n "$previous" ]]; then
    SPM_CLOUD_IMAGE="spm-cloud:$previous" docker compose up -d --no-deps spm-cloud || true
  fi
  echo "Updated spm-cloud container did not remain running" >&2
  exit 1
fi
printf '%s\n' "$tag" >"$STATE_FILE"
echo "Updated spm-cloud to $tag"
