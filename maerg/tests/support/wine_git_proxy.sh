#!/usr/bin/env bash
set -u

marker=$1
working_directory=$2
shift 2

if cd "$working_directory"; then
    /usr/bin/git "$@"
    status=$?
else
    status=1
fi

cd /
printf '%s\n' "$status" > "${marker}.tmp"
mv "${marker}.tmp" "$marker"
