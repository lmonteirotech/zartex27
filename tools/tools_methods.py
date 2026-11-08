import os
# Paths: REPO is the root of this repository (this file is in tools/). ZARTEX2_SRC is the folder of the Zartex 2 source
# code (the tool these generators read); set the environment variable ZARTEX2_SRC before running them.
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ZARTEX2_SRC = os.environ.get("ZARTEX2_SRC", "Zartex2")
import re, json
from commentutil import blank_comments
P = os.path.join(ZARTEX2_SRC, r'Zartex2\MissionScript\LuaMissionScript.cs')
src = open(P, encoding='utf-8-sig').read()
src = blank_comments(src).split(chr(10))
sig = json.load(open('oracle/api_signatures.json'))
for o in sig:
    i = o['line'] - 1
    # find the opening brace then match
    depth = 0; started = False; body = []
    j = i
    while j < len(src):
        line = src[j]
        # strip // comments crudely
        code = re.sub(r'//.*$', '', line)
        body.append(line)
        depth += code.count('{') - code.count('}')
        if '{' in code: started = True
        if started and depth == 0: break
        j += 1
    o['body'] = '\n'.join(body)
    o['end'] = j + 1
json.dump(sig, open('oracle/api_methods.json', 'w'), indent=1)
pat_node = 0; pat_actor = 0; other = []
for o in sig:
    b = o['body']
    if 'Nodes.Definitions.Add(new NodeDefinition' in b and 'return new Node(' in b: o['kind'] = 'node'
    elif 'Actors.Definitions.Add(new ActorDefinition' in b and 'return new Actor(' in b: o['kind'] = 'actor'
    else: o['kind'] = 'other'
from collections import Counter
print(Counter((o['class'][-6:], o['kind']) for o in sig))
print([o['name'] for o in sig if o['kind'] == 'other'])
