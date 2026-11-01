"""Package all four projects into pages without colliding shared style IDs."""
import json
import subprocess
import os
import shutil
from pathlib import Path

BASE = Path(__file__).resolve().parent
cli_path = os.environ.get('FIGCRAFT_CLI') or shutil.which('figcraft-cli')
if not cli_path and os.name == 'nt':
    candidate = Path(os.environ.get('ProgramFiles', 'C:/Program Files')) / 'ArtCraft/FigCraft/figcraft-cli.exe'
    if candidate.is_file():
        cli_path = str(candidate)
if not cli_path:
    raise SystemExit('Set FIGCRAFT_CLI to the FigCraft CLI executable or add figcraft-cli to PATH.')
CLI = Path(cli_path)
commands = [{'command': 'file.rename', 'params': {'name': 'Harbor — čtyři varianty'}}]


def remap(value, mapping):
    if isinstance(value, str):
        return mapping.get(value, value)
    if isinstance(value, list):
        return [remap(v, mapping) for v in value]
    if isinstance(value, dict):
        return {mapping.get(k, k): remap(v, mapping) for k, v in value.items()}
    return value


for i, (theme, label) in enumerate([('anthropic', 'Terra'), ('apple', 'Clarity'),
                                    ('nocturne', 'Nocturne'), ('original', 'Original')]):
    folder = BASE / theme
    folder.mkdir(exist_ok=True)
    original = BASE.parent / 'harbor-mariadb.figcraft' if theme == 'original' else folder / f'harbor-{theme}.figcraft'
    inspected = folder / 'document.inspect.json'
    subprocess.run([str(CLI), 'convert', str(original), str(inspected)], check=True)
    d = json.loads(inspected.read_text(encoding='utf-8-sig'))
    if theme == 'original':
        d['name'] = 'Harbor — Original'
        for node in d['nodes'].values():
            if node.get('type') == 'page':
                node['name'] = 'Original · Obrazovky'
                for flow in node.get('flows', []):
                    flow['name'] = 'Harbor / Original'
    # The installed FigCraft importer merges equal style IDs, so give each
    # source its own range before importing. Native layer IDs are remapped by it.
    mapping = {style['id']: f'S:{i*1000+j+1}' for j, style in enumerate(d['styles'])}
    d = remap(d, mapping)
    for style in d['styles']:
        style['name'] = label + '/' + style['name']
    source = folder / 'combined-source.json'
    source.write_text(json.dumps(d, ensure_ascii=False), encoding='utf-8')
    commands.append({'command': 'file.import', 'params': {'path': str(source)}})

commands.extend([{'command': 'page.delete', 'params': {'id': 1}},
                 {'command': 'page.select', 'params': {'index': 3}}])
script = BASE / 'compare.commands.json'
script.write_text(json.dumps(commands, ensure_ascii=False, indent=2), encoding='utf-8')
subprocess.run([str(CLI), 'run', str(script), '--out', str(BASE/'harbor-all-variants.figcraft')], check=True)
