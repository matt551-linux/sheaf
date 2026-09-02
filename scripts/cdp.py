# Minimal CDP client for the running Sheaf webview (WebView2 remote debugging).
# Usage: python scripts/cdp.py "<js expression>"   (prints the JSON result)
#        python scripts/cdp.py --console  (prints recent console + exceptions for 3s)
import json, sys, socket, struct, os, base64, urllib.request, time

PORT = int(os.environ.get("SHEAF_CDP_PORT", "9223"))

def ws_connect(url):
    _, rest = url.split("://", 1)
    hostport, path = rest.split("/", 1)
    host, port = hostport.split(":")
    s = socket.create_connection((host, int(port)))
    key = base64.b64encode(os.urandom(16)).decode()
    req = (f"GET /{path} HTTP/1.1\r\nHost: {hostport}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
           f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n")
    s.send(req.encode())
    buf = b""
    while b"\r\n\r\n" not in buf:
        buf += s.recv(4096)
    return s

def ws_send(s, text):
    data = text.encode()
    hdr = bytearray([0x81])
    mask = os.urandom(4)
    n = len(data)
    if n < 126: hdr.append(0x80 | n)
    elif n < 65536: hdr += bytes([0x80 | 126]) + struct.pack(">H", n)
    else: hdr += bytes([0x80 | 127]) + struct.pack(">Q", n)
    s.send(bytes(hdr) + mask + bytes(b ^ mask[i % 4] for i, b in enumerate(data)))

def ws_recv(s):
    def rd(n):
        b = b""
        while len(b) < n:
            c = s.recv(n - len(b))
            if not c: raise EOFError
            b += c
        return b
    h = rd(2)
    n = h[1] & 0x7F
    if n == 126: n = struct.unpack(">H", rd(2))[0]
    elif n == 127: n = struct.unpack(">Q", rd(8))[0]
    return rd(n).decode(errors="replace")

def main():
    targets = json.load(urllib.request.urlopen(f"http://127.0.0.1:{PORT}/json"))
    page = next(t for t in targets if t.get("type") == "page")
    s = ws_connect(page["webSocketDebuggerUrl"])
    mid = 0
    def call(method, **params):
        nonlocal mid
        mid += 1
        ws_send(s, json.dumps({"id": mid, "method": method, "params": params}))
        while True:
            m = json.loads(ws_recv(s))
            if m.get("id") == mid:
                return m
    if sys.argv[1:2] == ["--console"]:
        call("Runtime.enable"); call("Log.enable")
        s.settimeout(3)
        try:
            while True:
                m = json.loads(ws_recv(s))
                if m.get("method") in ("Runtime.consoleAPICalled", "Runtime.exceptionThrown", "Log.entryAdded"):
                    print(json.dumps(m["params"])[:600])
        except (socket.timeout, EOFError):
            pass
        return
    expr = sys.argv[1]
    r = call("Runtime.evaluate", expression=expr, awaitPromise=True, returnByValue=True)
    print(json.dumps(r.get("result", r), indent=1)[:4000])

main()
