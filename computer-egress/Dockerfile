FROM alpine:3.24.4
RUN apk add --no-cache bash iptables iptables-legacy
COPY restrict-computer-egress.sh /usr/local/lib/rakazo/restrict-computer-egress.sh
COPY computer-egress.sh /usr/local/bin/computer-egress
ENTRYPOINT ["bash", "/usr/local/bin/computer-egress"]
