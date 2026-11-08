import os
# Paths: REPO is the root of this repository (this file is in tools/). ZARTEX2_SRC is the folder of the Zartex 2 source
# code (the tool these generators read); set the environment variable ZARTEX2_SRC before running them.
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ZARTEX2_SRC = os.environ.get("ZARTEX2_SRC", "Zartex2")
"""Generates src/api_gen.rs from the C# LuaMissionScript source (api_methods.json).

Handles the regular shapes (logic node creators, simple delegations). Everything it cannot
translate with confidence is listed in gen_report.txt and ported by hand.
"""
import json, re, sys

M = json.load(open(os.path.join(REPO, r'oracle\api_methods.json')))

PROP = {  # C# class -> (type id, rust PropValue ctor with %s as the value, value kind)
    'WireCollectionProperty': (19, 'PropValue::Int(%s)', 'int'),
    'FloatProperty': (2, 'PropValue::Float(%s)', 'float'),
    'FlagsProperty': (9, 'PropValue::Int(%s)', 'int'),
    'IntegerProperty': (1, 'PropValue::Int(%s)', 'int'),
    'EnumProperty': (6, 'PropValue::Int(%s)', 'int'),
    'ActorProperty': (7, 'PropValue::Int(%s)', 'actor'),
    'LocalisedStringProperty': (20, 'PropValue::Int(%s)', 'int'),
    'StringProperty': (3, 'PropValue::Str(%s)', 'short'),
    'BooleanProperty': (4, 'PropValue::Bool(%s)', 'bool'),
    'ObjectTypeProperty': (28, 'PropValue::Int(%s)', 'int'),
    'VehicleTintProperty': (29, 'PropValue::Int(%s)', 'int'),
    'AnimationTypeProperty': (30, 'PropValue::Int(%s)', 'int'),
}

RTYPE = {'int': 'i32', 'uint': 'u32', 'byte': 'u8', 'short': 'i16', 'long': 'i64', 'float': 'f32',
         'double': 'f64', 'string': 'Option<String>', 'bool': 'bool', 'Actor': 'Option<usize>',
         'Node': 'Option<usize>', 'decimal': 'f64'}


def snake(n):
    s = re.sub(r'(?<=[a-z0-9])([A-Z])', r'_\1', n)
    s = re.sub(r'([A-Z]+)([A-Z][a-z])', r'\1_\2', s)
    return s.lower()


def split_top(s, sep=','):
    out, depth, cur, instr = [], 0, '', False
    i = 0
    while i < len(s):
        ch = s[i]
        if ch == '"' and s[i - 1] != '\\':
            instr = not instr
        if not instr:
            if ch in '(<[{': depth += 1
            if ch in ')>]}': depth -= 1
            if ch == sep and depth == 0:
                out.append(cur.strip()); cur = ''; i += 1; continue
        cur += ch; i += 1
    if cur.strip(): out.append(cur.strip())
    return out


def parse_params(plist):
    ps = []
    for p in plist:
        d = None
        if '=' in p:
            p, d = [x.strip() for x in p.split('=', 1)]
        t, n = p.rsplit(' ', 1)
        ps.append({'type': t, 'name': n, 'default': d})
    return ps


class Unsupported(Exception):
    pass


def rust_default(ty, d):
    """Default literal for the Lua glue (Option<...> for the extraction helpers)."""
    if d is None: return 'None'
    if ty == 'string':
        if d == 'null': return 'Some(None)'
        m = re.fullmatch(r'"(.*)"', d)
        if not m: raise Unsupported('string default ' + d)
        return 'Some(Some("%s"))' % m.group(1)
    if ty == 'bool':
        return 'Some(%s)' % d
    if ty == 'Actor' or ty == 'Node':
        return 'None'
    m = re.fullmatch(r'(-?[0-9.]+)[fFdD]?', d)
    if m:
        v = m.group(1)
        if ty in ('float', 'double') and '.' not in v: v += '.0'
        if ty in ('int', 'uint', 'byte', 'short', 'long') and '.' in v: raise Unsupported('int default ' + d)
        return 'Some(%s)' % v
    m = re.fullmatch(r'(0x[0-9A-Fa-f]+)', d)
    if m: return 'Some(%s)' % d
    raise Unsupported('default ' + d)


KW = {'type', 'match', 'fn', 'ref', 'in', 'loop', 'move', 'box', 'mod', 'use', 'where', 'impl'}


