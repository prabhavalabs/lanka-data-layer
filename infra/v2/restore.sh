#!/usr/bin/env bash
# infra/v2/restore.sh: restore from the local backups that backup.sh writes (07 §6).
#
#   infra/v2/restore.sh pg    --db NAME [--dump FILE] [--replace]
#   infra/v2/restore.sh store --to DIR  [--snapshot TS]
#
# pg: restores a dump (default: the newest) into database NAME in the
# database container. NAME must not exist, unless --replace is given, which
# drops it first. Restoring over the live database therefore needs both its
# name and --replace; stop `lanka serve` and the worker before doing that.
#
# store: copies a snapshot (default: latest) into DIR and checks every file
# against the snapshot's manifest. DIR must be empty or missing, so a restore
# never mixes with a live store.
#
# Environment: as backup.sh (LANKA_BACKUP_DIR, LANKA_DB_CONTAINER, POSTGRES_USER).
set -euo pipefail

BACKUP_DIR=${LANKA_BACKUP_DIR:-/Volumes/externalSSD/lanka-data/backups}
DB_CONTAINER=${LANKA_DB_CONTAINER:-lanka-v2-db}
DB_USER=${POSTGRES_USER:-lanka}

log() { printf '[restore] %s\n' "$*" >&2; }
die() { log "error: $*"; exit 1; }
sha256() { if command -v sha256sum >/dev/null; then sha256sum "$@"; else shasum -a 256 "$@"; fi; }
psql_admin() { docker exec -i "$DB_CONTAINER" psql -v ON_ERROR_STOP=1 -U "$DB_USER" -d postgres -Atq "$@"; }

restore_pg() {
  local db="" dump="" replace=0
  while [ $# -gt 0 ]; do
    case "$1" in
      --db) db=$2; shift 2 ;;
      --dump) dump=$2; shift 2 ;;
      --replace) replace=1; shift ;;
      *) die "unknown option $1" ;;
    esac
  done
  [ -n "$db" ] || die "--db NAME is required"
  [[ "$db" =~ ^[a-z_][a-z0-9_]*$ ]] || die "database name must be lower-case letters, digits and _"
  if [ -z "$dump" ]; then
    # shellcheck disable=SC2012
    dump=$(ls -1 "$BACKUP_DIR"/pg/*.dump 2>/dev/null | sort | tail -1 || true)
    [ -n "$dump" ] || die "no dump in $BACKUP_DIR/pg"
  fi
  [ -f "$dump" ] || die "$dump not found"
  if [ -f "$dump.sha256" ]; then
    (cd "$(dirname "$dump")" && sha256 -c "$(basename "$dump").sha256" >/dev/null) || die "$dump: checksum mismatch"
  fi

  local exists; exists=$(psql_admin -c "select 1 from pg_database where datname = '$db'")
  if [ "$exists" = 1 ]; then
    [ "$replace" = 1 ] || die "database $db exists; pass --replace to drop and restore it"
    log "dropping database $db"
    psql_admin -c "drop database \"$db\" with (force)"
  fi
  psql_admin -c "create database \"$db\" owner \"$DB_USER\""
  log "restoring $(basename "$dump") into $db"
  # --exit-on-error: a partial restore is a failed restore.
  docker exec -i "$DB_CONTAINER" pg_restore -U "$DB_USER" -d "$db" --exit-on-error <"$dump" \
    || die "pg_restore failed; database $db is incomplete"
  log "restored into $db"
}

restore_store() {
  local to="" snap=""
  while [ $# -gt 0 ]; do
    case "$1" in
      --to) to=$2; shift 2 ;;
      --snapshot) snap=$2; shift 2 ;;
      *) die "unknown option $1" ;;
    esac
  done
  [ -n "$to" ] || die "--to DIR is required"
  [ -n "$snap" ] || snap=$(readlink "$BACKUP_DIR/store/latest" 2>/dev/null || true)
  [ -n "$snap" ] && [ -d "$BACKUP_DIR/store/$snap" ] || die "no store snapshot ${snap:-(none)} in $BACKUP_DIR/store"
  if [ -e "$to" ] && [ -n "$(ls -A "$to" 2>/dev/null)" ]; then die "$to is not empty"; fi
  mkdir -p "$to"
  log "copying snapshot $snap to $to"
  rsync -a "$BACKUP_DIR/store/$snap/" "$to/"
  (cd "$to" && sha256 -c --quiet "$BACKUP_DIR/store/$snap.sha256") || die "restored files do not match the manifest"
  log "restored $(wc -l <"$BACKUP_DIR/store/$snap.sha256" | tr -d ' ') files to $to; point LANKA_STORE_DIR at it"
}

cmd=${1:-}; shift || true
case "$cmd" in
  pg) restore_pg "$@" ;;
  store) restore_store "$@" ;;
  *) die "usage: restore.sh pg --db NAME [--dump FILE] [--replace] | store --to DIR [--snapshot TS]" ;;
esac
