#!/usr/bin/env bash
# infra/v2/restore-drill.sh: prove the newest backups restore (07 §6).
#
#   infra/v2/restore-drill.sh            # database + store manifest
#   LANKA_DRILL_TOLERATE_DRIFT=1 infra/v2/restore-drill.sh
#
# 1. Restores the newest dump into a scratch database (lanka_drill) in the
#    database container. The live database is never touched.
# 2. Runs the engine against it: `lanka db migrate` must find the schema
#    complete, `lanka release list` must read it, and `lanka serve rebuild`
#    must rebuild the serving projections from the restored release.
# 3. Compares every table's row count with the counts taken at dump time.
#    A difference fails the drill unless LANKA_DRILL_TOLERATE_DRIFT=1 (for
#    dumps taken while the worker was writing); an empty table that had rows
#    always fails.
# 4. Re-hashes every file of the newest store snapshot against its manifest.
# 5. Drops the scratch database and records the outcome
#    (`lanka ops record-backup --kind drill`).
#
# Environment: as backup.sh, plus DATABASE_URL for the live database (the
# drill swaps its database name for lanka_drill).
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
BACKUP_DIR=${LANKA_BACKUP_DIR:-/Volumes/externalSSD/lanka-data/backups}
DB_CONTAINER=${LANKA_DB_CONTAINER:-lanka-v2-db}
DB_USER=${POSTGRES_USER:-lanka}
LIVE_URL=${DATABASE_URL:-postgres://lanka:lanka-local-dev@127.0.0.1:55432/lanka}
DRILL_DB=lanka_drill
DRILL_URL="${LIVE_URL%/*}/$DRILL_DB"

log() { printf '[drill] %s\n' "$*" >&2; }
sha256() { if command -v sha256sum >/dev/null; then sha256sum "$@"; else shasum -a 256 "$@"; fi; }
lanka_bin() {
  if [ -n "${LANKA_BIN:-}" ]; then echo "$LANKA_BIN"; return; fi
  if command -v lanka >/dev/null; then command -v lanka; return; fi
  for b in "$REPO/engine/target/release/lanka" "$REPO/engine/target/debug/lanka"; do
    [ -x "$b" ] && { echo "$b"; return; }
  done
  echo "lanka binary not found (set LANKA_BIN)" >&2; exit 1
}
LANKA=$(lanka_bin)

failures=()
fail() { log "FAIL: $*"; failures+=("$*"); }
t0=$(date +%s)

cleanup() {
  docker exec -i "$DB_CONTAINER" psql -U "$DB_USER" -d postgres -Atqc "drop database if exists \"$DRILL_DB\" with (force)" >/dev/null 2>&1 || true
}
trap cleanup EXIT

# shellcheck disable=SC2012
dump=$(ls -1 "$BACKUP_DIR"/pg/*.dump 2>/dev/null | sort | tail -1 || true)
[ -n "$dump" ] || { log "no dump in $BACKUP_DIR/pg; run infra/v2/backup.sh first"; exit 1; }
counts="${dump%.dump}.counts"
log "dump $(basename "$dump")"

# 1. restore into the scratch database
if ! "$HERE/restore.sh" pg --db "$DRILL_DB" --dump "$dump" --replace; then
  fail "restore of $(basename "$dump") failed"
else
  # 2. the engine against the restored database
  if ! DATABASE_URL=$DRILL_URL "$LANKA" db migrate >/dev/null 2>&1; then fail "lanka db migrate failed on the restored database"; fi
  if ! rel=$(DATABASE_URL=$DRILL_URL "$LANKA" release list 2>&1); then fail "lanka release list failed: $rel"; else log "releases readable ($(printf '%s\n' "$rel" | grep -c . ) lines)"; fi
  if ! out=$(DATABASE_URL=$DRILL_URL "$LANKA" serve rebuild 2>&1); then fail "lanka serve rebuild failed: $(printf '%s' "$out" | tail -1)"; else log "serving projections rebuilt"; fi

  # 3. row counts
  if [ -f "$counts" ]; then
    restored=$(docker exec -i "$DB_CONTAINER" psql -U "$DB_USER" -d "$DRILL_DB" -Atq -F ' ' <<'SQL'
select t.table_schema || '.' || t.table_name,
       (xpath('/row/c/text()', query_to_xml(format('select count(*) as c from only %I.%I', t.table_schema, t.table_name), false, true, '')))[1]::text
from information_schema.tables t
where t.table_type = 'BASE TABLE' and t.table_schema not in ('pg_catalog', 'information_schema')
order by 1;
SQL
)
    # `serve rebuild` rewrote the serving projections, so serve.* is compared before nothing: skip it.
    drift=$(awk 'NR==FNR { want[$1]=$2; next } { got[$1]=$2 }
      END {
        for (t in want) { if (t ~ /^serve\./) continue
          if (!(t in got)) print "missing " t
          else if (got[t] == 0 && want[t] > 0) print "empty " t " (had " want[t] ")"
          else if (got[t] != want[t]) print "drift " t " " want[t] " -> " got[t] }
      }' "$counts" <(printf '%s\n' "$restored") | sort)
    tables=$(grep -c . "$counts")
    if [ -z "$drift" ]; then log "row counts match in all $tables tables"
    else
      printf '%s\n' "$drift" | head -20 | sed 's/^/[drill]   /' >&2
      if printf '%s\n' "$drift" | grep -q -E '^(missing|empty) '; then fail "tables missing or emptied after restore"
      elif [ "${LANKA_DRILL_TOLERATE_DRIFT:-0}" = 1 ]; then log "row counts drifted in $(printf '%s\n' "$drift" | grep -c .) tables (tolerated)"
      else fail "row counts differ in $(printf '%s\n' "$drift" | grep -c .) tables"; fi
    fi
  else
    log "no row counts beside the dump; skipping that check"
  fi
fi

# 4. store snapshot
snap=$(readlink "$BACKUP_DIR/store/latest" 2>/dev/null || true)
if [ -z "$snap" ]; then fail "no store snapshot"
elif (cd "$BACKUP_DIR/store/$snap" && sha256 -c --quiet "../$snap.sha256"); then
  log "store snapshot $snap: $(grep -c . "$BACKUP_DIR/store/$snap.sha256") files match the manifest"
else fail "store snapshot $snap does not match its manifest"; fi

# 5. outcome
secs=$(( $(date +%s) - t0 ))
if [ ${#failures[@]} -eq 0 ]; then
  log "drill passed in ${secs}s"
  "$LANKA" ops record-backup --kind drill --ok --detail "{\"dump\":\"$(basename "$dump")\",\"snapshot\":\"$snap\",\"seconds\":$secs}" >/dev/null 2>&1 || log "outcome not recorded"
else
  log "drill FAILED (${#failures[@]} checks) in ${secs}s"
  "$LANKA" ops record-backup --kind drill --failed --detail "{\"dump\":\"$(basename "$dump")\",\"failures\":${#failures[@]}}" >/dev/null 2>&1 || true
  exit 1
fi
