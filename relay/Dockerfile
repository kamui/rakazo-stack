FROM oven/bun:1.4.4-alpine
RUN apk add --no-cache ffmpeg~=8.1
WORKDIR /app
COPY src ./src
USER bun
EXPOSE 13306
CMD ["bun", "src/relay.ts"]
