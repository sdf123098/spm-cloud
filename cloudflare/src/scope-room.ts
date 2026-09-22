import { DurableObject } from "cloudflare:workers";

/** One realtime room per Cloud scope. Ordering and fan-out are per DO instance. */
export class ScopeRoom extends DurableObject<Env> {
  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
  }

  async fetch(request: Request): Promise<Response> {
    if (request.headers.get("Upgrade")?.toLowerCase() !== "websocket") {
      return new Response("WebSocket upgrade required", { status: 426 });
    }
    const pair = new WebSocketPair();
    const client = pair[0];
    const server = pair[1];
    this.ctx.acceptWebSocket(server);
    return new Response(null, { status: 101, webSocket: client });
  }

  webSocketMessage(_socket: WebSocket, message: string | ArrayBuffer): void {
    for (const peer of this.ctx.getWebSockets()) {
      try {
        peer.send(message);
      } catch {
        peer.close(1011, "broadcast failed");
      }
    }
  }

  webSocketClose(socket: WebSocket): void {
    socket.close();
  }
}