def rn(n):
    return n + '_' if n in KW else n


class Ctx:
    def __init__(self, params):
        self.p = {x['name']: x for x in params}
        self.actor_i = []
        self.str_s = []


def tr(expr, ctx, want=None):
    """Translate a small C# expression into Rust. `want` = rust numeric type for literals."""
    e = expr.strip()
    # strip casts
    m = re.fullmatch(r'\((int|float|short|byte|uint|long|double)\)\s*(.+)', e)
    if m:
        rt = {'int': 'i32', 'float': 'f32', 'short': 'i16', 'byte': 'u8', 'uint': 'u32', 'long': 'i64', 'double': 'f64'}[m.group(1)]
        inner = tr(m.group(2), ctx)
        return '(%s as %s)' % (inner, rt)
    m = re.fullmatch(r'(-?\d+(?:\.\d+)?)[fFdD]?', e)
    if m:
        v = m.group(1)
        if want in ('f32', 'f64'):
            return v + ('.0' if '.' not in v else '') + '_' + want
        if want:
            return v + '_' + want
        return v
    if e == 'null': return 'None'
    if e in ('true', 'false'): return e
    m = re.fullmatch(r'"(.*)"', e)
    if m: return '"%s"' % m.group(1)
    m = re.fullmatch(r'(\w+) == null \? (-?\d+) : (\w+)\.index', e)
    if m and m.group(1) in ctx.p:
        return '%s.map(|v| v as i32).unwrap_or(%s)' % (rn(m.group(1)), m.group(2))
    m = re.fullmatch(r'(\w+)\.index', e)
    if m and m.group(1) in ctx.p and ctx.p[m.group(1)]['type'] == 'Actor':
        if m.group(1) not in ctx.actor_i: ctx.actor_i.append(m.group(1))
        return '%s_i' % rn(m.group(1))
    m = re.fullmatch(r'(\w+)', e)
    if m and e in ctx.p:
        p = ctx.p[e]
        if p['type'] in ('Actor', 'Node'):
            if e not in ctx.actor_i: ctx.actor_i.append(e)
            return '%s_i' % rn(e)
        return rn(e)
    m = re.fullmatch(r'missionData\.LogicData\.StringCollection\.findStringIdByValueOrCreateNew\((.+)\)', e)
    if m:
        a = m.group(1).strip()
        if a in ctx.p and ctx.p[a]['type'] == 'string':
            if a not in ctx.str_s: ctx.str_s.append(a)
            return 'st.string_id(%s_s)' % rn(a)
        mm = re.fullmatch(r'"(.*)"', a)
        if mm: return 'st.string_id("%s")' % mm.group(1)
        raise Unsupported('findString arg ' + a)
    m = re.fullmatch(r'(-?)(\w+)', e)
    if m and m.group(2) in ctx.p:
        return '(-%s)' % rn(m.group(2))
    raise Unsupported('expr ' + e)


def balanced(s, start):
    """s[start] == '(' -> index after the matching ')'"""
    depth = 0
    i = start
    instr = False
    while i < len(s):
        ch = s[i]
        if ch == '"' and s[i - 1] != '\\': instr = not instr
        if not instr:
            if ch == '(': depth += 1
            elif ch == ')':
                depth -= 1
                if depth == 0: return i + 1
        i += 1
    raise Unsupported('unbalanced')


def strip_comments(b):
    b = re.sub(r'/\*.*?\*/', '', b, flags=re.S)
    return re.sub(r'//[^\n]*', '', b)


def gen_validations(body, ctx):
    out = []
    for m in re.finditer(r'if \((.*?)\)\s*(?:\{\s*)?throw new ScriptRuntimeException\((".*?")\);', body, flags=re.S):
        cond, msg = m.group(1).strip(), m.group(2)
        c = None
        mm = re.fullmatch(r'(\w+) == null', cond)
        if mm and mm.group(1) in ctx.p:
            t = ctx.p[mm.group(1)]['type']
            if t in ('Actor', 'Node'): c = '%s.is_none()' % mm.group(1)
            elif t in ('int', 'float', 'short', 'byte', 'uint', 'long', 'double', 'bool'): c = 'false'
            elif t == 'string': c = '%s.is_none()' % mm.group(1)
        mm = re.fullmatch(r'(\w+)\.TheActor\.TypeId (!=|==) (\d+)', cond)
        if mm and mm.group(1) in ctx.p:
            n = mm.group(1)
            c = 'st.logic.actors.definitions[%s.ok_or_else(nre)?].type_id %s %s' % (n, mm.group(2), mm.group(3))
        mm = re.fullmatch(r'(\w+) (<=|<|>=|>|==|!=) (-?\d+)', cond)
        if mm and mm.group(1) in ctx.p:
            c = '%s %s %s' % (mm.group(1), mm.group(2), mm.group(3))
        if c is None: raise Unsupported('validation ' + cond)
        out.append('    if %s { return Err(rt_err(%s)); }' % (c, msg))
    return out


