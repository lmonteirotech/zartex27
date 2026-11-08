import re, sys
from docx import Document
from docx.enum.table import WD_TABLE_ALIGNMENT
from docx.enum.text import WD_ALIGN_PARAGRAPH
from docx.oxml import OxmlElement
from docx.oxml.ns import qn
from docx.shared import Cm, Pt, RGBColor

SRC, OUT, TITLE, FOOT = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
doc = Document()


def rfonts(rpr, name):
    rf = rpr.find(qn('w:rFonts'))
    if rf is None:
        rf = OxmlElement('w:rFonts')
        rpr.insert(0, rf)
    for a in ('w:ascii', 'w:hAnsi', 'w:eastAsia', 'w:cs'):
        rf.set(qn(a), name)


def style_font(style, name, size=None, bold=None, color=None):
    style.font.name = name
    rfonts(style.element.get_or_add_rPr(), name)
    if size:
        style.font.size = Pt(size)
    if bold is not None:
        style.font.bold = bold
    if color:
        style.font.color.rgb = RGBColor.from_string(color)


style_font(doc.styles['Normal'], 'Arial', 10.5)
doc.styles['Normal'].paragraph_format.space_after = Pt(5)
for n, s in (('Heading 1', 17), ('Heading 2', 13), ('Heading 3', 11.5)):
    style_font(doc.styles[n], 'Arial', s, True, '1F3864')
    doc.styles[n].paragraph_format.keep_with_next = True
style_font(doc.styles['List Bullet'], 'Arial', 10.5)
sec = doc.sections[0]
sec.page_width, sec.page_height = Cm(21), Cm(29.7)
sec.left_margin = sec.right_margin = sec.top_margin = sec.bottom_margin = Cm(1.27)
sec.footer_distance = Cm(0.6)


def field(p, instr, placeholder, size):
    for kind in ('begin', None, 'separate', 'text', 'end'):
        r = p.add_run()
        r.font.size = Pt(size)
        if kind in ('begin', 'separate', 'end'):
            fc = OxmlElement('w:fldChar')
            fc.set(qn('w:fldCharType'), kind)
            r._r.append(fc)
        elif kind is None:
            it = OxmlElement('w:instrText')
            it.set(qn('xml:space'), 'preserve')
            it.text = ' %s ' % instr
            r._r.append(it)
        else:
            r.text = placeholder


fp = sec.footer.paragraphs[0]
fp.alignment = WD_ALIGN_PARAGRAPH.CENTER
r = fp.add_run(FOOT)
r.font.size = Pt(8)
fp2 = sec.footer.add_paragraph()
fp2.alignment = WD_ALIGN_PARAGRAPH.CENTER
r = fp2.add_run('Page ')
r.font.size = Pt(9)
field(fp2, 'PAGE', '1', 9)
r = fp2.add_run(' of ')
r.font.size = Pt(9)
field(fp2, 'NUMPAGES', '1', 9)

TOKEN = re.compile(r'(\*\*.+?\*\*|`[^`]+`|\[[^\]]+\]\([^)]+\))')


def rich(p, text, size=None):
    for part in TOKEN.split(text):
        if not part:
            continue
        if part.startswith('**') and part.endswith('**') and len(part) > 4:
            r = p.add_run(part[2:-2])
            r.bold = True
        elif part.startswith('`') and part.endswith('`'):
            r = p.add_run(part[1:-1])
            r.font.name = 'Consolas'
            rfonts(r._r.get_or_add_rPr(), 'Consolas')
            r.font.size = Pt((size or 10.5) - 1)
            continue
        elif part.startswith('['):
            m = re.match(r'\[([^\]]+)\]\(([^)]+)\)', part)
            r = p.add_run('%s (%s)' % (m.group(1), m.group(2)))
        else:
            r = p.add_run(part)
        if size:
            r.font.size = Pt(size)


def shade(cell, fill):
    tcPr = cell._tc.get_or_add_tcPr()
    sh = OxmlElement('w:shd')
    sh.set(qn('w:val'), 'clear')
    sh.set(qn('w:color'), 'auto')
    sh.set(qn('w:fill'), fill)
    tcPr.append(sh)


def table(rows):
    ncol = len(rows[0])
    t = doc.add_table(rows=1, cols=ncol)
    t.style = doc.styles['Table Grid']
    t.alignment = WD_TABLE_ALIGNMENT.CENTER
    t.autofit = False
    tw = OxmlElement('w:tblW')
    tw.set(qn('w:w'), '10490')
    tw.set(qn('w:type'), 'dxa')
    t._tbl.tblPr.append(tw)
    lay = OxmlElement('w:tblLayout')
    lay.set(qn('w:type'), 'fixed')
    t._tbl.tblPr.append(lay)
    lens = [max(6, max(len(r[i]) for r in rows)) for i in range(ncol)]
    lens = [min(l, 60) for l in lens]
    tot = sum(lens)
    widths = [int(10490 * l / tot) for l in lens]
    widths[-1] += 10490 - sum(widths)
    hdr = t.rows[0]
    trPr = hdr._tr.get_or_add_trPr()
    th = OxmlElement('w:tblHeader')
    th.set(qn('w:val'), 'true')
    trPr.append(th)
    for i, h in enumerate(rows[0]):
        c = hdr.cells[i]
        shade(c, 'D9E2F3')
        c.paragraphs[0].paragraph_format.space_after = Pt(2)
        rr = c.paragraphs[0].add_run(h.replace('**', ''))
        rr.bold = True
        rr.font.size = Pt(9.5)
    for row in rows[1:]:
        cells = t.add_row().cells
        for i in range(ncol):
            cells[i].paragraphs[0].paragraph_format.space_after = Pt(2)
            rich(cells[i].paragraphs[0], (row[i] if i < len(row) else '').replace('<br>', '\n'), 9.5)
    for row in t.rows:
        for c, w in zip(row.cells, widths):
            tcPr = c._tc.get_or_add_tcPr()
            tcw = tcPr.find(qn('w:tcW'))
            if tcw is None:
                tcw = OxmlElement('w:tcW')
                tcPr.insert(0, tcw)
            tcw.set(qn('w:w'), str(w))
            tcw.set(qn('w:type'), 'dxa')
    for g, w in zip(t._tbl.tblGrid.findall(qn('w:gridCol')), widths):
        g.set(qn('w:w'), str(w))
    doc.add_paragraph().paragraph_format.space_after = Pt(2)


