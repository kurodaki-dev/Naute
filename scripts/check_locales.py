#!/usr/bin/env python3
"""Check that every locale defines the keys of the English reference.

Keys whose value is the same in every language (paper sizes such as A4)
may be omitted; the app falls back to English for them.
"""
import pathlib, sys

ROOT = pathlib.Path(__file__).resolve().parent.parent / "ui" / "locales"
OPTIONAL = {"preset.a4", "preset.a5", "preset.a3", "preset.story", "kind.design", "canvas.text"}

def load(p):
    out = {}
    for line in p.read_text(encoding="utf-8").splitlines():
        if line.strip().startswith("#") or "=" not in line:
            continue
        k, v = line.split("=", 1)
        out[k.strip()] = v.strip()
    return out

ref = load(ROOT / "en.txt")
bad = 0
for f in sorted(ROOT.glob("*.txt")):
    d = load(f)
    missing = sorted(set(ref) - set(d) - OPTIONAL)
    extra = sorted(set(d) - set(ref))
    placeholders = [k for k in d if "{0}" in ref.get(k, "") and "{0}" not in d[k]]
    if missing or extra or placeholders:
        bad += 1
        print(f"{f.name}: missing={missing} extra={extra} placeholders={placeholders}")
print(f"{len(list(ROOT.glob('*.txt')))} locales, {len(ref)} keys" + ("" if not bad else f", {bad} with problems"))
sys.exit(1 if bad else 0)
