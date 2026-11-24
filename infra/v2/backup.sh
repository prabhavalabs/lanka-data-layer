#!/usr/bin/env bash
# infra/v2/backup.sh: local backups of the v2 database and blob store (07 §6).
#
#   infra/v2/backup.sh            # both: database dump, then store snapshot
#   infra/v2/backup.sh pg         # database only
#   infra/v2/backup.sh store      # blob store only
#   infra/v2/backup.sh verify     # re-check the newest dump and snapshot
#
# Database: `pg_dump -Fc` inside the database container (its pg_dump matches
# the server), written beside the backups as <ts>.dump with a .sha256, then
# read back with `pg_restore --list` before it counts.
#
# Store: a dated snapshot under store/<ts>/, hard-linked to the previous one
# (rsync --link-dest), so an unchanged file costs no space and a file deleted
# or overwritten in the live store survives in older snapshots. Every
# snapshot has a SHA-256 manifest; only files new in it are hashed, the rest
# carry their hash over from the previous manifest. tmp/ is not backed up.
#
# Each run ends with `lanka ops record-backup`, so /v2/status shows when the
# last good backup was and a stale one becomes an incident.
#
# This is the single-host form. The VPS adds the off-site copy (pgBackRest +
# restic, 07 §6); the snapshot layout here is what that copy ships.
#
# Environment:
#   LANKA_BACKUP_DIR   backups root   (default /Volumes/externalSSD/lanka-data/backups)
#   LANKA_STORE_DIR    live store     (default <repo>/data/v2)
#   LANKA_DB_CONTAINER database container (default lanka-v2-db)
#   POSTGRES_DB / POSTGRES_USER       (default lanka / lanka)
#   LANKA_KEEP         snapshots and dumps kept (default 14)
#   LANKA_BIN          lanka binary for record-backup (default: on PATH, else engine/target/{release,debug})
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
BACKUP_DIR=${LANKA_BACKUP_DIR:-/Volumes/externalSSD/lanka-data/backups}
STORE_DIR=${LANKA_STORE_DIR:-$REPO/data/v2}
DB_CONTAINER=${LANKA_DB_CONTAINER:-lanka-v2-db}
DB_NAME=${POSTGRES_DB:-lanka}
DB_USER=${POSTGRES_USER:-lanka}
KEEP=${LANKA_KEEP:-14}

log() { printf '[backup] %s\n' "$*" >&2; }
die() { log "error: $*"; exit 1; }
now_ts() { date -u +%Y%m%dT%H%M%SZ; }
sha256() { if command -v sha256sum >/dev/null; then sha256sum "$@"; else shasum -a 256 "$@"; fi; }
bytes() { du -sk "$1" | awk '{print $1 * 1024}'; }

lanka_bin() {
  if [ -n "${LANKA_BIN:-}" ]; then echo "$LANKA_BIN"; return; fi
  if command -v lanka >/dev/null; then command -v lanka; return; fi
  for b in "$REPO/engine/target/release/lanka" "$REPO/engine/target/debug/lanka"; do
    [ -x "$b" ] && { echo "$b"; return; }
  done
  echo ""
}

# record <kind> <ok|failed> <json detail>: best effort; a backup is not undone
# because the status database could not be told about it.
record() {
  local bin; bin=$(lanka_bin)
  if [ -z "$bin" ]; then log "lanka binary not found; outcome not recorded"; return 0; fi
  "$bin" ops record-backup --kind "$1" "--$2" --detail "$3" >/dev/null 2>&1 \
    || log "could not record the $1 outcome in the database"
}

# prune <dir> <glob>: keep the newest $KEEP entries matching glob (names sort by time).
prune() {
  local dir=$1 glob=$2 n
  # shellcheck disable=SC2012,SC2086  # $glob is a pattern on purpose
  n=$(ls -1d "$dir"/$glob 2>/dev/null | wc -l | tr -d ' ')
  if [ "$n" -gt "$KEEP" ]; then
    # shellcheck disable=SC2012,SC2086  # $glob is a pattern on purpose
    ls -1d "$dir"/$glob | sort | head -n $((n - KEEP)) | while read -r old; do
      log "pruning $(basename "$old")"
      rm -rf "$old" "$old.sha256" "${old%.dump}.counts"
    done
  fi
}

backup_pg() {
  local dir="$BACKUP_DIR/pg" ts out t0
  mkdir -p "$dir"
  docker inspect -f '{{.State.Running}}' "$DB_CONTAINER" 2>/dev/null | grep -q true \
    || die "database container $DB_CONTAINER is not running"
  ts=$(now_ts); out="$dir/$ts.dump"; t0=$(date +%s)
  log "dumping $DB_NAME from $DB_CONTAINER"
  if ! docker exec "$DB_CONTAINER" pg_dump -U "$DB_USER" -d "$DB_NAME" -Fc -Z 6 >"$out.part"; then
    rm -f "$out.part"; record pg failed "{\"error\":\"pg_dump failed\"}"; die "pg_dump failed"
  fi
  # A dump only counts once it reads back: the table of contents must parse.
  if ! docker exec -i "$DB_CONTAINER" pg_restore --list <"$out.part" >/dev/null; then
    rm -f "$out.part"; record pg failed "{\"error\":\"dump does not read back\"}"; die "dump does not read back"
  fi
  mv "$out.part" "$out"
  (cd "$dir" && sha256 "$ts.dump" >"$ts.dump.sha256")
  # Exact row counts per table, for the restore drill to compare against. They
  # are taken right after the dump, so they match it when nothing wrote in
  # between (run backups while the worker is idle, or let the drill tolerate drift).
  docker exec -i "$DB_CONTAINER" psql -U "$DB_USER" -d "$DB_NAME" -Atq -F ' ' >"$dir/$ts.counts" <<'SQL' || log "row counts not recorded"
select t.table_schema || '.' || t.table_name,
       (xpath('/row/c/text()', query_to_xml(format('select count(*) as c from only %I.%I', t.table_schema, t.table_name), false, true, '')))[1]::text
from information_schema.tables t
where t.table_type = 'BASE TABLE' and t.table_schema not in ('pg_catalog', 'information_schema')
order by 1;
SQL
  local size; size=$(bytes "$out")
  log "database dump $ts.dump ($((size / 1048576)) MB, $(( $(date +%s) - t0 )) s)"
  record pg ok "{\"file\":\"$out\",\"bytes\":$size,\"seconds\":$(( $(date +%s) - t0 ))}"
  prune "$dir" '*.dump'
}

