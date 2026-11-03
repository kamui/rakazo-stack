# Sidecar image with Rakazo v0.4.0's restrict-computer-egress.sh (unmodified) built in, so the container needs no
# bind mount (WSL bind mounts need the distro running) and no network at start (no apk add at boot).
FROM alpine:3
RUN apk add --no-cache bash iptables ip6tables
COPY restrict-computer-egress.sh /usr/local/bin/restrict-computer-egress.sh
CMD ["sh", "-c", "while true; do bash /usr/local/bin/restrict-computer-egress.sh --apply && touch /tmp/applied; sleep 60; done"]
