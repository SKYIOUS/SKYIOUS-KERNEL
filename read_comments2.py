import json
with open("pr_comments.json", "r", encoding="utf-8-sig") as f:
    data = json.load(f)
for c in data:
    print("--- " + c["user"]["login"] + " at " + c["created_at"] + " ---")
    print("Path: " + c.get("path", "N/A"))
    print(c["body"][:1000])
    print()