backup_store() {
  local dir="$BACKUP_DIR/store" ts snap prev t0 new_list
  mkdir -p "$dir"
  [ -d "$STORE_DIR/" ] || die "store $STORE_DIR does not exist"
  ts=$(now_ts); snap="$dir/$ts"; t0=$(date +%s)
  prev=$(readlink "$dir/latest" 2>/dev/null || true)
  log "snapshotting $STORE_DIR"
  local link=()
  [ -n "$prev" ] && [ -d "$dir/$prev" ] && link=(--link-dest="$dir/$prev")
  # The trailing slash copies the store's contents, and follows a symlinked store root.
  if ! rsync -a --exclude='/tmp/' "${link[@]}" "$STORE_DIR/" "$snap.partial/"; then
    record store failed "{\"error\":\"rsync failed\"}"; die "rsync failed"
  fi

  # Manifest: hash files new in this snapshot (one link: not shared with the
  # previous one); take the rest from the previous manifest.
  new_list=$(mktemp)
  (cd "$snap.partial" && find . -type f -links 1 | LC_ALL=C sort) >"$new_list"
  {
    if [ -n "$prev" ] && [ -f "$dir/$prev.sha256" ]; then
      # unchanged files: present in the snapshot and not new
      (cd "$snap.partial" && find . -type f -links +1 | LC_ALL=C sort) \
        | awk 'NR==FNR { keep[$0]=1; next } { p=$0; sub(/^[0-9a-f]+  /, "", p); if (p in keep) print }' - "$dir/$prev.sha256"
    fi
    if [ -s "$new_list" ]; then (cd "$snap.partial" && tr '\n' '\0' <"$new_list" | xargs -0 -n 200 sh -c 'if command -v sha256sum >/dev/null; then sha256sum "$@"; else shasum -a 256 "$@"; fi' _); fi
  } | LC_ALL=C sort -k2 >"$snap.partial.sha256"
  local n_new n_all
  n_new=$(wc -l <"$new_list" | tr -d ' '); rm -f "$new_list"
  n_all=$(wc -l <"$snap.partial.sha256" | tr -d ' ')
  local n_files; n_files=$(cd "$snap.partial" && find . -type f | wc -l | tr -d ' ')
  if [ "$n_all" != "$n_files" ]; then
    record store failed "{\"error\":\"manifest covers $n_all of $n_files files\"}"
    die "manifest covers $n_all of $n_files files"
  fi

  mv "$snap.partial" "$snap"; mv "$snap.partial.sha256" "$snap.sha256"
  ln -sfn "$ts" "$dir/latest"
  local size; size=$(bytes "$snap")
  log "store snapshot $ts: $n_files files, $n_new new, $((size / 1048576)) MB apparent, $(( $(date +%s) - t0 )) s"
  record store ok "{\"snapshot\":\"$snap\",\"files\":$n_files,\"new_files\":$n_new,\"seconds\":$(( $(date +%s) - t0 ))}"
  prune "$dir" '20*Z'
}

verify() {
  local dump snap
  # shellcheck disable=SC2012,SC2086  # $glob is a pattern on purpose
  dump=$(ls -1 "$BACKUP_DIR"/pg/*.dump 2>/dev/null | sort | tail -1 || true)
  if [ -n "$dump" ]; then
    (cd "$(dirname "$dump")" && sha256 -c "$(basename "$dump").sha256" >/dev/null) || die "$dump: checksum mismatch"
    docker exec -i "$DB_CONTAINER" pg_restore --list <"$dump" >/dev/null || die "$dump does not read back"
    log "ok: $(basename "$dump")"
  else log "no database dump yet"; fi
  snap=$(readlink "$BACKUP_DIR/store/latest" 2>/dev/null || true)
  if [ -n "$snap" ]; then
    (cd "$BACKUP_DIR/store/$snap" && sha256 -c --quiet "../$snap.sha256") || die "store snapshot $snap: checksum mismatch"
    log "ok: store snapshot $snap ($(wc -l <"$BACKUP_DIR/store/$snap.sha256" | tr -d ' ') files)"
  else log "no store snapshot yet"; fi
}

case "${1:-all}" in
  pg) backup_pg ;;
  store) backup_store ;;
  all) backup_pg; backup_store ;;
  verify) verify ;;
  *) die "usage: backup.sh [all|pg|store|verify]" ;;
esac