def gen_node(o, params, ctx):
    body = strip_comments(o['body'])
    # color
    mc = re.search(r'Color = new NodeColor\((.*?)\),', body)
    if not mc: raise Unsupported('color')
    cargs = split_top(mc.group(1))
    if len(cargs) != 4: raise Unsupported('color args')
    ctype = lambda a: ('f64' if True else '')
    color = 'node_color(%s as f64, %s as f64, %s as f64, %s as f64)' % tuple(
        (rn(a) if a in ctx.p else a) for a in cargs[:3] + [cargs[3]])
    for a in cargs[:3]:
        if a not in ctx.p: raise Unsupported('color arg ' + a)
    mt = re.search(r'TypeId = (\d+),', body)
    if not mt: raise Unsupported('typeid')
    mf = re.search(r'\n\s*Flags = (0x[0-9A-Fa-f]+|\d+),', body)
    flags = mf.group(1) if mf else '0'
    mp = re.search(r'Properties = new List<NodeProperty>\s*(?:\(\))?\s*\{', body)
    if not mp: raise Unsupported('properties')
    rest = body[mp.end():]
    props = []
    pos = 0
    while True:
        m = re.compile(r'\s*new (\w+)\(').match(rest, pos)
        if not m: break
        cls = m.group(1)
        end = balanced(rest, m.end() - 1)
        args = rest[m.end():end - 1]
        m2 = re.compile(r'\s*\{\s*StringId\s*=\s*\(short\)missionData\.LogicData\.StringCollection\.findStringIdByValueOrCreateNew\("([^"]+)"\)\s*\}\s*,?').match(rest, end)
        if not m2: raise Unsupported('prop tail near ' + cls)
        props.append((cls, args, m2.group(1)))
        pos = m2.end()
    if not props: raise Unsupported('no props')
    lines = []
    lines += gen_validations(body, ctx)
    plines = []
    for i, (cls, args, name) in enumerate(props):
        if cls == 'Float3Property':
            mv = re.fullmatch(r'new Vector4\((.*)\)', args.strip())
            if not mv: raise Unsupported('Float3 arg ' + args)
            comps = split_top(mv.group(1))
            if len(comps) != 4: raise Unsupported('Float3 comps')
            vs = ['(%s) as f32' % tr(c, ctx) for c in comps]
            plines.append('            st.prop("%s", 17, PropValue::Vec4(Vec4 { x: %s, y: %s, z: %s, w: %s }))' % (name, vs[0], vs[1], vs[2], vs[3]))
            continue
        if cls not in PROP: raise Unsupported('prop class ' + cls)
        tid, ctor, kind = PROP[cls]
        if i == 0:
            if cls != 'WireCollectionProperty': raise Unsupported('first prop is ' + cls)
            continue
        want = {'float': 'f32', 'int': 'i32', 'short': 'i16', 'actor': 'i32', 'bool': None}[kind] if kind != 'bool' else None
        v = tr(args, ctx, want)
        if kind == 'float': v = '(%s) as f32' % v
        if 'st.string_id(' in v:
            plines.append('            { let sv = %s; st.prop("%s", %d, %s) }' % (v, name, tid, ctor % 'sv'))
        else:
            plines.append('            st.prop("%s", %d, %s)' % (name, tid, ctor % v))
    props_rs = ',\n'.join(plines)
    note = [p for p in params if p['name'] == 'note']
    note_expr = 'note.as_deref()' if note else 'None'
    lines.append('    let color = %s;' % color)
    for an in ctx.actor_i:
        lines.append('    let %s_i = %s.ok_or_else(nre)? as i32;' % (rn(an), rn(an)))
    for an in ctx.str_s:
        lines.append('    let %s_s = %s.as_deref().ok_or_else(nre)?;' % (rn(an), rn(an)))
    lines.append('    Ok(st.create_node(%s, %s, color, %s, |st| vec![\n%s\n    ]))' % (mt.group(1), note_expr, flags, props_rs))
    return lines


