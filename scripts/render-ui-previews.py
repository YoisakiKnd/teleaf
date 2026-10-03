#!/usr/bin/env python3
"""Render synthetic Ratatui cell snapshots as SVG and a local HTML gallery.

NO_COLOR= TERM=xterm-256color COLORTERM=truecolor TG_UI_PREVIEW=1 cargo test all_pages_render_on_wide_and_small_terminals
python3 scripts/render-ui-previews.py target/ui-previews
No dependencies, real account access or production rendering changes.
"""
import argparse
import html
import json
import re
from pathlib import Path


def color(value, foreground):
    if value.startswith('Rgb('):
        return '#%02x%02x%02x' % tuple(map(int, re.findall(r'\d+', value)))
    if value.startswith('Indexed('):
        n = int(re.search(r'\d+', value)[0])
        if n >= 232:
            return '#%02x%02x%02x' % ((8 + (n - 232) * 10,) * 3)
        if n >= 16:
            n -= 16
            levels = (0, 95, 135, 175, 215, 255)
            return '#%02x%02x%02x' % (levels[n // 36], levels[n // 6 % 6], levels[n % 6])
        value = ('Black', 'Red', 'Green', 'Yellow', 'Blue', 'Magenta', 'Cyan', 'Gray',
                 'DarkGray', 'LightRed', 'LightGreen', 'LightYellow', 'LightBlue',
                 'LightMagenta', 'LightCyan', 'White')[n]
    return {'Black': '#161b22', 'Red': '#c75b65', 'Green': '#7ebf8e',
            'Yellow': '#cfa85d', 'Blue': '#7da4d1', 'Magenta': '#b592cf',
            'Cyan': '#7dbabf', 'Gray': '#c6cdd5', 'DarkGray': '#586574',
            'LightRed': '#ff9297', 'LightGreen': '#a2e3ad', 'LightYellow': '#eed397',
            'LightBlue': '#a9cef5', 'LightMagenta': '#dcb6f2', 'LightCyan': '#aae6ea',
            'White': '#f0f3f6'}.get(value, '#e1e9ef' if foreground else '#171e24')


def render(data):
    cw, ch = 10, 22
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{data["width"]*cw}" height="{data["height"]*ch}" viewBox="0 0 {data["width"]*cw} {data["height"]*ch}">',
           '<style>rect{shape-rendering:crispEdges}text{font-family:Menlo,"DejaVu Sans Mono","PingFang SC",monospace;font-size:16px}</style>']
    text = []
    skip_to = {}
    for cell in data['cells']:
        if cell['x'] < skip_to.get(cell['y'], 0):
            continue
        x, y = cell['x'] * cw, cell['y'] * ch
        fg, bg = color(cell['fg'], True), color(cell['bg'], False)
        mods = cell['modifiers']
        if mods & 64:  # REVERSED
            fg, bg = bg, fg
        span = cell.get('width', 1)
        skip_to[cell['y']] = cell['x'] + span
        out.append(f'<rect x="{x}" y="{y}" width="{cw*span}" height="{ch}" fill="{bg}"/>')
        if not cell['text'].strip():
            continue
        attrs = ' font-weight="bold"' if mods & 1 else ''
        if mods & 2:
            attrs += ' opacity="0.6"'
        text.append(f'<text x="{x}" y="{y+17}" fill="{fg}"{attrs}>{html.escape(cell["text"])}</text>')
    return '\n'.join(out + text + ['</svg>'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path, nargs='?', default=Path('target/ui-previews'))
    args = parser.parse_args()
    files = sorted(args.directory.glob('*.json'))
    if not files:
        parser.error('No cell snapshots. Run the command in the script docstring first.')
    pages, sizes, entries = set(), set(), []
    for path in files:
        page, size = path.stem.rsplit('-', 1)
        pages.add(page)
        sizes.add(size)
        path.with_suffix('.svg').write_text(render(json.loads(path.read_text(encoding='utf-8'))), encoding='utf-8')
        entries.append(f'<section data-page="{page}" data-size="{size}"><h2>{page} · {size}</h2><img src="{path.with_suffix(".svg").name}" alt="{page} {size}"/></section>')
    options = lambda values: ''.join(f'<option>{v}</option>' for v in sorted(values))
    document = '''<!doctype html><html lang="zh"><meta charset="utf-8"><title>Teleaf UI preview</title>
<style>body{background:#10171c;color:#e1e9ef;font:15px system-ui;margin:24px}header{position:sticky;top:0;background:#10171c;padding:14px 0}h1{margin:0 0 8px;font-size:24px}p{color:#96a9b5}select{padding:6px;margin-right:12px;background:#26343d;color:#e1e9ef;border:1px solid #435664;border-radius:6px}section{overflow:auto;margin:20px 0}h2{font-size:16px;font-weight:500}img{border:1px solid #435664;border-radius:8px;max-width:100%;height:auto}</style>
<header><h1>Teleaf · 界面预览</h1><p>离线测试夹具的实际字符与配色。字体、原生图片及终端透明度须在真实终端中验证。</p>
<label>页面 <select id="page">''' + options(pages) + '''</select></label><label>窗口 <select id="size">''' + options(sizes) + '''</select></label></header>''' + '\n'.join(entries) + '''
<script>const p=document.querySelector('#page'),s=document.querySelector('#size');p.value='chats';s.value='110x32';function update(){document.querySelectorAll('section').forEach(e=>e.hidden=e.dataset.page!==p.value||e.dataset.size!==s.value)}p.onchange=s.onchange=update;update();</script></html>'''
    index = args.directory / 'index.html'
    index.write_text(document, encoding='utf-8')
    print(f'{len(files)} styled previews: {index.resolve()}')


if __name__ == '__main__':
    main()
