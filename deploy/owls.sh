#!/usr/bin/env bash
# Owls on this machine without Docker, from the release binary. Each owl gets
# its own directory under run/ (ignored by git and by Docker builds), which
# holds its log, its logos-delivery store, and its instance ID, so a
# restarted owl keeps its name:
#
#   run/flock.key    the flock key, unless FLOCK_KEY is set
#   run/owl-N/       owl.log, instance, data/
#   run/owlet-N/     the same, for ephemeral owls
#
# Commands:
#   deploy/owls.sh start N       make sure named owls 1..N are running
#   deploy/owls.sh owlets N      make sure ephemeral owls 1..N are running
#   deploy/owls.sh stop [owlets] stop every owl, or only the ephemeral owls
#   deploy/owls.sh status        what is running
#
# Build first:
#   LOGOS_DELIVERY_LIB_DIR=/path/to/libchat/result/lib cargo build --release -p owl
#
# Settings, from the environment:
#   ROOST_URL     where roost is, default http://127.0.0.1:30117
#   FLOCK_KEY     the flock key, default the contents of run/flock.key
#   OWL_OPERATOR  who runs these owls, shown next to them in roost; default
#                 your login name
set -euo pipefail
cd "$(dirname "$0")/.."

RUN=run
BIN=target/release/owl
mkdir -p "$RUN"

: "${ROOST_URL:=http://127.0.0.1:30117}"
: "${OWL_OPERATOR:=$(id -un)}"
export ROOST_URL OWL_OPERATOR NO_COLOR=1

# Whether the process in pid file $1 is alive.
alive() { [ -f "$1" ] && kill -0 "$(cat "$1")" 2>/dev/null; }

# Waits up to 90s for the process in pid file $1 to exit, then forgets it.
# One still running after that keeps its pid file, so it is not lost track of.
gone() {
  local f="$1"
  for _ in $(seq 1 180); do alive "$f" || break; sleep 0.5; done
  if alive "$f"; then
    echo "$(dirname "$f"): still shutting down (pid $(cat "$f")); run this again later" >&2
    return 1
  fi
  rm -f "$f"
}

case "${1:-}" in
  start|owlets)
    [ -x "$BIN" ] || { echo "no $BIN: build it first (see the top of $0)" >&2; exit 1; }
    if [ "$1" = owlets ]; then kind=owlet ephemeral=true; else kind=owl ephemeral=false; fi
    n="${2:?how many? e.g. $0 $1 5}"
    if [ -z "${FLOCK_KEY:-}" ]; then
      [ -s "$RUN/flock.key" ] || { echo "set FLOCK_KEY, or put roost's flock key in $RUN/flock.key" >&2; exit 1; }
      FLOCK_KEY="$(cat "$RUN/flock.key")"
    fi
    for i in $(seq 1 "$n"); do
      dir="$RUN/$kind-$i"
      mkdir -p "$dir"
      alive "$dir/owl.pid" && continue
      # Only the owl goes to the background, so $! is its pid. It keeps that
      # pid when it restarts itself.
      (cd "$dir" || exit 1; FLOCK_KEY="$FLOCK_KEY" OWL_EPHEMERAL="$ephemeral" nohup "../../$BIN" >> owl.log 2>&1 & echo $! > owl.pid)
      echo "$kind-$i started: log $dir/owl.log"
    done
    echo "operator: $OWL_OPERATOR"
    if [ "$kind" = owl ]; then
      echo "roost gives each a named owl: the same one as before, if it had one"
    else
      echo "roost gives each ephemeral owl a free owlet name, registering more as needed"
    fi
    ;;
  stop)
    # All at once: each flushes its reports. Each keeps its claim on its
    # identity, so starting it again gets the same owl back.
    case "${2:-}" in
      "") which="owl owlet" ;;
      owlets) which=owlet ;;
      *) echo "stop what? $0 stop, or $0 stop owlets" >&2; exit 1 ;;
    esac
    stuck=0
    for k in $which; do
      for f in "$RUN"/"$k"-*/owl.pid; do if alive "$f"; then kill -TERM "$(cat "$f")"; fi; done
    done
    for k in $which; do
      for f in "$RUN"/"$k"-*/owl.pid; do if [ -e "$f" ]; then gone "$f" || stuck=1; fi; done
    done
    echo "stopped"
    exit "$stuck"
    ;;
  status)
    for f in "$RUN"/owl-*/owl.pid "$RUN"/owlet-*/owl.pid; do
      [ -e "$f" ] || continue
      dir="$(dirname "$f")"
      if alive "$f"; then state=running; else state=stopped; fi
      echo "$(basename "$dir"): $state  $(tail -n 1 "$dir/owl.log" 2>/dev/null | cut -c 1-120)"
    done
    ;;
  *)
    sed -n '2,24p' "$0" | sed 's/^# \{0,1\}//'
    exit 1
    ;;
esac
