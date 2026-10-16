"""PC client for the http_server payload. Run:  python examples/pc/ps5_client.py

Asks for the PS5's IP, then lets you read its info, send it a toast,
or stop the server.
"""
import urllib.error
import urllib.parse
import urllib.request

PORT = 9954


def call(ip, path):
    url = f"http://{ip}:{PORT}{path}"
    try:
        with urllib.request.urlopen(url, timeout=5) as r:
            return f"[{r.status}] {r.read().decode(errors='replace').strip()}"
    except urllib.error.HTTPError as e:
        return f"[{e.code}] {e.read().decode(errors='replace').strip()}"
    except OSError as e:
        return f"cannot reach {url}: {e}"


def main():
    ip = input("PS5 IP (e.g. 192.168.1.49): ").strip()
    while True:
        print("\n1) info   2) send notification   3) stop server   0) exit")
        choice = input("> ").strip()
        if choice == "1":
            print(call(ip, "/info"))
        elif choice == "2":
            msg = input("message: ")
            print(call(ip, "/notify?msg=" + urllib.parse.quote_plus(msg)))
        elif choice == "3":
            print(call(ip, "/quit"))
        elif choice == "0":
            break


if __name__ == "__main__":
    main()
