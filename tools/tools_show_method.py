import os
# Paths: REPO is the root of this repository (this file is in tools/). ZARTEX2_SRC is the folder of the Zartex 2 source
# code (the tool these generators read); set the environment variable ZARTEX2_SRC before running them.
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ZARTEX2_SRC = os.environ.get("ZARTEX2_SRC", "Zartex2")
"""usage: python tools_show_method.py Name [Class]  -- prints every C# overload (comments stripped) with its parameter defaults"""
import json, sys, re
m = json.load(open(os.path.join(REPO, r'oracle\api_methods.json')))
name = sys.argv[1]; cls = sys.argv[2] if len(sys.argv) > 2 else None
for o in m:
    if o['name'] == name and (cls is None or o['class'].endswith(cls)):
        print('=== %s.%s (line %d)' % (o['class'], o['name'], o['line']))
        print('\n'.join(l for l in o['body'].split('\n') if l.strip()))