def gen_delegate(o, params, ctx, byname):
    body = strip_comments(o['body'])
    m = re.search(r'return (\w+)\((.*)\);\s*\}\s*$', body, flags=re.S)
    if not m: raise Unsupported('not a delegation')
    callee, args = m.group(1), split_top(m.group(2))
    cands = [c for c in byname.get(callee, []) if len(c['params']) >= len(args) and len(parse_params(c['params'])) - sum(1 for p in parse_params(c['params']) if p['default'] is not None) <= len(args)]
    if len(byname.get(callee, [])) != 1: raise Unsupported('callee overloaded or unknown: ' + callee)
    c = byname[callee][0]
    cps = parse_params(c['params'])
    if len(args) > len(cps): raise Unsupported('too many args')
    out_args = []
    for i, a in enumerate(args):
        t = cps[i]['type']
        rt = RTYPE.get(t)
        if t in ('Actor', 'Node'):
            if a == 'null': out_args.append('None')
            elif a in ctx.p: out_args.append(a)
            else: raise Unsupported('actor arg ' + a)
            continue
        if t == 'string':
            if a in ctx.p: out_args.append(a)
            else:
                mm = re.fullmatch(r'"(.*)"', a)
                if not mm: raise Unsupported('string arg ' + a)
                out_args.append('Some("%s".to_string())' % mm.group(1))
            continue
        v = tr(a, ctx, rt)
        if a in ctx.p and ctx.p[a]['type'] != t:
            v = '(%s as %s)' % (v, rt)
        out_args.append(v)
    # remaining defaults
    for p in cps[len(args):]:
        out_args.append(default_value_rs(p))
    return ['    %s(st, %s)' % (fname(c), ', '.join(out_args))]


def default_value_rs(p):
    t, d = p['type'], p['default']
    if d is None: raise Unsupported('missing default')
    if t == 'string':
        if d == 'null': return 'None'
        return 'Some(%s.to_string())' % d
    if t == 'bool': return d
    if t in ('Actor', 'Node'): return 'None'
    m = re.fullmatch(r'(-?[0-9.]+)[fFdD]?', d)
    if m:
        v = m.group(1)
        rt = RTYPE[t]
        if rt in ('f32', 'f64') and '.' not in v: v += '.0'
        return '%s_%s' % (v, rt)
    if re.fullmatch(r'0x[0-9A-Fa-f]+', d): return '%s_%s' % (d, RTYPE[t])
    raise Unsupported('default ' + d)


def fname(o):
    return ('dpl_' if o['class'].endswith('DPL') else '') + snake(o['name'])


