#!/system/bin/sh
# Sourced by modules/deployer/run.sh (and by host smoke tests). Composes the
# receiver-side sync command with the local deploy command; it never parses or
# synthesises JSON. POSIX sh only, no secrets and no JSON are generated here.
#
# run_receiver_steps MODE SYNC_BIN DEPLOY_BIN CONFIG RESULT_BASE
#   MODE        action (foreground, no sidecars) | run | auto
#   RESULT_BASE result file prefix, e.g. <tmp>/sync-task; must live in a
#               private (0700) directory. Empty for action.
# Step sidecars: <base>.preflight.{json,err,code} (auto only),
# <base>.fetch.{json,err,code}, <base>.deploy.{json,err,code},
# <base>.last.op, <base>.last.err and <base>.last.{code,steps}.
# publish_run_result RESULT_BASE OVERALL publishes the 8-section snapshot and
# then (only after the rename succeeds) the completion code.

RKP_STEP_SEPARATOR='<<<RKP-SEP>>>'
RKP_SECTION_MAX=16384

# Last 16 KiB of a step's stderr, ending on a newline so the following
# separator line stays intact. Missing/empty files append nothing.
rkp_append_bounded() {
  [ -f "$1" ] && [ -s "$1" ] || return 0
  tail -c "$RKP_SECTION_MAX" "$1"
  if [ "$(tail -c 1 "$1" | wc -l)" -ne 1 ]; then
    printf '\n'
  fi
}

# Small files (exit code, one --json envelope) verbatim, newline-terminated.
rkp_append_raw() {
  [ -f "$1" ] && [ -s "$1" ] || return 0
  cat "$1"
  if [ "$(tail -c 1 "$1" | wc -l)" -ne 1 ]; then
    printf '\n'
  fi
}

# rkp_run_step PREFIX COMMAND...: raw stdout/stderr plus numeric exit code.
rkp_run_step() {
  _rkp_prefix=$1
  shift
  "$@" >"$_rkp_prefix.json" 2>"$_rkp_prefix.err"
  _rkp_code=$?
  printf '%s\n' "$_rkp_code" >"$_rkp_prefix.code"
  return "$_rkp_code"
}

