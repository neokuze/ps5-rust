"""Test server for the http_test payload. Run on the PC:  python tools/test_server.py

GET  /hello   -> a short greeting the PS5 shows as a toast
POST /report  -> prints what the PS5 sent (model, temp, fw)
"""
import http.server
import socket
import sys

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8080


class Handler(http.server.BaseHTTPRequestHandler):
    def _reply(self, code, body):
        data = body.encode()
        self.send_response(code)
        self.send_header("Content-Type", "text/plain")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.path == "/hello":
            self._reply(200, "Hello PS5, this is your PC!")
        else:
            self._reply(404, "not found")

    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(length).decode(errors="replace")
        if self.path == "/report":
            print(f"\n=== report from {self.client_address[0]} ===\n{body}\n", flush=True)
            self._reply(200, "ok")
        else:
            self._reply(404, "not found")


if __name__ == "__main__":
    host = socket.gethostname()
    ips = {i[4][0] for i in socket.getaddrinfo(host, None, socket.AF_INET)}
    print(f"listening on port {PORT}; this PC's addresses: {', '.join(sorted(ips))}")
    # Own the port exclusively: with the default SO_REUSEADDR, Windows lets a
    # second copy bind 8080 too, and one of them silently gets no requests.
    class Server(http.server.ThreadingHTTPServer):
        allow_reuse_address = False

        def server_bind(self):
            if hasattr(socket, "SO_EXCLUSIVEADDRUSE"):
                self.socket.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
            super().server_bind()

    try:
        Server(("0.0.0.0", PORT), Handler).serve_forever()
    except OSError as e:
        sys.exit(f"cannot listen on port {PORT}: {e} (another test_server running?)")
