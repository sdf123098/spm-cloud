import snapshot from "./mojang-certificate-keys.json";

const RSA = "RSASSA-PKCS1-v1_5";
const ROOT_URL = "https://api.minecraftservices.com/publickeys";
const ROOT_CACHE_KEY = `${ROOT_URL}?spm_trust_set=${encodeURIComponent(snapshot.fetched_at)}`;

/** Binds the proof to this Cloud, operation, account and single-use challenge. */
export function profileKeyPayload(origin: string, purpose: string, accountId: string | null,
  providerId: string, uuid: string, challengeId: string, serverId: string): string {
  return ["SPM-CLOUD-GAME-IDENTITY-V1", origin, purpose, accountId ?? "", providerId, uuid, challengeId, serverId].join("\n");
}

function base64(value: unknown, maximum: number): Uint8Array<ArrayBuffer> {
  if (typeof value !== "string" || !value || value.length > maximum
      || !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(value)) {
    throw new Error("invalid base64");
  }
  return Uint8Array.from(atob(value), character => character.charCodeAt(0));
}

async function rsaKey(bytes: Uint8Array<ArrayBuffer>, hash: string): Promise<CryptoKey> {
  const key = await crypto.subtle.importKey("spki", bytes, { name: RSA, hash }, false, ["verify"]);
  const algorithm = key.algorithm as RsaHashedKeyAlgorithm;
  if (algorithm.modulusLength < 2048 || algorithm.modulusLength > 8192) throw new Error("invalid RSA key size");
  return key;
}

async function importRoots(value: unknown): Promise<CryptoKey[]> {
  if (!Array.isArray(value) || value.length < 1 || value.length > 8) throw new Error("invalid certificate roots");
  return Promise.all(value.map(item => rsaKey(base64(item?.publicKey, 2048), "SHA-1")));
}

async function certificateRoots(family: "playerCertificateKeys" | "profilePropertyKeys" = "playerCertificateKeys"): Promise<CryptoKey[]> {
  const cache = await caches.open("spm-mojang-certificate-roots");
  try {
    const cacheKey = `${ROOT_CACHE_KEY}&family=${family}`;
    const cached = await cache.match(cacheKey);
    if (cached) return await importRoots((await cached.json() as Record<string,unknown>)[family]);
    const response = await fetch(ROOT_URL, {
      headers: { accept: "application/json" }, redirect: "manual", signal: AbortSignal.timeout(5000),
    });
    if (!response.ok || !response.headers.get("content-type")?.includes("json")) {
      throw new Error(`publickeys returned HTTP ${response.status}`);
    }
    const body = await boundedText(response);
    const keys = await importRoots(JSON.parse(body)[family]);
    // Only public, server-fetched trust anchors are cached. No request I/O promises cross requests.
    await cache.put(`${ROOT_CACHE_KEY}&family=${family}`, new Response(body, {
      headers: { "content-type": "application/json", "cache-control": "public, max-age=3600" },
    }));
    console.info("Mojang certificate roots refreshed", keys.length);
    return keys;
  } catch (failure) {
    console.warn("Mojang certificate roots refresh failed", failure instanceof Error ? failure.message : "fetch failed");
  }
  // Retry a blocked refresh after five minutes while keeping the deployed trust set.
  await cache.put(`${ROOT_CACHE_KEY}&family=${family}`, new Response(JSON.stringify(snapshot), {
    headers: { "content-type": "application/json", "cache-control": "public, max-age=300" },
  })).catch(() => {});
  return importRoots(snapshot[family]);
}

async function boundedText(response: Response): Promise<string> {
  if (!response.body) throw new Error("empty publickeys response");
  const reader = response.body.getReader();
  const parts: Uint8Array[] = [];
  let size = 0;
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      size += value.byteLength;
      if (size > 32_768) throw new Error("publickeys response is too large");
      parts.push(value);
    }
  } finally { await reader.cancel(); }
  const bytes = new Uint8Array(size);
  let offset = 0;
  for (const part of parts) { bytes.set(part, offset); offset += part.byteLength; }
  return new TextDecoder().decode(bytes);
}

/** Mojang v2 certificate signs UUID (16 bytes), expiry (8 bytes BE), then SPKI. */
export async function verifyOfficialProfileKey(value: unknown, uuid: string, payload: string): Promise<boolean> {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const proof = value as Record<string, unknown>;
  const expiresAt = proof.expires_at_ms;
  if (typeof expiresAt !== "number" || !Number.isSafeInteger(expiresAt) || expiresAt <= Date.now()) return false;
  let key: CryptoKey;
  let certificate: Uint8Array<ArrayBuffer>;
  let keySignature: Uint8Array<ArrayBuffer>;
  let challengeSignature: Uint8Array<ArrayBuffer>;
  try {
    const bytes = base64(proof.public_key, 2048);
    keySignature = base64(proof.key_signature, 2048);
    challengeSignature = base64(proof.challenge_signature, 2048);
    key = await rsaKey(bytes, "SHA-256");
    certificate = new Uint8Array(24 + bytes.length);
    const compactUuid = uuid.replaceAll("-", "");
    for (let i = 0; i < 16; i++) certificate[i] = parseInt(compactUuid.slice(i * 2, i * 2 + 2), 16);
    new DataView(certificate.buffer).setBigInt64(16, BigInt(expiresAt), false);
    certificate.set(bytes, 24);
  } catch { return false; }
  // Versioned Mojang trust anchors let normal certificates verify without Worker egress.
  // Renew this snapshot on a key rotation; certificates themselves must never be expired.
  const pinned = await importRoots(snapshot.playerCertificateKeys);
  let certified = (await Promise.all(pinned.map(root => crypto.subtle.verify(RSA, root, keySignature, certificate)))).some(Boolean);
  if (!certified) {
    const fresh = await certificateRoots();
    certified = (await Promise.all(fresh.map(root => crypto.subtle.verify(RSA, root, keySignature, certificate)))).some(Boolean);
  }
  if (!certified) return false;
  return crypto.subtle.verify(RSA, key, challengeSignature, new TextEncoder().encode(payload));
}

/** Signed public profile metadata proves canonical name; never trusts a client-supplied name or key. */
export async function verifyOfficialProfileName(value: unknown, uuid: string): Promise<string | null> {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const proof=value as Record<string,unknown>;
  let signature: Uint8Array<ArrayBuffer>, encoded: Uint8Array<ArrayBuffer>, name: string;
  try {
    const decoded=base64(proof.value,16384);
    signature=base64(proof.signature,2048);
    const profile=JSON.parse(new TextDecoder("utf-8",{fatal:true}).decode(decoded));
    if (typeof profile.profileId !== "string" || profile.profileId.toLowerCase() !== uuid.replaceAll("-", "")
        || typeof profile.profileName !== "string" || !/^[A-Za-z0-9_]{1,16}$/.test(profile.profileName)
        || !Number.isSafeInteger(profile.timestamp) || profile.timestamp < Date.now()-300000
        || profile.timestamp > Date.now()+60000) return null;
    name=profile.profileName;
    encoded=new TextEncoder().encode(proof.value as string);
  } catch { return null; }
  const pinned=await importRoots(snapshot.profilePropertyKeys);
  if ((await Promise.all(pinned.map(root=>crypto.subtle.verify(RSA,root,signature,encoded)))).some(Boolean)) return name;
  const fresh=await certificateRoots("profilePropertyKeys");
  return (await Promise.all(fresh.map(root=>crypto.subtle.verify(RSA,root,signature,encoded)))).some(Boolean) ? name : null;
}
