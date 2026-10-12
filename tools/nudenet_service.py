"""Local NudeNet /infer adapter for the Rust Cloud (no images retained)."""
import argparse
import json
import io
import os
import threading
from email import policy
from email.parser import BytesParser
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

os.environ.setdefault("OPENCV_IO_MAX_IMAGE_PIXELS", "16777216")

MAX_REQUEST_BYTES = 32 * 1024 * 1024 + 8192


def serve(bind, port):
    import cv2
    import numpy as np
    from nudenet import NudeDetector
    from PIL import Image, ImageOps

    Image.MAX_IMAGE_PIXELS = 16777216

    detector = NudeDetector()
    inference_lock = threading.Lock()
    slots = threading.BoundedSemaphore(4)

    class Handler(BaseHTTPRequestHandler):
        def reply(self, status, data):
            payload = json.dumps(data, allow_nan=False).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

        def do_GET(self):
            self.reply(200 if self.path == "/health" else 404,
                       {"ok": self.path == "/health", "model": "NudeNet 320n"})

        def do_POST(self):
            if self.path != "/infer":
                self.reply(404, {"success": False})
                return
            if not slots.acquire(blocking=False):
                self.reply(503, {"success": False})
                return
            try:
                self.connection.settimeout(30)
                length = int(self.headers.get("Content-Length", "0"))
                if not 0 < length <= MAX_REQUEST_BYTES:
                    self.reply(413, {"success": False})
                    return
                content_type = self.headers.get("Content-Type", "")
                if not content_type.startswith("multipart/form-data;"):
                    self.reply(400, {"success": False})
                    return
                body = self.rfile.read(length)
                if len(body) != length:
                    self.reply(400, {"success": False})
                    return
                message = BytesParser(policy=policy.default).parsebytes(
                    ("Content-Type: " + content_type + "\r\nMIME-Version: 1.0\r\n\r\n").encode() + body)
                parts = [part for part in message.iter_parts()
                         if part.get_param("name", header="content-disposition") == "f1"]
                if len(parts) != 1:
                    self.reply(400, {"success": False})
                    return
                image_bytes = parts[0].get_payload(decode=True)
                # YSM files can contain AVIF/WebP bytes under a .png filename.
                # Identify by content and check dimensions before full decoding.
                with Image.open(io.BytesIO(image_bytes)) as source:
                    if source.width * source.height > Image.MAX_IMAGE_PIXELS or getattr(source, "n_frames", 1) != 1:
                        self.reply(422, {"success": False})
                        return
                    image = np.asarray(ImageOps.exif_transpose(source).convert("RGB"))[:, :, ::-1].copy()
                with inference_lock:
                    detections = detector.detect(image)
                detections = [{"class": str(item["class"]), "score": float(item["score"]),
                               "box": [int(v) for v in item["box"]]} for item in detections]
                self.reply(200, {"success": True, "prediction": [detections]})
            except (ValueError, TypeError, OSError, cv2.error):
                self.reply(422, {"success": False})
            except Exception:
                self.reply(500, {"success": False})
            finally:
                slots.release()

        def log_message(self, *_args):
            pass

    server = ThreadingHTTPServer((bind, port), Handler)
    print(f"NudeNet service listening on {bind}:{port}", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bind", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=8080)
    args = parser.parse_args()
    serve(args.bind, args.port)
