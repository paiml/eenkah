#!/bin/sh
# Production install of eenkah. For everyday local use, run `make build` / `make run` instead.
#
# Builds a verified release (web assets embedded), installs the binary and, with --service,
# a systemd unit that keeps `eenkah serve` running and imports from Garmin Connect daily.
#
#   ./install.sh [--prefix /usr/local] [--service] [--user NAME] [--data DIR] [--listen ADDR]
set -eu

prefix=/usr/local
service=0
user=$(id -un)
data=""
listen=127.0.0.1:8090

while [ $# -gt 0 ]; do
  case "$1" in
    --prefix) prefix=$2; shift 2 ;;
    --service) service=1; shift ;;
    --user) user=$2; shift 2 ;;
    --data) data=$2; shift 2 ;;
    --listen) listen=$2; shift 2 ;;
    -h|--help) sed -n '2,9p' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

cd "$(dirname "$0")"
command -v npm >/dev/null || { echo "npm is required to build the web assets" >&2; exit 1; }

echo "==> verifying (format, clippy, tests, type checks)"
make ui-deps
make ui
make verify
echo "==> building release"
make build

sudo=""
[ -w "$prefix/bin" ] 2>/dev/null || sudo=sudo
$sudo install -d "$prefix/bin"
$sudo install -m 0755 target/release/eenkah "$prefix/bin/eenkah"
echo "installed $prefix/bin/eenkah ($("$prefix/bin/eenkah" --version))"

if [ "$service" = 1 ]; then
  command -v systemctl >/dev/null || { echo "--service needs systemd" >&2; exit 1; }
  datarg=""
  [ -n "$data" ] && datarg="--data $data"
  case "$listen" in 127.0.0.1:*|localhost:*) ;; *) echo "warning: --listen $listen exposes your health data beyond this machine; put it behind authentication" >&2 ;; esac
  $sudo tee /etc/systemd/system/eenkah.service >/dev/null <<UNIT
[Unit]
Description=eenkah web UI
After=network-online.target

[Service]
User=$user
ExecStart=$prefix/bin/eenkah $datarg serve --listen $listen
Restart=on-failure
NoNewPrivileges=true
PrivateTmp=true

[Install]
WantedBy=multi-user.target
UNIT
  $sudo tee /etc/systemd/system/eenkah-import.service >/dev/null <<UNIT
[Unit]
Description=eenkah: download from Garmin Connect and import
After=network-online.target

[Service]
Type=oneshot
User=$user
ExecStart=$prefix/bin/eenkah $datarg import
UNIT
  $sudo tee /etc/systemd/system/eenkah-import.timer >/dev/null <<UNIT
[Unit]
Description=eenkah daily import

[Timer]
OnCalendar=*-*-* 06:30:00
RandomizedDelaySec=20m
Persistent=true

[Install]
WantedBy=timers.target
UNIT
  $sudo systemctl daemon-reload
  $sudo systemctl enable --now eenkah.service eenkah-import.timer
  echo "service running on http://$listen ; daily import at ~06:30"
  echo "sign in once as $user: eenkah $datarg login"
fi
