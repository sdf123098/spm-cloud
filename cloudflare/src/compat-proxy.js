// Compatibility entry point only. All authentication, data and realtime work
// runs on the Rust origin; this Worker does not read or write D1/R2/DO storage.
const OLD_ORIGINS = new Set([
  "https://micafic.xyz",
  "https://spm-cloud-official.robinson171.workers.dev",
]);

export default {
  async fetch(request, env) {
    const incoming = new URL(request.url);
    if (!OLD_ORIGINS.has(incoming.origin)) return new Response("Unknown legacy origin", { status: 421 });
    if (env.SPM_CLOUD_PROXY_MAINTENANCE === "true") {
      return new Response("SPM Cloud data migration in progress. Please retry shortly.", {
        status: 503, headers: { "Retry-After": "60", "Cache-Control": "no-store" },
      });
    }
    const target = new URL(incoming.pathname + incoming.search, env.SPM_CLOUD_ORIGIN);
    if (/^\/(admin|account)(\/|$)/.test(incoming.pathname)) return Response.redirect(target.href, 308);
    const forwarded = new Request(target.href, request);
    forwarded.headers.set("X-SPM-Compat-Origin", incoming.origin);
    // Returning fetch's original response preserves a 101 WebSocket response.
    return fetch(forwarded, { redirect: "manual" });
  },
};

// Retain the existing namespace so the deployment does not delete its storage.
// Legacy sockets reconnect through the Rust proxy when this retired room wakes.
export class ScopeRoom {
  constructor(state) {
    for (const socket of state.getWebSockets()) socket.close(1012, "SPM Cloud moved to Rust");
  }
  async fetch() { return new Response("Realtime has moved to the Rust origin", { status: 410 }); }
}
