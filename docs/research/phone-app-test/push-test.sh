#!/usr/bin/env bash
# Sends one push to the Owner's registered device the way Rakazo's ExpoPushProvider does:
# token decrypted with the stack's ENCRYPTION_KEY, POST to exp.host from the worker container,
# no EXPO_ACCESS_TOKEN. Then reads the receipt. The token itself is never printed.
set -euo pipefail
dc=${DC:?set DC to a command that runs docker compose for the stack}
row=$($dc exec -T postgres psql -U rakazo -d rakazo -qAtX -F ' ' -c \
  "select p.\"userId\", p.ciphertext, p.\"updatedAt\" from push_tokens p join \"user\" u on u.id = p.\"userId\" where u.email = 'owner@example.com'")
[[ -n $row ]] || { echo "no push token registered for the Owner"; exit 1; }
read -r user_id cipher updated <<<"$row"
echo "token registered at $updated"
$dc exec -T -e PT_USER="$user_id" -e PT_CIPHER="$cipher" worker node --input-type=module -e '
import { scrypt, createDecipheriv, randomUUID } from "node:crypto";
const c = process.env.PT_CIPHER, user = process.env.PT_USER, key = process.env.ENCRYPTION_KEY;
if (!c.startsWith("v2:")) throw new Error("unexpected ciphertext format");
const b = Buffer.from(c.slice(3), "base64");
const salt = b.subarray(0, 16), iv = b.subarray(16, 28), tag = b.subarray(28, 44), enc = b.subarray(44);
const k = await new Promise((ok, no) => scrypt(key, salt, 32, { N: 16384, r: 8, p: 1, maxmem: 64 << 20 }, (e, v) => e ? no(e) : ok(v)));
const d = createDecipheriv("aes-256-gcm", k, iv); d.setAAD(Buffer.from(`push-token:${user}`)); d.setAuthTag(tag);
const token = Buffer.concat([d.update(enc), d.final()]).toString("utf8");
console.log("token kind:", token.startsWith("ExponentPushToken[") ? "Expo push token" : "other");
const post = async (url, body) => {
  const r = await fetch(url, { method: "POST", headers: { "content-type": "application/json", accept: "application/json" }, body: JSON.stringify(body) });
  return { status: r.status, body: await r.json().catch(() => null) };
};
const send = await post("https://exp.host/--/api/v2/push/send", {
  to: token, title: "rakazo-stack push test", body: `Sent from a self-hosted Instance at ${new Date().toISOString()}`,
  data: { kind: "test", deliveryId: randomUUID() },
});
console.log("send:", send.status, JSON.stringify(send.body));
const id = send.body?.data?.[0]?.id ?? send.body?.data?.id;
if (!id) process.exit(0);
await new Promise((r) => setTimeout(r, 20000));
const receipt = await post("https://exp.host/--/api/v2/push/getReceipts", { ids: [id] });
console.log("receipt:", receipt.status, JSON.stringify(receipt.body));
'
