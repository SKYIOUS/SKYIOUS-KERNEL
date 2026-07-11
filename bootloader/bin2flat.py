"""Extract .text + .rdata from a PE (image-base=0) into a flat binary
   suitable for loading at the .text section's VA."""
import argparse, struct, sys

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--pe", required=True)
    ap.add_argument("--output", required=True)
    args = ap.parse_args()

    with open(args.pe, "rb") as f:
        data = f.read()

    pe_off = struct.unpack("<I", data[60:64])[0]
    num_sec = struct.unpack("<H", data[pe_off + 6 : pe_off + 8])[0]
    opt_sz = struct.unpack("<H", data[pe_off + 20 : pe_off + 22])[0]
    sec_start = pe_off + 24 + opt_sz

    sections = {}
    for i in range(num_sec):
        off = sec_start + i * 40
        name = data[off : off + 8].rstrip(b"\x00").decode("ascii", "replace")
        vsize, vaddr, rsize, roff = struct.unpack("<IIII", data[off + 8 : off + 24])
        sections[name] = {"vaddr": vaddr, "roff": roff, "rsize": rsize}

    # .text is always first (lowest VA)
    text = sections.get(".text")
    if not text:
        sys.exit("no .text section")

    text_base = text["vaddr"]
    total = 0
    parts = []
    for name in (".text", ".rdata"):
        s = sections.get(name)
        if not s:
            continue
        offset = s["vaddr"] - text_base
        content = data[s["roff"] : s["roff"] + s["rsize"]]
        parts.append((offset, content))
        end = offset + len(content)
        if end > total:
            total = end

    buf = bytearray(total)
    for offset, content in parts:
        buf[offset : offset + len(content)] = content

    with open(args.output, "wb") as f:
        f.write(buf)

    sys.stderr.write(f"wrote {len(buf)} B to {args.output}\n")


if __name__ == "__main__":
    main()
