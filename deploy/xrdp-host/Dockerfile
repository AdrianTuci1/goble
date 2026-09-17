# An xrdp + Xfce desktop host for the RDP end-to-end test (T4).
#
# The package set and the pinned session are the ones the product's own
# `REMOTE_DESKTOP_STEP` installs and writes on a real host
# (`crates/goble-core/src/provision.rs`): `xrdp xorgxrdp xfce4 xfce4-terminal
# dbus-x11`, `/etc/xrdp/startwm.sh` running `startxfce4`. The image carries no
# goble code at all — it is the desktop the client connects *to*:
#
#   docker build -t goble-xrdp-host:t4 deploy/xrdp-host
#   docker run -d --name goble-t4-xrdp --shm-size=512m \
#     -p 127.0.0.1:13389:3389 goble-xrdp-host:t4
#   cargo test -p goble-screen-sdk --features rdp --test rdp_e2e -- --ignored
#
# `crates/goble-screen-sdk/tests/rdp_e2e.rs` starts and stops this fixture
# itself, so the two docker commands above are for running it by hand.
FROM ubuntu:24.04

ENV container=docker
ENV DEBIAN_FRONTEND=noninteractive

# The same packages `REMOTE_DESKTOP_STEP` installs, plus what a container needs
# to run an X session with no init: the `dbus` system daemon itself (`dbus-x11`
# is only the session client and a real host already has the daemon),
# `procps` for `xrdp-sesman`'s session handling, and `openssl` for the RDP TLS
# certificate.
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      xrdp xorgxrdp xfce4 xfce4-terminal dbus-x11 procps openssl ca-certificates \
      dbus \
 && rm -rf /var/lib/apt/lists/*

# An account with a known password, which is what the RDP client authenticates
# with. Nothing here is a secret — the fixture is a throwaway container.
RUN useradd -m -s /bin/bash goble \
 && echo 'goble:goble-rdp-e2e' | chpasswd

# `startwm.sh` is what xrdp runs for a session; pin it to `startxfce4` exactly as
# the install script does, so this fixture exercises the same session path.
RUN printf '%s\n' \
      '#!/bin/sh' \
      '[ -r /etc/profile ] && . /etc/profile' \
      'exec startxfce4' \
      > /etc/xrdp/startwm.sh \
 && chmod +x /etc/xrdp/startwm.sh

# The RDP TLS certificate the client negotiates. The package normally generates
# one in its post-install; generate it here when it is missing so a start never
# depends on that step having run, and give the `xrdp` user its key the way
# `adduser xrdp ssl-cert` does on a real host. `/etc/xrdp` itself stays
# world-traversable: the session user has to reach `startwm.sh` through it, and a
# group-only directory makes `xrdp-sesman` fail the session with
# `exec … /etc/xrdp/startwm.sh … errno: 13 Permission denied`.
RUN if [ ! -f /etc/xrdp/cert.pem ]; then \
      openssl req -x509 -newkey rsa:2048 -nodes -days 3650 \
        -keyout /etc/xrdp/key.pem -out /etc/xrdp/cert.pem -subj '/CN=xrdp-e2e'; \
    fi \
 && chown root:ssl-cert /etc/xrdp/key.pem /etc/xrdp/cert.pem \
 && chmod 0640 /etc/xrdp/key.pem /etc/xrdp/cert.pem \
 && chmod 0755 /etc/xrdp

COPY entrypoint.sh /usr/local/bin/xrdp-entrypoint.sh
RUN chmod +x /usr/local/bin/xrdp-entrypoint.sh

EXPOSE 3389

ENTRYPOINT ["/usr/local/bin/xrdp-entrypoint.sh"]
