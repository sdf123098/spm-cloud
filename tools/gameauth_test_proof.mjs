// Synthetic keys exist only in memory. Rust uses this helper exclusively from cfg(test).
import { generateKeyPairSync, sign } from "node:crypto";
let input = "";
for await (const chunk of process.stdin) input += chunk;
const { profile, payload } = JSON.parse(input);
const root = generateKeyPairSync("rsa", { modulusLength: 2048 });
const player = generateKeyPairSync("rsa", { modulusLength: 2048 });
const expiry = 4102444800000;
const spki = player.publicKey.export({ format: "der", type: "spki" });
const time = Buffer.alloc(8);
time.writeBigInt64BE(BigInt(expiry));
const certificate = Buffer.concat([Buffer.from(profile.replaceAll("-", ""), "hex"), time, spki]);
process.stdout.write(JSON.stringify({
  root: root.publicKey.export({ format: "der", type: "spki" }).toString("base64"),
  profile_key: { public_key: spki.toString("base64"), expires_at_ms: expiry,
    key_signature: sign("RSA-SHA1", certificate, root.privateKey).toString("base64"),
    challenge_signature: sign("RSA-SHA256", Buffer.from(payload), player.privateKey).toString("base64") }
}));
