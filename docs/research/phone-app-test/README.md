# Phone app test against a self-hosted Instance (throwaway)

Ticket: "Do Rakazo's phone apps work with a self-hosted Instance end to end?" (kamui/rakazo-stack#19). Results are on the ticket.

Setup used: Rakazo v0.4.0 published images (`infra/compose/docker-compose.images.yml` at the tag, unmodified) plus `compose.override.yml` (no Docker socket for the supervisor, so no bot computers), on an Apple-silicon Mac with Docker Desktop. Env as in `research/setup-rpc-proof`'s `make-env.sh`, except:

- `RAKAZO_WEB_BIND=0.0.0.0` so the phone can reach the web port over Wi-Fi and the Tailscale IP.
- `RAKAZO_HOST=<device>.<tailnet>.ts.net` so Vite's host check accepts the `tailscale serve` name (IP addresses pass it by default).
- `BETTER_AUTH_URL`, `WEB_ORIGIN`, `API_URL` set to the `https://<device>.<tailnet>.ts.net` origin.
- Host-level `tailscale serve --bg --https=443 http://127.0.0.1:<web port>` for the HTTPS origin (removed afterwards with `tailscale serve --https=443 off`).
- `bootstrap-owner` needs an `OWNER_PASSWORD` of at least 16 characters.

`push-test.sh` sends one notification to the Owner's registered device the way Rakazo's `ExpoPushProvider` does: it decrypts the stored token with the stack's `ENCRYPTION_KEY` (same scheme as `EncryptedSecretStore`, AAD `push-token:<userId>`), POSTs to `exp.host` from the worker container with no `EXPO_ACCESS_TOKEN`, then reads the receipt. The token is never printed. Run with `DC="docker compose -p <project> ..." ./push-test.sh`; it assumes the Owner is `owner@example.com`.
