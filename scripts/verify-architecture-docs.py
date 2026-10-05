#!/usr/bin/env python3
"""Verify architecture/*.md: internal links resolve and file:line citations are in range."""
import os, re, sys, glob

ARCH = "architecture"
errors, warnings = [], []
md_files = sorted(glob.glob(f"{ARCH}/*.md"))

# ---------- 1. internal + repo-relative links resolve ----------
link_re = re.compile(r'\[([^\]]*)\]\(([^)\s]+)\)')
for f in md_files:
    base = os.path.dirname(f)
    for label, target in link_re.findall(open(f).read()):
        if target.startswith(('http://', 'https://', '#', 'mailto:')):
            continue
        # strip a GitHub line anchor (#L30 / #L30-L40) before checking the path
        path_part = target.split('#')[0]
        if not path_part:
            continue
        resolved = os.path.normpath(os.path.join(base, path_part))
        if not os.path.exists(resolved):
            errors.append(f"{f}: broken link -> {target} (resolves to {resolved})")

# ---------- 2. file:line citations are in range ----------
# Build basename -> real paths index
index = {}
for root, dirs, files in os.walk('.'):
    dirs[:] = [d for d in dirs if d not in ('.git', 'target', 'node_modules')]
    for fn in files:
        index.setdefault(fn, []).append(os.path.join(root, fn)[2:])

cite_re = re.compile(r'([A-Za-z0-9_./-]+\.(?:rs|yml|yaml|py|sh|json|md|toml)):(\d+)')
total_cites = 0
for f in md_files:
    txt = open(f).read()
    for path, ln in cite_re.findall(txt):
        total_cites += 1
        base = os.path.basename(path)
        cands = index.get(base)
        if not cands:
            errors.append(f"{f}: citation to unknown file {path}:{ln}")
            continue
        # prefer the exact relative path when the citation gives one; if that
        # fails and the basename is ambiguous (several crates ship a lib.rs or
        # Cargo.toml), fall back to the other candidates, because the bare form
        # cannot disambiguate which crate was meant
        norm = os.path.normpath(path)
        ordered = ([norm] if norm in cands else []) + [c for c in cands if c != norm]
        ok = False
        sizes = []
        for target in ordered:
            try:
                nlines = sum(1 for _ in open(target, errors='ignore'))
            except OSError:
                continue
            sizes.append(f"{target}={nlines}")
            if 1 <= int(ln) <= nlines:
                ok = True
                break
        if not ok:
            errors.append(f"{f}: {path}:{ln} out of range ({', '.join(sizes)})")

# ---------- 3. required section headings present in deep dives ----------
REQUIRED = ["Purpose", "Source layout", "Key types", "How it works", "Invariants",
            "Failure model", "Boundaries", "Tests and qualification", "Review focus", "Related"]
deep = [f for f in md_files if os.path.basename(f) not in
        ('overview.md', 'core.md', 'runner.md', 'drivers.md', 'evidence.md')]
for f in deep:
    h2 = re.findall(r'^## (.+)$', open(f).read(), re.M)
    for sec in REQUIRED:
        if sec not in h2:
            errors.append(f"{f}: missing required section '## {sec}'")

# ---------- 4. overview.md links to every deep dive ----------
ov = open(f"{ARCH}/overview.md").read()
ov_links = {os.path.basename(t) for _, t in link_re.findall(ov) if t.endswith('.md')}
for f in deep:
    if os.path.basename(f) not in ov_links:
        errors.append(f"overview.md does not link {os.path.basename(f)}")

# ---------- report ----------
print(f"markdown files:      {len(md_files)} ({len(deep)} deep dives)")
print(f"total lines:         {sum(sum(1 for _ in open(f, errors='ignore')) for f in md_files)}")
print(f"citations checked:   {total_cites}")
print(f"errors:              {len(errors)}")
for e in errors:
    print("  ERROR  ", e)
sys.exit(1 if errors else 0)
