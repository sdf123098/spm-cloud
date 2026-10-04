import { createHash, createPublicKey } from "node:crypto";
import { writeFile } from "node:fs/promises";

const source = "https://api.minecraftservices.com/publickeys";
const response = await fetch(source, { redirect: "error", signal: AbortSignal.timeout(10_000) });
if (!response.ok || !response.headers.get("content-type")?.includes("json")) {
  throw new Error(`Official publickeys endpoint returned HTTP ${response.status}`);
}
const { playerCertificateKeys, profilePropertyKeys } = await response.json();
if (!Array.isArray(playerCertificateKeys) || playerCertificateKeys.length < 1 || playerCertificateKeys.length > 8) {
  throw new Error("Official endpoint returned an invalid player certificate key set");
}
if (!Array.isArray(profilePropertyKeys) || !profilePropertyKeys.length || profilePropertyKeys.length > 8) throw new Error("Invalid official profile property roots");
for (const { publicKey } of [...playerCertificateKeys, ...profilePropertyKeys]) {
  if (typeof publicKey !== "string" || publicKey.length > 2048 || !/^[A-Za-z0-9+/]+={0,2}$/.test(publicKey)) {
    throw new Error("Invalid certificate root encoding");
  }
  const bytes = Buffer.from(publicKey, "base64");
  const key = createPublicKey({ key: bytes, type: "spki", format: "der" });
  if (key.asymmetricKeyType !== "rsa" || key.asymmetricKeyDetails.modulusLength < 2048) throw new Error("Invalid RSA root");
  console.log(`Official Mojang public root SHA-256: ${createHash("sha256").update(bytes).digest("hex")}`);
}
await writeFile(new URL("../src/mojang-certificate-keys.json", import.meta.url), JSON.stringify({
  source, fetched_at: new Date().toISOString(), playerCertificateKeys, profilePropertyKeys,
}, null, 2) + "\n");
console.log(`Saved ${playerCertificateKeys.length} certificate and ${profilePropertyKeys.length} profile property roots; validate and redeploy the Worker to activate changes`);
