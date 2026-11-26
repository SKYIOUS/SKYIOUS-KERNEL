import json
import sys
with open("pr_comments.json", "r", encoding="utf-8-sig") as f:
    data = json.load(f)
for c in data:
    print("--- " + c["user"]["login"] + " at " + c["created_at"] + " ---")
    print("Path: " + c.get("path", "N/A"))
    body = c["body"][:1000]
    sys.stdout.buffer.write(body.encode("utf-8", errors="replace"))
    sys.stdout.buffer.write(b"\n\n")