run_receiver_steps() {
  _rkp_mode=$1
  _rkp_sync=$2
  _rkp_deployer=$3
  _rkp_config=$4
  _rkp_base=$5
  _rkp_fetch=0
  _rkp_deploy=0

  umask 077

  if [ "$_rkp_mode" = action ]; then
    # Foreground: stdin/stdout/stderr stay attached for passphrase prompts and
    # deploy confirmations; no result sidecars are written.
    for _rkp_bin in "$_rkp_sync" "$_rkp_deployer"; do
      if [ ! -f "$_rkp_bin" ] || [ ! -x "$_rkp_bin" ]; then
        printf '%s\n' "rkp-mini: binary missing or not executable: $_rkp_bin" >&2
        return 2
      fi
    done
    "$_rkp_sync" --config "$_rkp_config" --role receiver fetch
    _rkp_fetch=$?
    # Run apply even if fetch failed: it re-validates enabled/risk/stream and
    # only deploys a step that this receiver has already accepted.
    "$_rkp_deployer" --config "$_rkp_config" --role receiver apply --if-staged
    _rkp_deploy=$?
    if [ "$_rkp_deploy" -ne 0 ]; then
      return "$_rkp_deploy"
    fi
    return "$_rkp_fetch"
  fi

  # Result directory and cleanup come before any binary check, so an aborted
  # launch can never leave the previous run's steps/code behind.
  case "$_rkp_base" in
    */*)
      _rkp_dir=${_rkp_base%/*}
      [ -n "$_rkp_dir" ] || _rkp_dir=/
      ;;
    *)
      _rkp_dir=.
      ;;
  esac
  if ! mkdir -p "$_rkp_dir" 2>/dev/null; then
    printf '%s\n' "rkp-mini: cannot create result dir: $_rkp_dir" >&2
    return 2
  fi
  if [ "$_rkp_dir" != "." ] && [ "$_rkp_dir" != "/" ]; then
    chmod 0700 "$_rkp_dir" 2>/dev/null
  fi

  rm -f "$_rkp_base.last.code"
  rm -f "$_rkp_base.last.steps"
  rm -f "$_rkp_base.last.steps.tmp"
  rm -f "$_rkp_base.last.json"
  rm -f "$_rkp_base.last.err"
  rm -f "$_rkp_base.last.op"
  rm -f "$_rkp_base.preflight.json" "$_rkp_base.preflight.err" "$_rkp_base.preflight.code"
  rm -f "$_rkp_base.fetch.json" "$_rkp_base.fetch.err" "$_rkp_base.fetch.code"
  rm -f "$_rkp_base.deploy.json" "$_rkp_base.deploy.err" "$_rkp_base.deploy.code"
  printf '%s\n' "$_rkp_mode" >"$_rkp_base.last.op"

  for _rkp_bin in "$_rkp_sync" "$_rkp_deployer"; do
    if [ ! -f "$_rkp_bin" ] || [ ! -x "$_rkp_bin" ]; then
      _rkp_msg="binary missing or not executable: $_rkp_bin"
      printf '%s\n' "rkp-mini: $_rkp_msg" >>"$_rkp_base.last.err"
      printf '%s\n' "rkp-mini: $_rkp_msg" >&2
      return 2
    fi
  done

  case "$_rkp_mode" in
    run)
      # WebUI task: no check-auto gating, no implicit approvals; apply only
      # picks up staging this receiver has already accepted.
      rkp_run_step "$_rkp_base.fetch" \
        "$_rkp_sync" --config "$_rkp_config" --role receiver --json fetch
      _rkp_fetch=$?
      rkp_run_step "$_rkp_base.deploy" \
        "$_rkp_deployer" --config "$_rkp_config" --role receiver --json apply --if-staged
      _rkp_deploy=$?
      ;;
    auto)
      # Unattended: the verified-state gate runs first and a first-import
      # refusal stops the cycle without touching fetch or deploy.
      rkp_run_step "$_rkp_base.preflight" \
        "$_rkp_sync" --config "$_rkp_config" --role receiver --json check-auto --require-verified
      _rkp_preflight=$?
      if [ "$_rkp_preflight" -ne 0 ]; then
        return "$_rkp_preflight"
      fi
      rkp_run_step "$_rkp_base.fetch" \
        "$_rkp_sync" --config "$_rkp_config" --role receiver --json fetch --auto
      _rkp_fetch=$?
      rkp_run_step "$_rkp_base.deploy" \
        "$_rkp_deployer" --config "$_rkp_config" --role receiver --json apply --auto --if-staged
      _rkp_deploy=$?
      ;;
    *)
      return 2
      ;;
  esac

  if [ "$_rkp_deploy" -ne 0 ]; then
    return "$_rkp_deploy"
  fi
  if [ "$_rkp_fetch" -ne 0 ]; then
    return "$_rkp_fetch"
  fi
  return 0
}

publish_run_result() {
  _rkp_base=$1
  _rkp_overall=$2
  [ -n "$_rkp_base" ] || return 0

  _rkp_steps="$_rkp_base.last.steps"
  _rkp_tmp="$_rkp_base.last.steps.tmp"

  # Snapshot first: an interrupted publish must leave the old snapshot (or
  # none) and never a completion code that claims these steps finished.
  : >"$_rkp_tmp" || return 1
  {
    printf '%s\n' "$_rkp_overall"
    printf '%s\n' "$RKP_STEP_SEPARATOR"
    rkp_append_raw "$_rkp_base.fetch.code"
    printf '%s\n' "$RKP_STEP_SEPARATOR"
    rkp_append_raw "$_rkp_base.fetch.json"
    printf '%s\n' "$RKP_STEP_SEPARATOR"
    rkp_append_bounded "$_rkp_base.fetch.err"
    printf '%s\n' "$RKP_STEP_SEPARATOR"
    rkp_append_raw "$_rkp_base.deploy.code"
    printf '%s\n' "$RKP_STEP_SEPARATOR"
    rkp_append_raw "$_rkp_base.deploy.json"
    printf '%s\n' "$RKP_STEP_SEPARATOR"
    rkp_append_bounded "$_rkp_base.deploy.err"
    printf '%s\n' "$RKP_STEP_SEPARATOR"
    rkp_append_bounded "$_rkp_base.last.err"
  } >>"$_rkp_tmp" || {
    rm -f "$_rkp_tmp"
    return 1
  }

  mv -f "$_rkp_tmp" "$_rkp_steps" || {
    rm -f "$_rkp_tmp"
    return 1
  }
  printf '%s\n' "$_rkp_overall" >"$_rkp_base.last.code"
}
