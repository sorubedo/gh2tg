#!/bin/sh
set -eu

schedule="${BETTER_CI_CRON:-0 * * * *}"
config_path="${BETTER_CI_CONFIG:-config.json}"
state_path="${BETTER_CI_STATE:-state.json}"

shell_quote() {
    printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"
}

quoted_config_path=$(shell_quote "$config_path")
quoted_state_path=$(shell_quote "$state_path")

printf '%s root cd /data && /usr/local/bin/better-ci --config %s --state %s >>/proc/1/fd/1 2>>/proc/1/fd/2\n' \
    "$schedule" \
    "$quoted_config_path" \
    "$quoted_state_path" \
    > /etc/cron.d/better-ci

chmod 0644 /etc/cron.d/better-ci
exec cron -f
