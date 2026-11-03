# Sidecar image with Rakazo v0.4.0's restrict-computer-egress.sh (unmodified) built in. iptables-legacy is included so
# RAKAZO_IPTABLES=iptables-legacy / RAKAZO_IP6TABLES=ip6tables-legacy work on hosts whose Docker uses the legacy backend.
FROM alpine:3
RUN apk add --no-cache bash iptables ip6tables iptables-legacy
COPY restrict-computer-egress.sh /usr/local/bin/restrict-computer-egress.sh
CMD ["sh", "-c", "while true; do bash /usr/local/bin/restrict-computer-egress.sh --apply && touch /tmp/applied; sleep 60; done"]
