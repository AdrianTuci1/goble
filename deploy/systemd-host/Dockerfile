# A systemd-as-PID-1 host for the generated install script (T3).
#
# Not the same thing as deploy/goblin/Dockerfile: that one is the stale
# Kubernetes/distroless artifact of an earlier design and carries the worker
# itself. This image carries no goble code and no language runtime at all. It is
# a plain Ubuntu host the way a VPS is — systemd is PID 1 — and the install
# script is copied into it and run there:
#
#   cargo test -p goble-core --test install_systemd_e2e -- --ignored --nocapture
#
# which builds this image, mounts it with `--privileged --cgroupns=host
# --tmpfs /run --tmpfs /run/lock -v /sys/fs/cgroup:/sys/fs/cgroup:rw`, copies in
# a Linux `goblin` built from the mounted source, and runs the script as root.
FROM ubuntu:24.04

# systemd as PID 1 needs to know it is in a container, or it refuses to boot.
ENV container=docker
ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
 && apt-get install -y --no-install-recommends systemd systemd-sysv \
 && rm -rf /var/lib/apt/lists/*

# systemd generates its machine id on first boot; a pre-seeded one belongs to the
# build host, and `systemctl is-system-running` then waits forever on a mismatch.
RUN rm -f /etc/machine-id /var/lib/dbus/machine-id

# `systemctl stop` is what this signal is: without it the container is killed
# with SIGTERM, which systemd ignores as PID 1.
STOPSIGNAL SIGRTMIN+3

CMD ["/sbin/init"]
