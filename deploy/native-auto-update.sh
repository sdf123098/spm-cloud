#!/usr/bin/env bash
# spm-cloud native updater (hardened)
#
# 设计目标: 无论出现什么错误, 要么升级成功, 要么保持/恢复到一个可运行的版本。
# 绝不留下"服务停机 + 坏二进制"的终态。
#
# 与官方脚本的差异见同目录 README 或提交信息。
set -Eeuo pipefail

REPOSITORY="${SPM_CLOUD_REPOSITORY:-sdf123098/spm-cloud}"
SERVICE="${SPM_CLOUD_SYSTEMD_SERVICE:-spm-cloud.service}"
BINARY="${SPM_CLOUD_BINARY:-/usr/local/bin/spm-cloud}"
STATE_DIR="${SPM_CLOUD_UPDATE_STATE_DIR:-/var/lib/spm-cloud/updates}"
HEALTH_URL="${SPM_CLOUD_AUTO_UPDATE_HEALTH_URL:-http://127.0.0.1:8787/health}"
CONFIG="${SPM_CLOUD_CONFIG:-/etc/spm-cloud/config.json}"
LOG="${STATE_DIR}/update.log"
KEEP="${SPM_CLOUD_UPDATE_KEEP:-5}"          # 保留多少个已知良好版本
MIN_FREE_MB="${SPM_CLOUD_UPDATE_MIN_FREE_MB:-512}"

mkdir -p "$STATE_DIR"
touch "$LOG" 2>/dev/null || true

log()  { printf '%s [%s] %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$1" "${*:2}" | tee -a "$LOG" >&2; }
info() { log INFO "$@"; }
warn() { log WARN "$@"; }
err()  { log ERROR "$@"; }
die()  { err "$@"; exit 1; }

# 无论以何种方式退出, 都要保证服务处于运行状态。
finalize() {
  local rc=$?
  # 绝不改写退出码: 上层(timer/on-call)依赖它判断成败
  if ! systemctl is-active --quiet "$SERVICE"; then
    warn "服务在退出时未运行 (rc=$rc)，尝试拉起当前二进制"
    systemctl start "$SERVICE" || warn "拉起失败，需人工介入: systemctl status $SERVICE"
  fi
  return $rc
}
trap finalize EXIT

# ---------------------------------------------------------------- 前置检查 ----
for c in curl jq sha256sum tar xz flock install systemctl; do
  command -v "$c" >/dev/null 2>&1 || die "缺少依赖命令: $c"
done

# 磁盘空间: 需要容纳压缩包(约45MB)+解包(约100MB)+备份(约100MB)
avail_mb=$(df -Pm "$STATE_DIR" | awk 'NR==2{print $4}')
if [[ -n "$avail_mb" && "$avail_mb" -lt "$MIN_FREE_MB" ]]; then
  die "磁盘可用空间不足: ${avail_mb}MB < ${MIN_FREE_MB}MB"
fi

# 并发保护 (timer 与手动触发可能同时发生)
exec 9>"/run/lock/spm-cloud-native-update.lock" 2>/dev/null || \
  exec 9>"${STATE_DIR}/.lock"
flock -n 9 || { info "另一次更新正在进行，跳过本次"; exit 0; }

case "$(uname -m)" in
  x86_64|amd64)  target="x86_64-unknown-linux-gnu" ;;
  aarch64|arm64) target="aarch64-unknown-linux-gnu" ;;
  *) die "不支持的原生 Linux 架构: $(uname -m)" ;;
esac

# ---------------------------------------------------------------- 版本比较 ----
# 数字比较, 避免把"已是最新"误判为需要更新, 也避免降级 (e.g. 手动装了更高版本)
ver_num() { echo "$1" | awk -F. '{printf "%d%06d%06d", $1, $2, $3}'; }

