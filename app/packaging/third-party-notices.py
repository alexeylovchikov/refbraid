#!/usr/bin/env python3
"""Writes THIRD-PARTY-NOTICES.md (in the repository root): every library that is built into
RefBraid for Linux and Windows, its licence, and the licence texts with their copyright
lines, taken from the libraries' own packages in the cargo registry.

    python3 packaging/third-party-notices.py        (from app/; `make notices`)

Run it again after dependencies change (Cargo.lock). Needs the dependencies downloaded
(`cargo fetch`)."""
import hashlib
import json
import os
import re
import subprocess
import sys

APP = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(os.path.dirname(APP), "THIRD-PARTY-NOTICES.md")
TARGETS = ["x86_64-unknown-linux-gnu", "x86_64-pc-windows-gnu"]
LICENCE_FILE = re.compile(r"^(licen[sc]e|copying|notice|unlicense|copyright)", re.I)


def metadata(target):
    out = subprocess.run(["cargo", "metadata", "--format-version", "1", "--filter-platform", target], cwd=APP, capture_output=True, text=True, check=True)
    return json.loads(out.stdout)


def used_packages():
    """The packages reachable from the program through normal and build dependencies."""
    found = {}
    for target in TARGETS:
        meta = metadata(target)
        by_id = {p["id"]: p for p in meta["packages"]}
        nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
        root = meta["resolve"]["root"]
        todo, seen = [root], {root}
        while todo:
            node = nodes[todo.pop()]
            for dep in node["deps"]:
                if all(k["kind"] == "dev" for k in dep["dep_kinds"]):
                    continue
                if dep["pkg"] not in seen:
                    seen.add(dep["pkg"])
                    todo.append(dep["pkg"])
        for pid in seen - {root}:
            p = by_id[pid]
            found[(p["name"], p["version"])] = p
    return [found[k] for k in sorted(found, key=lambda k: (k[0].lower(), k[1]))]


def is_apache(text):
    return "Apache License" in text[:400] and "TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION" in text


def licence_files(pkg):
    """(file name, text) of the licence files of a package; the bundled fonts' ones too."""
    folder = os.path.dirname(pkg["manifest_path"])
    places = [folder]
    if os.path.isdir(os.path.join(folder, "fonts")):
        places.append(os.path.join(folder, "fonts"))
    files = []
    for place in places:
        for name in sorted(os.listdir(place)):
            path = os.path.join(place, name)
            fonts = place != folder
            if os.path.isfile(path) and (LICENCE_FILE.match(name) or (fonts and name.lower().endswith(".txt"))):
                try:
                    text = open(path, encoding="utf-8", errors="replace").read().strip()
                except OSError:
                    continue
                if text:
                    files.append((("fonts/" if fonts else "") + name, text))
    return files


def chosen(pkg, files):
    """Of a package's licence files, the ones that apply: where the licence is a choice that
    includes MIT, the MIT text (its copyright line is what has to be kept); the long Apache
    text is not repeated for every package — it is printed once at the end."""
    expr = pkg.get("license") or ""
    options = re.split(r"\s+OR\s+|/", expr)
    mit = [f for f in files if "mit" in f[0].lower() or f[1].lstrip().lower().startswith(("mit license", "the mit license", "permission is hereby granted"))]
    if any(o.strip(" ()") == "MIT" for o in options) and mit and " AND " not in expr:
        return mit[:1], False
    kept = [f for f in files if not is_apache(f[1])]
    return kept, len(kept) != len(files)


def main():
    packages = used_packages()
    apache_text = None
    blocks = {}  # hash of text -> [text, [who]]
    order = []
    plain = []   # packages with no licence file of their own
    needs_apache = []
    for p in packages:
        files = licence_files(p)
        if apache_text is None:
            apache_text = next((t for _, t in files if is_apache(t)), None)
        keep, apache = chosen(p, files)
        who = f"{p['name']} {p['version']}"
        if apache or "Apache-2.0" in (p.get("license") or "") and not keep:
            needs_apache.append(who)
        if not keep and not apache:
            plain.append(p)
        for name, text in keep:
            h = hashlib.sha256(re.sub(r"\s+", " ", text).encode()).hexdigest()
            if h not in blocks:
                blocks[h] = [text, []]
                order.append(h)
            blocks[h][1].append(f"{who} ({name})")

    out = []
    w = out.append
    w("# Third-party notices\n")
    w("RefBraid is built from the open-source libraries listed below; their code is part of the")
    w("program file for Linux and Windows. Each is used under the licence named beside it. Where a")
    w("library offers a choice of licences, RefBraid uses it under the MIT licence when that is")
    w("one of the choices, otherwise under the Apache License 2.0.\n")
    w("This file is generated by `app/packaging/third-party-notices.py` from the libraries' own")
    w("packages; run it again when the dependencies change.\n")
    w(f"## Libraries ({len(packages)})\n")
    w("| Library | Version | Licence | Source |")
    w("|---|---|---|---|")
    for p in packages:
        src = p.get("repository") or p.get("homepage") or f"https://crates.io/crates/{p['name']}"
        w(f"| {p['name']} | {p['version']} | {p.get('license') or 'see the package'} | {src} |")
    w("")
    w("## Fonts\n")
    w("The program file also carries the fonts that come with the `epaint_default_fonts` library:")
    w("Ubuntu Light (Ubuntu Font Licence 1.0), Hack (MIT, with Bitstream Vera terms), Noto Emoji")
    w("(SIL Open Font License 1.1) and emoji-icon-font (MIT). Their licence texts are among the")
    w("texts below. Other fonts are read from the system at run time and are not distributed.\n")
    w("## Licence texts\n")
    w("Identical texts are printed once, with the libraries they come from.\n")
    for h in order:
        text, who = blocks[h]
        w("### " + "; ".join(who) + "\n")
        w("```text")
        w(text.replace("```", "'''"))
        w("```\n")
    if plain:
        w("## Libraries whose package carries no licence file\n")
        w("Their licence is the one named in the table above; the text is in the library's source")
        w("repository.\n")
        for p in plain:
            w(f"- {p['name']} {p['version']} — {p.get('license')}")
        w("")
    if apache_text:
        w("## Apache License 2.0\n")
        w("Applies to the libraries used under it: " + ", ".join(needs_apache) + ".\n")
        w("```text")
        w(apache_text)
        w("```")
    with open(OUT, "w", encoding="utf-8") as f:
        f.write("\n".join(out) + "\n")
    print(f"{OUT}: {len(packages)} libraries, {len(order)} licence texts, {len(plain)} without a licence file, {os.path.getsize(OUT) // 1024} KB")


if __name__ == "__main__":
    sys.exit(main())
