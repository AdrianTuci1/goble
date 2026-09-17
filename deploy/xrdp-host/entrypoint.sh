#!/bin/sh
# Start xrdp the way its systemd unit does on a real host, without an init in the
# container: the system bus, then `xrdp-sesman` (which starts the session), then
# `xrdp` itself in the foreground so the container lives as long as the server.
set -eu

mkdir -p /var/run/xrdp /var/run/dbus
rm -f /var/run/xrdp/xrdp.pid /var/run/xrdp/xrdp-sesman.pid

if [ ! -S /var/run/dbus/system_bus_socket ]; then
    dbus-daemon --system --fork
fi

/usr/sbin/xrdp-sesman

exec /usr/sbin/xrdp --nodaemon