# cover
p = doc.add_paragraph()
p.alignment = WD_ALIGN_PARAGRAPH.CENTER
p.paragraph_format.space_before = Pt(160)
r = p.add_run(TITLE)
r.bold = True
r.font.size = Pt(28)
r.font.color.rgb = RGBColor.from_string('1F3864')
p = doc.add_paragraph()
p.alignment = WD_ALIGN_PARAGRAPH.CENTER
p.add_run('Valid for version 0.1.0').font.size = Pt(13)
p = doc.add_paragraph()
p.paragraph_format.page_break_before = True
r = p.add_run('Contents')
r.bold = True
r.font.size = Pt(17)
r.font.color.rgb = RGBColor.from_string('1F3864')
p = doc.add_paragraph()
field(p, 'TOC \\o "1-2" \\h \\z \\u', 'Contents', 10.5)

lines = open(SRC, encoding='utf-8').read().split('\n')
i = 0
first_h1 = True
para_buf = []


def flush_para():
    global para_buf
    if para_buf:
        p = doc.add_paragraph()
        rich(p, ' '.join(para_buf))
        para_buf = []


while i < len(lines):
    l = lines[i]
    if l.startswith('```'):
        flush_para()
        lang = l[3:].strip()
        j = i + 1
        block = []
        while j < len(lines) and not lines[j].startswith('```'):
            block.append(lines[j])
            j += 1
        i = j + 1
        if lang == 'mermaid':
            p = doc.add_paragraph()
            rich(p, '*The diagram of how the crates link is in the Markdown version of this document and in the README (GitHub draws it). In short: the editor uses the core library and the formats crate; the compiler uses the core library; the core library uses the container reader.*'.replace('*', ''))
            continue
        for b in block:
            p = doc.add_paragraph()
            pf = p.paragraph_format
            pf.space_after = Pt(0)
            pf.left_indent = Cm(0.4)
            r = p.add_run(b)
            r.font.name = 'Consolas'
            rfonts(r._r.get_or_add_rPr(), 'Consolas')
            r.font.size = Pt(8.5)
        doc.add_paragraph().paragraph_format.space_after = Pt(2)
        continue
    if l.startswith('|') and i + 1 < len(lines) and lines[i + 1].startswith('|---'):
        flush_para()
        rows = []
        j = i
        while j < len(lines) and lines[j].startswith('|'):
            if not lines[j].startswith('|---'):
                cells = re.split(r'(?<!\\)\|', lines[j].strip().strip('|'))
                rows.append([c.strip().replace('\\|', '|') for c in cells])
            j += 1
        table(rows)
        i = j
        continue
    m = re.match(r'^(#{1,3})\s+(.*)$', l)
    if m:
        flush_para()
        lv = len(m.group(1))
        if lv == 1 and l.strip() == "# Zartex'27: code structure":
            i += 1
            continue
        h = doc.add_heading(m.group(2).replace('`', ''), level=lv)
        if lv == 1:
            h.paragraph_format.page_break_before = True
        i += 1
        continue
    if re.match(r'^\s*[-*]\s+', l):
        flush_para()
        p = doc.add_paragraph(style='List Bullet')
        indent = len(l) - len(l.lstrip())
        if indent:
            p.paragraph_format.left_indent = Cm(0.63 + 0.6 * (indent // 2))
        rich(p, re.sub(r'^\s*[-*]\s+', '', l))
        i += 1
        continue
    mm = re.match(r'^(\d+)\.\s+(.*)$', l)
    if mm:
        flush_para()
        p = doc.add_paragraph()
        p.paragraph_format.left_indent = Cm(0.6)
        p.paragraph_format.first_line_indent = Cm(-0.6)
        rich(p, '%s. %s' % (mm.group(1), mm.group(2)))
        i += 1
        continue
    if l.strip() == '---':
        flush_para()
        i += 1
        continue
    if not l.strip():
        flush_para()
        i += 1
        continue
    if l.startswith('**') and (l.startswith('**Purpose.') or l.startswith('**Origin.') or l.startswith('**Main') or l.startswith('**Calls.') or l.startswith('**Called') or l.startswith('**Data')):
        flush_para()
        p = doc.add_paragraph()
        p.paragraph_format.space_after = Pt(3)
        rich(p, l)
        i += 1
        continue
    para_buf.append(l.strip())
    i += 1
flush_para()
doc.core_properties.title = TITLE
doc.save(OUT)
print('saved', OUT)
