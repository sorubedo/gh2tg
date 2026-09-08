#!/bin/sh
set -eu

schedule="${GH2TG_CRON:-0 * * * *}"
config_path="${GH2TG_CONFIG:-config.json}"
state_path="${GH2TG_STATE:-state.json}"

shell_quote() {
    printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"
}

quoted_config_path=$(shell_quote "$config_path")
quoted_state_path=$(shell_quote "$state_path")

printf '%s root cd /data && /usr/local/bin/gh2tg --config %s --state %s >>/proc/1/fd/1 2>>/proc/1/fd/2\n' \
    "$schedule" \
    "$quoted_config_path" \
    "$quoted_state_path" \
    > /etc/cron.d/gh2tg

chmod 0644 /etc/cron.d/gh2tg
exec cron -f