def main():
    byname = {}
    for o in M:
        o['body'] = o['body'].replace('this.', '')
        b = o['body']
        o['kind'] = 'node' if ('Nodes.Definitions.Add(new NodeDefinition' in b and 'return new Node(' in b) else ('actor' if ('Actors.Definitions.Add(new ActorDefinition' in b and 'return new Actor(' in b) else 'other')
        o['pp'] = parse_params(o['params'])
        byname.setdefault(o['name'], []).append(o)
    # separate namespaces: DPL class overrides
    base_by = {}
    for o in M:
        if o['class'] == 'LuaMissionScript': base_by.setdefault(o['name'], []).append(o)
    dpl_by = {}
    for o in M:
        if o['class'] == 'LuaMissionScriptDPL': dpl_by.setdefault(o['name'], []).append(o)
    out = ['// @generated by tools_gen_api.py from LuaMissionScript.cs -- do not edit; fix the generator or port by hand.',
           '#![allow(unused_variables, unused_mut, non_snake_case, clippy::all)]',
           'use crate::binio::*;', 'use crate::logic::PropValue;', 'use crate::luaapi::*;', 'use crate::script::*;',
           'use mlua::{Lua, MultiValue, Result as LuaResult, Table, Value};', '',
           'fn nre() -> mlua::Error { rt_err("Object reference not set to an instance of an object.") }', '']
    report = []
    done = []  # (obj, rust fn name, ret kind)
    pending = [o for o in M if o['class'] != 'LuaMissionPackage']
    gen_names = set()
    rounds = 0
    while True:
        progressed = False
        still = []
        for o in pending:
            lst = (base_by if o['class'] == 'LuaMissionScript' else dpl_by)[o['name']]
            if len(lst) != 1:
                still.append((o, 'overloaded')); continue
            params = o['pp']
            ctx = Ctx(params)
            try:
                for pp_ in params:
                    if pp_['type'] not in RTYPE: raise Unsupported('param type ' + pp_['type'])
                if o['kind'] == 'node':
                    body = gen_node(o, params, ctx)
                elif o['kind'] == 'other' and o['ret'] == 'Node' and re.search(r'return \w+\(', strip_comments(o['body'])) and 'Definitions' not in o['body']:
                    callee = re.search(r'return (\w+)\(', strip_comments(o['body'])).group(1)
                    if callee not in gen_names and not (o['class'].endswith('DPL') and callee in gen_names):
                        still.append((o, 'callee not generated yet: ' + callee)); continue
                    body = gen_delegate(o, params, ctx, base_by if o['class'] == 'LuaMissionScript' else {**base_by, **dpl_by})
                else:
                    still.append((o, 'kind=%s' % o['kind'])); continue
            except Unsupported as e:
                still.append((o, str(e))); continue
            sig = ', '.join('%s: %s' % (rn(p['name']), RTYPE[p['type']]) for p in params)
            out.append('pub fn %s(st: &mut ScriptState%s) -> LuaResult<(usize, usize)> {' % (fname(o), (', ' + sig) if sig else ''))
            out += body
            out.append('}')
            out.append('')
            done.append((o, fname(o), 'node'))
            gen_names.add(o['name'])
            progressed = True
        pending = [o for o, _ in still]
        reasons = {id(o): r for o, r in still}
        if not progressed or not pending: break
    for o in pending:
        report.append('MANUAL (%s): %s.%s' % (reasons[id(o)], o['class'], o['name']))
    # glue
    out.append('pub fn register_generated(lua: &Lua, t: &Table, st: &State, dpl: bool) -> LuaResult<()> {')
    for o, fn, ret in done:
        is_dpl = o['class'].endswith('DPL')
        cond = 'dpl' if is_dpl else 'true'
        ex = []
        for i, p in enumerate(o['pp']):
            ty, nm = p['type'], re.sub(r'^(type|match|fn|ref)$', r'\1_', p['name'])
            d = rust_default(ty, p['default'])
            if ty == 'int': ex.append('let %s = a.int(%d, %s)?;' % (nm, i, d))
            elif ty == 'uint': ex.append('let %s = a.uint(%d, %s)?;' % (nm, i, d))
            elif ty == 'byte': ex.append('let %s = a.byte(%d, %s)?;' % (nm, i, d))
            elif ty == 'short': ex.append('let %s = a.short(%d, %s)?;' % (nm, i, d))
            elif ty == 'long': ex.append('let %s = a.long(%d, %s)?;' % (nm, i, d))
            elif ty == 'float': ex.append('let %s = a.float(%d, %s)?;' % (nm, i, d))
            elif ty == 'double': ex.append('let %s = a.double(%d, %s)?;' % (nm, i, d))
            elif ty == 'bool': ex.append('let %s = a.boolean(%d, %s)?;' % (nm, i, d))
            elif ty == 'string': ex.append('let %s = a.string(%d, %s)?;' % (nm, i, d))
            elif ty == 'Actor': ex.append('let %s = a.actor(%d)?;' % (nm, i))
            elif ty == 'Node': ex.append('let %s = a.node(%d)?;' % (nm, i))
            else: raise SystemExit('glue type ' + ty)
        call = ', '.join(re.sub(r'^(type|match|fn|ref)$', r'\1_', p['name']) for p in o['pp'])
        out.append('    if %s {' % cond)
        out.append('        let st2 = st.clone();')
        out.append('        t.set("%s", lua.create_function(move |lua, args: MultiValue| {' % o['name'])
        out.append('            let a = Args::new(args, |v| matches!(v, Value::Table(_)));')
        for e in ex: out.append('            ' + e)
        out.append('            let (idx, wire) = { let mut s = st2.borrow_mut(); %s(&mut s%s)? };' % (fn, (', ' + call) if call else ''))
        out.append('            ret_node(lua, &st2, idx, wire)')
        out.append('        })?)?;')
        out.append('    }')
    out.append('    Ok(())')
    out.append('}')
    open(os.path.join(REPO, r'zartex-core\src\api_gen.rs'), 'w', encoding='utf-8').write('\n'.join(out) + '\n')
    open(os.path.join(REPO, 'tools', 'gen_report.txt'), 'w', encoding='utf-8').write('\n'.join(report) + '\n')
    print('generated', len(done), 'methods; manual', len(report))


