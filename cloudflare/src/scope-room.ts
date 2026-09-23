import { DurableObject } from "cloudflare:workers";

const MAX_MESSAGE_BYTES = 64 * 1024;
const MAX_BUFFERED_BYTES = 256 * 1024;

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
    const byteLength = typeof message === "string"
      ? new TextEncoder().encode(message).byteLength
      : message.byteLength;
    if (byteLength > MAX_MESSAGE_BYTES) {
      _socket.close(1009, "message too large");
      return;
    }
    for (const peer of this.ctx.getWebSockets()) {
      try {
        if (peer.bufferedAmount > MAX_BUFFERED_BYTES) {
          peer.close(1013, "consumer is too slow");
          continue;
        }
        peer.send(message);
      } catch {
        peer.close(1011, "broadcast failed");
      }
    }
  }

  webSocketClose(socket: WebSocket): void {
    socket.close();
  }

  webSocketError(socket: WebSocket): void {
    socket.close(1011, "realtime error");
  }
}
