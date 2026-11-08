import os
# Paths: REPO is the root of this repository (this file is in tools/). ZARTEX2_SRC is the folder of the Zartex 2 source
# code (the tool these generators read); set the environment variable ZARTEX2_SRC before running them.
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ZARTEX2_SRC = os.environ.get("ZARTEX2_SRC", "Zartex2")
"""Per-method parity: generate one tiny Lua script per API method/overload, compile it with the original
LuaC (oracle) and with zartex-luac, compare success/failure and output bytes."""
import json, os, re, subprocess, sys, shutil

ROOT = REPO
ORACLE = os.environ.get('LUAC_ORACLE', 'LuaC.exe')  # the original compiler, built from the Zartex 2 sources
RUST = os.path.join(ROOT, 'target', 'release', 'zartex-luac.exe')
COV = ROOT + r'\oracle\cov'
M = json.load(open(ROOT + r'\oracle\api_methods.json'))

SKIP = {'LoadScriptFromFile', 'ImportMissionScriptFromFile', 'LoadFromFile'}

PRE3 = '''local P = MISSION.CreateCharacter(1,2,3,0, SKIN_TANNER, 1, "Player", -1,-1,1.0,0.0,0,1.0)
local V = MISSION.CreateVehicle(4,5,6,90, 2, 0)
local S = MISSION.LogicStart("Start")
local N = MISSION.Timer(1)
'''
PREDPL = '''local P = INITMISSION.CreateCharacter(1,2,3,0, 29, 1.0, 0.0, nil, 0, 0, true, 65537)
local V = INITMISSION.CreateVehicle(4,5,6,90, 36, 0)
local S = INITMISSION.LogicStart(0, "Start")
local N = INITMISSION.SetTimer(1)
'''

TABLES = {
    ('CreateActorSetting', 0): '{1, 2}',
    ('CreateAIPath', 1): '{{1,2,3,0},{4,5,6,0}}',
    ('DensityControl', 2): '{{36, 0.1}, {38, 0.2}}',
    ('PedestrianDensityControl', 1): '{{36, 0.1}, {38, 0.2}}',
}


def parse(p):
    d = None
    if '=' in p:
        p, d = [x.strip() for x in p.split('=', 1)]
    t, n = p.rsplit(' ', 1)
    return t, n, d


def argval(t, n, i, name, mode='std'):
    if (name, i) in TABLES: return TABLES[(name, i)]
    if mode == 'nilstr' and t == 'string': return 'nil'
    if mode == 'nilact' and t in ('Actor', 'Node'): return 'nil'
    if mode == 'edge':
        if t in ('int', 'uint', 'byte', 'short', 'long'): return ['2.7', '-1', '300', '1e3', '0'][i % 5]
        if t in ('float', 'double', 'decimal'): return ['-0.5', '1e3', '0.1', '3.999'][i % 4]
    if t == 'Actor':
        return 'V' if re.search(r'vehicle|truck|car|jag|zone', n, re.I) and not re.search(r'character', n, re.I) else 'P'
    if t == 'Node': return 'N'
    if t == 'Table': return '{}'
    if t == 'string': return '"x"'
    if t == 'bool': return 'true'
    if t in ('float', 'double', 'decimal'): return '1.5'
    return str((i % 3) + 1)  # int/uint/byte/short/long


def cases():
    out = []
    for o in M:
        if o['name'] in SKIP or o['class'].endswith('Package'): continue
        dpl = o['class'].endswith('DPL')
        ps = [parse(p) for p in o['params']]
        req = [p for p in ps if p[2] is None]
        ov = '_%d' % o['line']
        for variant, plist, mode in (('min', req, 'std'), ('full', ps, 'std'), ('nilstr', ps, 'nilstr'), ('nilact', ps, 'nilact'), ('edge', ps, 'edge'), ('edgemin', req, 'edge')):
            args = ', '.join(argval(t, n, i, o['name'], mode) for i, (t, n, d) in enumerate(plist))
            recv = 'INITMISSION' if dpl else 'MISSION'
            call = '%s.%s(%s)' % (recv, o['name'], args)
            out.append((('dpl_' if dpl else '') + o['name'] + ov + '_' + variant, dpl, call))
    # constructor-level extras: DPL package API
    out.append(('dpl_package_basic', True, 'local m = PACKAGE.CreateMission("M", 5, 1.5, 2.5, 3)\nPACKAGE.AddMission(m, 7)\nm.LogicStart(0, "s")'))
    out.append(('dpl_package_two', True, 'local a = PACKAGE.CreateMission("A", 5, 0, 0, 0)\nlocal b = PACKAGE.CreateMission("B", 6, 1, 2, 3)\nPACKAGE.AddMission(b, 2)\nPACKAGE.AddMission(a, 1)\nb.SetTimer(2)'))
    return out


def run(exe, args, cwd=None):
    r = subprocess.run([exe] + args, capture_output=True, text=True, cwd=cwd, timeout=60, errors='replace')
    return r.returncode, r.stdout + r.stderr


def main():
    shutil.rmtree(COV, ignore_errors=True)
    os.makedirs(COV + r'\lua'); os.makedirs(COV + r'\or'); os.makedirs(COV + r'\rs')
    cs = cases()
    bad = []; both_ok = 0; both_fail = 0
    for name, dpl, call in cs:
        pre = PREDPL if dpl else PRE3
        src = pre + call + '\n'
        lp = COV + r'\lua\%s.lua' % name
        open(lp, 'w', encoding='utf-8').write(src)
        ext = 'sp' if dpl else 'mpc'
        po = COV + r'\or\%s.%s' % (name, ext); pr = COV + r'\rs\%s.%s' % (name, ext)
        flags = ['-dpl'] if dpl else []
        _, oo = run(ORACLE, [lp, '-o', po] + flags)
        ok_o = os.path.exists(po) and ('Compilation success' in oo or oo.startswith('OK'))
        if not ok_o and os.path.exists(po): os.remove(po)
        _, ro = run(RUST, [lp, '-o', pr] + flags)
        ok_r = os.path.exists(pr) and 'Compilation success' in ro
        if ok_o != ok_r:
            bad.append((name, 'oracle=%s rust=%s' % (ok_o, ok_r), oo.strip().splitlines()[-3:], ro.strip().splitlines()[-2:]))
        elif ok_o:
            if open(po, 'rb').read() != open(pr, 'rb').read():
                bad.append((name, 'bytes differ', [], []))
            else:
                both_ok += 1
        else:
            both_fail += 1
    print('cases', len(cs), 'both ok & identical', both_ok, 'both fail', both_fail, 'MISMATCH', len(bad))
    json.dump(bad, open(ROOT + r'\coverage_mismatches.json', 'w'), indent=1)
    for b in bad: print(b[0], '|', b[1])


main()