main()


def gen_consts():
    src = open(os.path.join(ZARTEX2_SRC, r'Zartex2\MissionScript\LuaMissionScript.cs'), encoding='utf-8-sig').read()
    col = open(os.path.join(ZARTEX2_SRC, r'Zartex2\MissionScript\Objects\Types\Collectable.cs'), encoding='utf-8-sig').read()
    enum = {}
    em = re.search(r'enum CollectableType[^{]*\{(.*?)\}', col, flags=re.S).group(1)
    em = re.sub(r'/\*.*?\*/', '', em, flags=re.S); em = re.sub(r'//[^\n]*', '', em)
    for n, v in re.findall(r'(\w+)\s*=\s*(\d+)', em): enum[n] = int(v)
    consts = []
    # only the LoadScriptFromFile (Driv3r) body, uncommented lines
    start = src.index('public static LuaMissionScript LoadScriptFromFile')
    end = src.index('DynValue res = script.DoFile(filepath);', start)
    body = src[start:end]
    body = re.sub(r'/\*.*?\*/', '', body, flags=re.S)
    body = re.sub(r'//[^\n]*', '', body)
    for n, e in re.findall(r'script\.Globals\["(\w+)"\]\s*=\s*([^;]+);', body):
        e = e.strip()
        if e.startswith('typeof'): continue
        m = re.fullmatch(r'CollectableType\.(\w+)', e)
        if m: consts.append((n, str(float(enum[m.group(1)])))); continue
        m = re.fullmatch(r'(-?)(0x[0-9A-Fa-f]+|\d+)', e)
        if m:
            v = int(m.group(2), 0) * (-1 if m.group(1) else 1)
            consts.append((n, '%s.0' % v)); continue
        print('const unsupported', n, e)
    vt = re.search(r'public static class VehicleType\s*\{(.*?)\};', src, flags=re.S).group(1)
    vt = re.sub(r'//[^\n]*', '', vt)
    veh = re.findall(r'public static readonly int (\w+) = (\d+);', vt)
    out = ['// @generated by tools_gen_api.py -- constants copied from LuaMissionScript.LoadScriptFromFile',
           'use mlua::{Lua, Result as LuaResult};', '',
           'pub fn register_constants(lua: &Lua) -> LuaResult<()> {', '    let g = lua.globals();']
    for n, v in consts: out.append('    g.set("%s", %sf64)?;' % (n, v))
    out.append('    let vt = lua.create_table()?;')
    for n, v in veh: out.append('    vt.set("%s", %s.0f64)?;' % (n, v))
    out.append('    g.set("VehicleType", vt)?;')
    out.append('    Ok(())')
    out.append('}')
    open(os.path.join(REPO, r'zartex-core\src\api_consts.rs'), 'w', encoding='utf-8').write('\n'.join(out) + '\n')
    print('consts', len(consts), 'vehicle types', len(veh))


gen_consts()


def gen_nodenames():
    src = open(os.path.join(ZARTEX2_SRC, r'Zartex2\MissionScript\Logic\NodeTypes.cs'), encoding='utf-8-sig').read()
    parts = re.split(r'public static IDictionary<int, string> (\w+)', src)
    out = ['// @generated by tools_gen_api.py from NodeTypes.cs', '']
    for name, body in zip(parts[1::2], parts[2::2]):
        body = body.split('public static string')[0]
        body = re.sub(r'//[^\n]*', '', body)
        ents = re.findall(r'\{\s*(\d+)\s*,\s*"([^"]*)"\s*\}', body)
        rname = {'LogicNodeTypes': 'LOGIC_NODE_TYPES', 'LogicNodeTypesDPL': 'LOGIC_NODE_TYPES_DPL', 'ActorNodeTypes': 'ACTOR_NODE_TYPES'}[name]
        out.append('pub static %s: &[(u8, &str)] = &[' % rname)
        for k, v in ents: out.append('    (%s, "%s"),' % (k, v))
        out.append('];')
        out.append('')
    out.append('''pub fn lookup(table: &[(u8, &'static str)], id: u8) -> Option<&'static str> {
    table.iter().find(|(k, _)| *k == id).map(|(_, v)| *v)
}''')
    open(os.path.join(REPO, r'zartex-core\src\nodenames.rs'), 'w', encoding='utf-8').write('\n'.join(out) + '\n')
    print('nodenames written')


gen_nodenames()
