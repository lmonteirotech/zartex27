import os
# Paths: REPO is the root of this repository (this file is in tools/). ZARTEX2_SRC is the folder of the Zartex 2 source
# code (the tool these generators read); set the environment variable ZARTEX2_SRC before running them.
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ZARTEX2_SRC = os.environ.get("ZARTEX2_SRC", "Zartex2")
import re, json
from commentutil import blank_comments
src = open(os.path.join(ZARTEX2_SRC, r'Zartex2\MissionScript\LuaMissionScript.cs'), encoding='utf-8-sig').read()
src = blank_comments(src).split(chr(10))
# find class boundaries
cls = None; out = []
depth_classes = []
for i, line in enumerate(src, 1):
    m = re.match(r'\s*public (?:static |abstract )?class (\w+)', line)
    if m: cls = m.group(1)
    m = re.match(r'\s*public (virtual |override |static )?([\w<>\[\],\.]+) (\w+)\((.*)\)\s*(\{.*)?$', line)
    if m and cls in ('LuaMissionScript', 'LuaMissionScriptDPL', 'LuaMissionPackage'):
        mods, ret, name, params, _ = m.groups()
        if name == cls: continue
        ps = []
        if params.strip():
            depth = 0; cur = ''
            for ch in params:
                if ch in '<(': depth += 1
                if ch in '>)': depth -= 1
                if ch == ',' and depth == 0: ps.append(cur.strip()); cur = ''
                else: cur += ch
            ps.append(cur.strip())
        out.append({'class': cls, 'line': i, 'ret': ret, 'name': name, 'mods': (mods or '').strip(), 'params': ps})
json.dump(out, open('api_signatures.json', 'w'), indent=1)
from collections import Counter
c = Counter((o['class'], o['name']) for o in out)
print(len(out), 'methods;', len(c), 'distinct names')
for k, v in Counter(o['class'] for o in out).items(): print(k, v)
print('overloaded:', [(k[1], v) for k, v in c.items() if v > 1])
