# -*- coding: utf-8 -*-
"""Rebuild ui/index.html by injecting the UI sources into the built page, leaving the
   themes / COVERS / base HTML untouched.
   - engine.js  : between `window.__PROXY__ = ...;` and the next </script>
   - redesign   : inside the <style id="mk-redesign"> block
   - ident.json : window.__IDENT__ = {...}   (library snapshot; runtime-loaded later)"""
import io, sys, os
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8', errors='replace')

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))   # repo root
PAGE = os.path.join(ROOT, "ui", "index.html")
content = open(PAGE, encoding="utf-8").read()
before_kb = len(content.encode()) // 1024

# --- ident: keep EMPTY in the build output; the app scans the real library at runtime
#     (loadLibrary) so we never bake personal/NSFW data into the committed/shipped frontend ---
imark = "window.__IDENT__ = "
ia = content.find(imark)
if ia >= 0:
    iv = ia + len(imark)
    iend = content.find(";\nwindow.__PROXY__", iv)
    if iend >= 0:
        content = content[:iv] + "{}" + content[iend:]

# --- engine.js ---
engine = open(os.path.join(ROOT, "ui-src", "engine.js"), encoding="utf-8").read()
marker = "window.__PROXY__ = 'http://127.0.0.1:8902';\n"
i = content.find(marker); assert i >= 0, "PROXY marker not found"
estart = i + len(marker)
eend = content.find("</script>", estart); assert eend >= 0, "engine </script> not found"
content = content[:estart] + engine + "\n" + content[eend:]

# --- redesign.css ---
redesign = open(os.path.join(ROOT, "ui-src", "redesign.css"), encoding="utf-8").read()
rmark = '<style id="mk-redesign">'
r = content.find(rmark); assert r >= 0, "redesign block not found"
rstart = r + len(rmark)
rend = content.find("</style>", rstart); assert rend >= 0, "redesign </style> not found"
content = content[:rstart] + "\n" + redesign + "\n" + content[rend:]

open(PAGE, "w", encoding="utf-8").write(content)
print("rebuilt ui/index.html  (engine %d + redesign %d bytes)  %d KB -> %d KB" %
      (len(engine), len(redesign), before_kb, len(content.encode()) // 1024))