# 服务"真的健康"= 进程活跃 + health 返回 ok。用于区分"已是最新"与"停在坏版本"。
service_healthy() {
  systemctl is-active --quiet "$SERVICE" || return 1
  local body
  body=$(curl --fail --silent --max-time 5 "$HEALTH_URL" 2>/dev/null) || return 1
  grep -q '"ok":true' <<<"$body" || return 1
  return 0
}

# ---------------------------------------------------------------- 查询发布 ----
# 用 ETag 条件请求, 304 不消耗 GitHub 匿名 60/h 配额
ETAG_FILE="${STATE_DIR}/.release.etag"
api="https://api.github.com/repos/$REPOSITORY/releases/latest"
etag_args=()
[[ -s "$ETAG_FILE" ]] && etag_args=(-H "If-None-Match: $(cat "$ETAG_FILE")")

tmp_hdr=$(mktemp)
release_json=""
set +e
release_json=$(curl --fail --silent --show-error --location \
  --retry 5 --retry-delay 3 --retry-all-errors --connect-timeout 15 --max-time 120 \
  -H 'Accept: application/vnd.github+json' \
  "${etag_args[@]}" -D "$tmp_hdr" "$api" 2>/dev/null)
curl_rc=$?
set -e

if grep -qiE '^HTTP/[0-9.]+ 304' "$tmp_hdr"; then
  rm -f "$tmp_hdr"
  # 304 只说明"远端没变", 不说明"本地已就绪"。
  # 用缓存发布的 tag 比较；手动升级/回退不会更新 .synced-version。
  release_json="$(cat "${STATE_DIR}/.release.json" 2>/dev/null || true)"
  cached_tag="$(jq -er '.tag_name' <<<"$release_json" 2>/dev/null || true)"
  cached_version="${cached_tag#v}"
  local_ver="$("$BINARY" --version 2>/dev/null | awk '{print $2}' || true)"
  # 304 只说明远端未变; 本地是否可用必须独立确认 (版本号可能来自损坏的二进制)
  if [[ "$cached_tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ && -n "$local_ver" ]] \
     && [[ "$(ver_num "$local_ver")" -ge "$(ver_num "$cached_version")" ]] \
     && service_healthy; then
    info "远端无变化 (304) 且本地 $local_ver 服务健康，无需处理"; exit 0
  fi
  warn "远端无变化 (304) 但本地为 ${local_ver:-未知}，版本或服务健康状态仍需处理，继续执行修复性升级"
  # 用缓存的上次响应继续走正常流程
  if [[ -z "$release_json" ]]; then
    warn "无缓存的发布信息，清除 ETag 以便下轮全量查询"
    rm -f "$ETAG_FILE"
    exit 0
  fi
else
  rm -f "$tmp_hdr"
  [[ -n "$release_json" ]] && printf '%s' "$release_json" > "${STATE_DIR}/.release.json"
fi

if [[ $curl_rc -ne 0 ]]; then
  # 限流或网络故障: 不作为失败退出, 等下一个周期
  warn "查询 GitHub 失败 (curl rc=$curl_rc)，本轮跳过"; exit 0
fi

tag=$(jq -er '.tag_name' <<<"$release_json" 2>/dev/null) || die "响应中没有 tag_name"
if [[ ! "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  warn "最新 tag 不是稳定正式版 ($tag)，跳过"; exit 0
fi
version="${tag#v}"

# 记录 ETag 以便下次 304
curl --silent --location --max-time 30 \
  -H 'Accept: application/vnd.github+json' -D - -o /dev/null "$api" 2>/dev/null \
  | awk 'BEGIN{IGNORECASE=1} /^etag:/{print $2}' | tr -d '\r' > "$ETAG_FILE" || true

current_version=""
if [[ -x "$BINARY" ]]; then
  current_version="$("$BINARY" --version 2>/dev/null | awk '{print $2}' || true)"
fi

# 判断是否需要(重新)安装。版本号不足以证明运行态健康: 一个损坏或伪造的二进制
# 也能自报任意版本号, 所以"版本已最新"还必须叠加"服务确实健康"才算真的不需要动。
need_update=0
if [[ -z "$current_version" ]]; then
  warn "无法读取当前二进制版本 (可能已损坏)，执行修复性升级"
  need_update=1
elif [[ "$(ver_num "$current_version")" -lt "$(ver_num "$version")" ]]; then
  info "发现新版本: $current_version -> $version"
  need_update=1
else
  # 版本已 >= 远端, 但需确认服务真的在正常提供能力
  if service_healthy; then
    info "已是最新且服务健康 (当前 $current_version >= 远端 $version)"; exit 0
  fi
  warn "版本为 $current_version 但服务不健康，执行修复性重装 (目标 $version)"
  need_update=1
fi
[[ $need_update -eq 1 ]] || exit 0

# 注意: 此处不再因服务不活跃而拒绝 —— 服务已挂时正是最需要升级恢复的场景。
if ! systemctl is-active --quiet "$SERVICE"; then
  warn "$SERVICE 当前未运行；仍继续(修复性)升级"
fi

# ---------------------------------------------------------------- 下载校验 ----
tmp=$(mktemp -d "$STATE_DIR/.update.XXXXXX")
keep_tmp=0
cleanup() { [[ $keep_tmp -eq 1 ]] || rm -rf "$tmp"; }
trap 'cleanup; finalize' EXIT

archive_name="spm-cloud-$target.tar.xz"
archive="$tmp/$archive_name"
checksum="$archive.sha256"
release_url="https://github.com/$REPOSITORY/releases/download/$tag"

info "开始升级 $current_version -> $version (target=$target)"

dl() { # dl <url> <out> <maxbytes>
  curl --fail --silent --show-error --location \
    --retry 5 --retry-delay 3 --retry-all-errors \
    --connect-timeout 15 --max-time 900 \
    "$1" -o "$2" || return 1
  local sz; sz=$(stat -c%s "$2" 2>/dev/null || echo 0)
  [[ "$sz" -gt 0 && "$sz" -le "$3" ]] || return 1
  return 0
}

dl "$release_url/$archive_name" "$archive" $((256*1024*1024)) || die "下载更新包失败"
dl "$release_url/$archive_name.sha256" "$checksum" 4096 || die "下载校验文件失败"

expected=$(awk '{print $1}' "$checksum" | tr 'A-F' 'a-f')
actual=$(sha256sum "$archive" | awk '{print $1}')
if [[ ! "$expected" =~ ^[0-9a-f]{64}$ ]]; then die "校验文件格式非法"; fi
if [[ "$actual" != "$expected" ]]; then die "校验和不匹配 (期望 $expected, 实际 $actual)"; fi
info "校验和通过"

# ---------------------------------------------------------------- 解包验证 ----
mkdir "$tmp/unpacked"
tar -xJf "$archive" -C "$tmp/unpacked" || die "解包失败"
# 发布包内含一层目标目录，按文件名查找而非硬编码路径
new_binary="$(find "$tmp/unpacked" -type f -name spm-cloud -print -quit)"
[[ -n "$new_binary" ]] || die "更新包内没有 spm-cloud 可执行文件"
chmod 0755 "$new_binary"

reported="$("$new_binary" --version 2>/dev/null | awk '{print $2}' || true)"
if [[ "$reported" != "$version" ]]; then
  die "新二进制自报版本 $reported，与期望 $version 不符"
fi
info "新二进制版本确认: $reported"

# 配置兼容性预检: 新版本能接受现有配置才替换 (避免装上后才炸)
if [[ -f "$CONFIG" ]]; then
  if "$new_binary" --config "$CONFIG" --check-config >/dev/null 2>&1; then
    info "配置兼容性预检通过"
  else
    die "新版本拒绝现有配置 ($CONFIG)，已中止升级以免服务无法启动"
  fi
else
  warn "未找到配置 $CONFIG，跳过兼容性预检"
fi

# ------------------------------------------------- 备份已知良好版本 (轮转) ----
# 关键: 备份的是"当前正在运行的、已经证明可用"的二进制，且只在其可执行时记录
KNOWN_DIR="$STATE_DIR/known-good"
mkdir -p "$KNOWN_DIR"
if [[ -x "$BINARY" ]] && "$BINARY" --version >/dev/null 2>&1; then
  cp -p "$BINARY" "$KNOWN_DIR/spm-cloud.${current_version:-unknown}"
  # 轮转: 只保留最近 KEEP 个
  ls -1t "$KNOWN_DIR"/spm-cloud.* 2>/dev/null | tail -n +$((KEEP+1)) | xargs -r rm -f
  info "已保存已知良好版本到 $KNOWN_DIR/spm-cloud.${current_version:-unknown}"
else
  warn "当前二进制不可执行，未纳入已知良好集合"
fi

# 稳定的回滚点: 优先用当前可执行版本, 否则用最近一个已知良好版本
ROLLBACK=""
if [[ -x "$BINARY" ]] && "$BINARY" --version >/dev/null 2>&1; then
  ROLLBACK="$tmp/rollback.bin"; cp -p "$BINARY" "$ROLLBACK"
else
  ROLLBACK="$(ls -1t "$KNOWN_DIR"/spm-cloud.* 2>/dev/null | head -n1 || true)"
  [[ -n "$ROLLBACK" ]] && warn "当前二进制不可用，回滚点改用 $ROLLBACK"
fi
[[ -n "$ROLLBACK" ]] || warn "没有可用回滚点；若升级失败只能依赖后续人工处理"

# ---------------------------------------------------------------- 原子替换 ----
staged="$BINARY.new.$$"
install -m 0755 "$new_binary" "$staged" || die "写入暂存二进制失败"

restore_and_exit() {
  local reason="$1"
  err "$reason"
  if [[ -n "$ROLLBACK" && -f "$ROLLBACK" ]]; then
    warn "从回滚点恢复: $ROLLBACK"
    install -m 0755 "$ROLLBACK" "$BINARY" || warn "恢复二进制失败"
    if "$BINARY" --version >/dev/null 2>&1; then
      info "回滚二进制可执行: $("$BINARY" --version)"
    else
      err "回滚二进制不可执行！需人工介入"
      keep_tmp=1
      warn "保留现场供排查: $tmp"
    fi
  fi
  systemctl restart "$SERVICE" || warn "重启失败"
  exit 1
}

mv -f "$staged" "$BINARY" || restore_and_exit "替换二进制失败"
info "二进制已替换，重启服务"
systemctl restart "$SERVICE" || restore_and_exit "服务重启失败"

# ---------------------------------------------------------------- 健康检查 ----
# 三重确认: 进程活跃 + HTTP ok + 运行版本 == 新版本
healthy=false
deadline=$((SECONDS+90))
while [[ $SECONDS -lt $deadline ]]; do
  if systemctl is-active --quiet "$SERVICE"; then
    body=$(curl --fail --silent --max-time 5 "$HEALTH_URL" 2>/dev/null || true)
    if grep -q '"ok":true' <<<"$body"; then
      running="$("$BINARY" --version 2>/dev/null | awk '{print $2}' || true)"
      if [[ "$running" == "$version" ]]; then
        healthy=true; break
      fi
    fi
  fi
  sleep 2
done

if [[ "$healthy" != true ]]; then
  restore_and_exit "健康检查未通过 (服务未活跃 / health 非 ok / 运行版本非 $version)"
fi

# ---------------------------------------------------------------- 收尾 ----
printf '%s\n' "$tag" >"$STATE_DIR/last-release-tag"
printf '%s\n' "$version" >"$STATE_DIR/.synced-version"
# 升级成功后, 把已验证可用的新版本也纳入已知良好集合
cp -p "$BINARY" "$KNOWN_DIR/spm-cloud.$version" 2>/dev/null || true
ls -1t "$KNOWN_DIR"/spm-cloud.* 2>/dev/null | tail -n +$((KEEP+1)) | xargs -r rm -f

info "升级成功: $current_version -> $version (健康检查通过，运行版本一致)"
exit 0
