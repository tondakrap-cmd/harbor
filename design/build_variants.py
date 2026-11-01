"""Three original, editable FigCraft art directions for the Harbor prototype.

Uses only native command API nodes, shared paint/text styles, and prototype links.
The existing first concept is intentionally independent of these documents.
"""
from __future__ import annotations

import json
import math
from pathlib import Path

OUT = Path(__file__).resolve().parent / 'variants'
THEMES = {
    'anthropic': dict(name='Anthropic', label='Terra', bg='#FAF9F5', surface='#FFFFFF',
        side='#F0EEE6', inset='#F4F2EB', ink='#25241F', muted='#706C62',
        border='#DFDDD3', accent='#B65C3D', bright='#D97757', tint='#F2E1D6',
        good='#4F6945', good_bg='#E8EDE0', warn='#825C2F', warn_bg='#F3E9D5',
        title_font='Merriweather', title_size=40, radius=6, mono='Liberation Mono'),
    'apple': dict(name='Apple', label='Clarity', bg='#F4F5F8', surface='#FFFFFF',
        side='#EBEDF3', inset='#F3F5F9', ink='#1D2330', muted='#687180',
        border='#E1E5ED', accent='#0766D8', bright='#0A84FF', tint='#DFEBFF',
        good='#23805A', good_bg='#E7F5EC', warn='#946018', warn_bg='#FFF3DB',
        title_font='Inter', title_size=32, radius=18, mono='Liberation Mono'),
    'nocturne': dict(name='Nocturne', label='Nocturne', bg='#0C1017', surface='#141B25',
        side='#10151E', inset='#1B2432', ink='#EDF4FA', muted='#A0ACBE',
        border='#293547', accent='#9BE7C4', bright='#9BE7C4', tint='#243F39',
        good='#9BE7C4', good_bg='#243F39', warn='#E8BD7E', warn_bg='#3C3024',
        title_font='Inter', title_size=34, radius=14, mono='Liberation Mono'),
}
NAV = [('overview', 'Přehled serveru', 'dashboard'), ('databases', 'Databáze', 'database'),
       ('users', 'Uživatelé a oprávnění', 'users'), ('activity', 'Aktivní spojení', 'pulse')]
SCREENS = [('overview', '01 Přehled serveru'), ('databases', '02 Databáze'),
           ('users', '03 Uživatelé a oprávnění'), ('activity', '04 Aktivní spojení'),
           ('connect', '05 Nové připojení')]


class Design:
    def __init__(self, key):
        self.key = key
        self.t = THEMES[key]
        self.commands = []
        self.count = 0
        self.frames = {}
        self.links = []
        self.paint_styles = {}
        self.text_styles = {}
        self.cmd('file.rename', name=f'Harbor — {self.t["name"]}')
        self.cmd('page.rename', name=f'{self.t["label"]} · Obrazovky')
        self.cmd('page.setBackground', color='#D9D8D2' if key == 'anthropic' else '#CBD1DB' if key == 'apple' else '#06090E')
        # Frame refs are declared first so every interaction has a stable destination.
        for i, (screen, name) in enumerate(SCREENS):
            self.frames[screen] = self.box(None, name, i % 2 * 1540, i // 2 * 1060,
                1440, 960, self.t['bg'], radius=12, clip=True)

    @staticmethod
    def ref(key):
        return {'$ref': key}

    def cmd(self, command, **params):
        self.commands.append(dict(command=command, params=params))

    def node(self, kind, name, x, y, w, h, p=None, **props):
        self.count += 1
        key = f'n{self.count}'
        params = dict(type=kind, name=name, x=x, y=y, width=w, height=h,
                      select=False, props=props)
        if p:
            params['parent'] = self.ref(p)
        self.commands.append(dict(command='node.create', params=params, **{'as': key}))
        fill = props.get('fill')
        if isinstance(fill, str) and fill.startswith('#'):
            if fill not in self.paint_styles:
                role = next((k for k, v in self.t.items() if v == fill), fill)
                style = f'Palette / {role}'
                style_ref = f'paint{len(self.paint_styles)}'
                self.commands.append(dict(command='style.create', params={
                    'type': 'paint', 'name': style, 'from': self.ref(key)}, **{'as': style_ref}))
                self.paint_styles[fill] = style_ref
            self.cmd('style.apply', ids=[self.ref(key)], style=self.ref(self.paint_styles[fill]))
        return key

    def box(self, p, name, x, y, w, h, fill=None, radius=None, border=False, shadow=False, **props):
        props.update(fill=fill or self.t['surface'], cornerRadius=self.t['radius'] if radius is None else radius)
        if border:
            props.update(stroke=self.t['border'], strokeWeight=1)
        if shadow:
            props['effects'] = [dict(type='dropShadow', color='#192C4D0B', offset=[0, 4], radius=16)]
        return self.node('frame', name, x, y, w, h, p, **props)

    def txt(self, p, value, x, y, w=300, size=14, color=None, weight=False,
            family='Inter', align='left', h=26, tracking=0):
        key = self.node('text', value[:64], x, y, w, h, p, text=value,
            fill=color or self.t['ink'], fontFamily=family,
            fontStyle='Semi Bold' if weight else 'Regular', fontSize=size,
            textAlign=align, textAutoResize='height', lineHeight='140%', letterSpacing=tracking)
        signature = (family, size, weight, color or self.t['ink'], align, tracking)
        if signature not in self.text_styles:
            style = f'Type / {len(self.text_styles)+1:02d} / {family} {size}'
            style_ref = f'type{len(self.text_styles)}'
            self.commands.append(dict(command='style.create', params={
                'type': 'text', 'name': style, 'from': self.ref(key)}, **{'as': style_ref}))
            self.text_styles[signature] = style_ref
        self.cmd('style.apply', ids=[self.ref(key)], style=self.ref(self.text_styles[signature]))
        return key

    def rule(self, p, x, y, w, color=None):
        return self.box(p, 'Oddělovač', x, y, w, 1, color or self.t['border'], 0)

    def icon(self, p, kind, x, y, color=None, size=20):
        color = color or self.t['muted']
        paths = {
            'dashboard': 'M1 1 H8 V8 H1 Z M12 1 H19 V8 H12 Z M1 12 H8 V19 H1 Z M12 12 H19 V19 H12 Z',
            'database': 'M2 4 C2 0 18 0 18 4 C18 8 2 8 2 4 Z M2 4 V16 C2 20 18 20 18 16 V4 M2 10 C2 14 18 14 18 10',
            'users': 'M7 9 A4 4 0 1 0 7 1 A4 4 0 1 0 7 9 M1 19 V16 C1 10 13 10 13 16 V19 M14 2 C20 2 20 9 14 9 M16 12 C20 12 20 16 20 19',
            'pulse': 'M0 10 H5 L8 2 L12 18 L15 10 H20',
            'plus': 'M10 1 V19 M1 10 H19',
            'search': 'M14 14 L20 20 M15 8 A7 7 0 1 0 1 8 A7 7 0 1 0 15 8',
            'chevron': 'M2 1 L9 8 L2 15',
            'arrow': 'M1 10 H19 M12 3 L19 10 L12 17',
            'check': 'M1 10 L7 16 L19 2',
            'lock': 'M5 9 V5 C5 -1 15 -1 15 5 V9 M2 9 H18 V20 H2 Z M10 13 V16',
            'refresh': 'M17 7 A8 8 0 1 0 18 14 M12 7 H18 V1',
            'settings': 'M0 5 H20 M0 15 H20 M5 1 V9 M15 11 V19',
        }
        return self.node('vector', f'Ikona / {kind}', x, y, size, size, p,
            path=paths[kind], fill='none', stroke=color, strokeWeight=1.5,
            strokeCap='round', strokeJoin='round')

    def dot(self, p, x, y, color=None, size=7):
        return self.node('ellipse', 'Stavový indikátor', x, y, size, size, p, fill=color or self.t['good'])

    def badge(self, p, label, x, y, w=110, role='good'):
        color, bg = (self.t['good'], self.t['good_bg']) if role == 'good' else (
            (self.t['warn'], self.t['warn_bg']) if role == 'warn' else (self.t['muted'], self.t['inset']))
        b = self.box(p, label, x, y, w, 28, bg, 14 if self.key != 'anthropic' else 4)
        self.txt(b, label, 10, 6, w-20, 11, color, True)
        return b

    def button(self, p, label, x, y, w=170, primary=False, dest=None, kind=None, danger=False):
        bg = self.t['accent'] if primary else self.t['surface']
        color = ('#10251D' if self.key == 'nocturne' else '#FFFFFF') if primary else self.t['ink']
        if danger:
            color = '#E0A09A' if self.key == 'nocturne' else '#B33C37'
        b = self.box(p, label, x, y, w, 40, bg, 20 if self.key == 'apple' else 6 if self.key == 'anthropic' else 8,
                     border=not primary, shadow=primary and self.key == 'apple')
        pad = 38 if kind else 14
        if kind:
            self.icon(b, kind, 13, 12, color, 15)
        self.txt(b, label, pad, 11, w-pad-12, 12, color, True)
        if dest:
            self.links.append((b, dest))
        return b

    def shell(self, screen, title, subtitle):
        t, key = self.t, self.key
        root = self.frames[screen]
        if key == 'apple':
            # Quiet translucent-looking blue-grey surfaces, kept as editable vectors.
            self.box(root, 'Ambient / blue', 980, -100, 600, 380, '#E8EEF9', 180)
            self.box(root, 'Ambient / lavender', 1170, 160, 430, 440, '#F0EDF9', 180)
        bar = self.box(root, 'Linux / Window title bar', 0, 0, 1440, 38, t['side'], 0)
        self.txt(bar, f'Harbor  /  {t["label"]}', 22, 10, 600, 11, t['muted'])
        self.txt(bar, '—       □       ×', 1303, 7, 120, 14, t['muted'])
        self.rule(root, 0, 38, 1440)
        sx, sy, sw, sh = (12, 50, 228, 898) if key == 'apple' else (0, 39, 240, 921)
        side = self.box(root, 'Sidebar / Navigation', sx, sy, sw, sh, t['side'], 20 if key == 'apple' else 0,
                        border=key == 'apple')
        if key == 'anthropic':
            # A small radial mark built from editable vector lines.
            for n in range(12):
                angle = n*math.pi/6
                xx, yy = 30+13*math.cos(angle), 43+13*math.sin(angle)
                self.node('vector', 'Harbor / paprsek', min(30, xx), min(43, yy), max(abs(xx-30), .1), max(abs(yy-43), .1),
                          side, path=f'M30 43 L{xx} {yy}', fill='none', stroke=t['bright'], strokeWeight=2)
            self.txt(side, 'harbor', 52, 16, 164, 31, t['ink'], family=t['title_font'])
        else:
            logo = self.box(side, 'Harbor / mark', 21, 23, 32, 32,
                            t['accent'] if key == 'nocturne' else t['bright'], 9)
            self.txt(logo, 'h', 9, 1, 23, 23, '#10251D' if key == 'nocturne' else '#FFFFFF', True)
            self.txt(side, 'harbor', 63, 23, 150, 24, t['ink'], True)
        self.txt(side, 'MARIADB MANAGER', 23, 79, 200, 10, t['muted'], tracking=1.2)
        server = self.box(side, 'Server / db-prod-01', 16, 126, sw-32, 83,
                          t['surface'] if key != 'nocturne' else t['inset'], 12 if key != 'anthropic' else 5,
                          border=key != 'apple')
        self.dot(server, 14, 18)
        self.txt(server, 'Produkční server', 29, 12, 160, 12, weight=True)
        self.txt(server, 'db-prod-01', 14, 39, 160, 11, t['muted'], family=t['mono'])
        self.txt(server, 'AlmaLinux 9', 14, 60, 160, 10, t['muted'])
        self.txt(side, 'PRACOVNÍ PROSTOR', 23, 248, 200, 9, t['muted'], True, tracking=1)
        for i, (dest, label, icon) in enumerate(NAV):
            selected = dest == screen
            fill = (t['tint'] if key != 'apple' else t['accent']) if selected else t['side']
            item = self.box(side, f'Nav / {label}', 12, 278+i*49, sw-24, 42, fill,
                            10 if key == 'apple' else 6)
            color = '#FFFFFF' if selected and key == 'apple' else t['accent'] if selected and key == 'nocturne' else t['ink'] if selected else t['muted']
            self.icon(item, icon, 14, 12, color, 17)
            self.txt(item, label, 43, 12, sw-78, 11.5, color, selected)
            if selected and key == 'nocturne':
                self.box(item, 'Active marker', 0, 13, 3, 16, t['accent'], 2)
            self.links.append((item, dest))
        self.rule(side, 23, 506, sw-46)
        self.txt(side, 'PŘIPOJENÍ', 23, 531, 170, 9, t['muted'], True, tracking=1)
        self.dot(side, 25, 568, t['good'], 6)
        self.txt(side, 'db-prod-01', 40, 559, 150, 12)
        self.dot(side, 25, 600, t['muted'], 6)
        self.txt(side, 'ubuntu-staging', 40, 591, 160, 12, t['muted'])
        self.button(side, 'Přidat připojení', 16, sh-154, sw-32, dest='connect', kind='plus')
        self.txt(side, 'AlmaLinux  /  Ubuntu', 23, sh-79, 190, 11, t['muted'])
        self.txt(side, 'NÁVRH DESKTOPOVÉ APLIKACE', 23, sh-51, 199, 8.5, t['muted'], tracking=.6)
        # Toolbar and contextual status.
        top = self.box(root, 'Toolbar / Server context', 264, 54, 1152, 56,
                       t['bg'] if key != 'apple' else '#FFFFFFB8', 15 if key == 'apple' else 0,
                       border=key == 'apple')
        self.txt(top, 'Servery', 16, 18, 80, 12, t['muted'])
        self.icon(top, 'chevron', 90, 22, t['muted'], 8)
        self.txt(top, 'db-prod-01', 115, 18, 210, 12, weight=True)
        self.dot(top, 848, 24)
        self.txt(top, 'Připojeno', 864, 18, 95, 12, t['good'])
        self.icon(top, 'lock', 978, 20, t['muted'], 13)
        self.txt(top, 'TLS ověřeno', 1002, 18, 130, 12, t['muted'])
        if key == 'anthropic':
            self.rule(root, 280, 115, 1120)
        self.txt(root, 'PRODUKČNÍ PROSTŘEDÍ' if key != 'nocturne' else 'WORKSPACE / PRODUCTION',
                 280, 142, 750, 10, t['muted'], True, tracking=1.3)
        self.txt(root, title, 280, 168, 1020, t['title_size'], weight=key != 'anthropic', family=t['title_font'])
        self.txt(root, subtitle, 280, 226, 1110, 13, t['muted'])
        self.rule(root, 280, 904, 1120)
        self.txt(root, 'UKÁZKOVÁ DATA  ·  Bez připojení k serveru', 280, 924, 680, 10, t['muted'])
        self.txt(root, f'HARBOR  /  {t["label"].upper()}', 1110, 924, 290, 10, t['muted'], align='right', tracking=1)
        return root

    def chart(self, p, x, y, w, h, compact=False):
        t = self.t
        c = self.box(p, 'Chart / Queries per second', x, y, w, h,
                     t['bg'] if self.key == 'anthropic' else t['surface'],
                     0 if self.key == 'anthropic' else t['radius'], border=self.key == 'nocturne', shadow=self.key == 'apple')
        self.txt(c, 'Provoz serveru', 24, 20, 400, 18, weight=True)
        self.txt(c, 'Dotazy za sekundu', 24, 50, 300, 12, t['muted'])
        self.badge(c, 'Poslední hodina', w-174, 22, 150, 'neutral')
        gx, gy, gw, gh = 62, 104, w-88, h-151
        for i in range(3):
            yy = gy+i*gh/2
            self.rule(c, gx, yy, gw, t['border'])
            self.txt(c, ['400', '200', '0'][i], 20, yy-8, 35, 10, t['muted'])
        vals = [45, 49, 43, 57, 54, 60, 52, 73, 66, 77, 70, 84, 72, 61, 66,
                97, 80, 88, 79, 74, 89, 78, 84, 73, 78, 74, 70, 64, 66, 62]
        pts = [(i*gw/29, gh*(100-v)/100) for i, v in enumerate(vals)]
        # Explicit zero-origin boxes also render consistently in older FigCraft releases.
        path = 'M '+' L '.join(f'{px:.2f} {py:.2f}' for px, py in pts)
        if self.key != 'anthropic':
            area = path+f' L{gw} {gh} L0 {gh} Z'
            self.node('vector', 'Chart / Area', gx, gy, gw, gh, c, path=area,
                      fill=t['accent']+'14', stroke='none')
        miny, maxy = min(v[1] for v in pts), max(v[1] for v in pts)
        self.node('vector', 'Chart / QPS line', gx, gy+miny, gw, maxy-miny, c,
                  path=path, fill='none', stroke=t['accent'], strokeWeight=2.5, strokeJoin='round')
        for i, label in enumerate(['14:00', '14:15', '14:30', '14:45', '15:00']):
            self.txt(c, label, gx+i*(gw-40)/4, h-33, 55, 10, t['muted'])
        return c

    def metric(self, p, title, value, detail, x, y, w, h=132, spark=False):
        t = self.t
        c = self.box(p, f'Metric / {title}', x, y, w, h,
                     t['bg'] if self.key == 'anthropic' else t['surface'],
                     0 if self.key == 'anthropic' else t['radius'],
                     border=self.key == 'nocturne', shadow=self.key == 'apple')
        pad = 0 if self.key == 'anthropic' else 20
        self.txt(c, title, pad, 16, w-2*pad, 11, t['muted'], tracking=.4)
        self.txt(c, value, pad, 40, w-2*pad, 30, weight=self.key != 'anthropic',
                 family=t['title_font'] if self.key == 'anthropic' else 'Inter')
        self.txt(c, detail, pad, h-27, w-2*pad, 11, t['good'])
        if spark:
            pts = [[0, 18], [9, 15], [18, 19], [27, 9], [36, 13], [45, 4], [54, 7], [63, 0], [74, 4]]
            self.node('vector', 'Metric / Sparkline', w-100, 51, 74, 19, c,
                path='M '+' L '.join(f'{a} {b}' for a, b in pts), fill='none', stroke=t['accent'], strokeWeight=2)
        if self.key == 'anthropic':
            self.rule(c, 0, h-1, w-18)
        return c

    def instance(self, p, x, y, w, h):
        t = self.t
        c = self.box(p, 'Instance / Metadata', x, y, w, h,
                     t['inset'] if self.key == 'anthropic' else t['surface'],
                     border=self.key == 'nocturne', shadow=self.key == 'apple')
        self.txt(c, 'Informace o instanci', 22, 20, w-44, 17, weight=True)
        values = [('Operační systém', 'AlmaLinux 9'), ('MariaDB', '11.4.5'),
                  ('Účet', 'harbor_admin'), ('Šifrování', 'TLS 1.3'), ('Časové pásmo', 'Europe/Prague')]
        for i, (label, val) in enumerate(values):
            self.txt(c, label, 22, 70+i*36, 151, 12, t['muted'])
            self.txt(c, val, w-173, 70+i*36, 151, 12)
        return c

    def alert(self, p, y=748, height=128):
        t = self.t
        c = self.box(p, 'Attention / Long running queries', 280, y, 1120, height,
                     t['warn_bg'] if self.key == 'anthropic' else t['surface'],
                     border=self.key == 'nocturne', shadow=self.key == 'apple')
        self.badge(c, 'Ke kontrole', 22, 22, 104, 'warn')
        self.txt(c, '3 dotazy běží déle než 30 sekund', 146, 24, 700, 15, weight=True)
        self.txt(c, 'Zkontrolujte jejich stav v přehledu aktivních spojení.', 146, 58, 695, 12, t['muted'])
        self.button(c, 'Zobrazit spojení', 910, 43, 186, dest='activity', kind='arrow')

    def overview(self):
        t = self.t
        title = 'Přehled serveru' if self.key != 'nocturne' else 'Vše běží. Máte přehled.'
        p = self.shell('overview', title, 'db-prod-01.example.net:3306   /   MariaDB 11.4   /   AlmaLinux 9')
        if self.key != 'nocturne':
            metrics = [('DOSTUPNOST', '18 dní 04 h', 'Server odpovídá'),
                       ('DOTAZY / SEKUNDU', '248', '+12 % za poslední hodinu'),
                       ('AKTIVNÍ SPOJENÍ', '24 / 150', '16 % nastaveného limitu'),
                       ('VELIKOST DATABÁZÍ', '42,8 GB', '8 databází celkem')]
            for i, args in enumerate(metrics):
                self.metric(p, *args, 280+i*285, 280, 265)
            self.chart(p, 280, 436, 736, 286)
            self.instance(p, 1040, 436, 360, 286)
            self.alert(p)
        else:
            c = self.box(p, 'Hero / Throughput', 280, 280, 736, 182, '#172A2B', border=True)
            self.txt(c, 'AKTUÁLNÍ PROVOZ', 24, 22, 500, 10, t['accent'], True, tracking=1.5)
            self.txt(c, '248', 24, 52, 270, 62, weight=True)
            self.txt(c, 'dotazů / s', 180, 94, 230, 15, t['muted'])
            self.badge(c, '+12 % za hodinu', 24, 139, 151)
            for i, v in enumerate([28, 35, 30, 49, 39, 60, 43, 72, 65, 84, 71, 90, 78, 96, 83, 101, 92, 113, 100, 109]):
                self.box(c, 'Throughput / Bar', 415+i*14, 148-v, 6, v,
                         '#A6EACD' if i > 14 else '#416B63', 3)
            self.metric(p, 'AKTIVNÍ SPOJENÍ', '24 / 150', '16 % nastaveného limitu', 1040, 280, 360, 182)
            self.chart(p, 280, 486, 736, 240)
            self.instance(p, 1040, 486, 360, 390)
            self.metric(p, 'DOSTUPNOST', '18 dní 04 h', 'Server odpovídá', 280, 750, 285, 126)
            self.metric(p, 'DATABÁZE', '42,8 GB', '8 databází celkem', 581, 750, 225, 126)
            c = self.box(p, 'Attention / Long queries', 822, 750, 194, 126, t['warn_bg'], border=True)
            self.txt(c, 'KE KONTROLE', 16, 14, 162, 10, t['warn'], True, tracking=.8)
            self.txt(c, '3 dotazy', 16, 39, 164, 23, t['ink'], True)
            self.txt(c, 'Zobrazit spojení →', 16, 91, 164, 11, t['warn'])
            self.links.append((c, 'activity'))

    def search(self, p, placeholder, action, refresh=False):
        t = self.t
        f = self.box(p, 'Search / '+placeholder, 280, 280, 520, 42,
                     t['surface'], 21 if self.key == 'apple' else 6, border=self.key != 'apple')
        self.icon(f, 'search', 14, 13, t['muted'], 16)
        self.txt(f, placeholder, 43, 12, 450, 12, t['muted'])
        self.button(p, action, 1188, 280, 212, primary=not refresh,
                    kind='refresh' if refresh else 'plus')

    def table(self, p, title, columns, widths, rows, tags=None, y=348, row_h=61):
        t = self.t
        c = self.box(p, 'Table / '+title, 280, y, 1120, 106+row_h*len(rows),
                     t['surface'], border=self.key != 'apple', shadow=self.key == 'apple')
        self.txt(c, title, 22, 20, 810, 17, weight=True)
        self.txt(c, f'{len(rows):02d} ZÁZNAMŮ', 903, 24, 194, 9, t['muted'], align='right', tracking=.7)
        self.box(c, 'Column headers', 1, 62, 1118, 38, t['inset'], 0)
        for i, label in enumerate(columns):
            self.txt(c, label, widths[i], 74, (widths[i+1]-widths[i]-10) if i+1 < len(widths) else 1100-widths[i],
                     9.5, t['muted'], True, tracking=.4)
        for r, row in enumerate(rows):
            yy = 100+r*row_h
            if self.key == 'apple' and r % 2 == 1:
                self.box(c, 'Alternating row', 8, yy, 1104, row_h, '#F8F9FC', 5)
            if self.key != 'apple' and r:
                self.rule(c, 22, yy, 1076)
            for i, value in enumerate(row):
                ww = (widths[i+1]-widths[i]-14) if i+1 < len(widths) else 1100-widths[i]
                if tags and i == len(widths)-1:
                    self.badge(c, value, widths[i], yy+17, min(ww, 100), 'good' if value == 'Aktivní' else 'neutral')
                else:
                    self.txt(c, value, widths[i], yy+21, ww, 12,
                             t['accent'] if i == 0 else t['ink'], i == 0)
        return c

    def databases(self):
        t = self.t
        p = self.shell('databases', 'Databáze', 'Osm databází. Jeden přehled o velikostech, tabulkách a kódování.')
        self.search(p, 'Hledat databázi…', 'Vytvořit databázi')
        self.table(p, 'Databáze na serveru', ['NÁZEV', 'TABULKY', 'VELIKOST', 'ZNAKOVÁ SADA', 'KOLACE'],
            [22, 350, 480, 650, 850], [
                ['commerce', '128', '28,4 GB', 'utf8mb4', 'utf8mb4_unicode_ci'],
                ['analytics', '64', '11,2 GB', 'utf8mb4', 'utf8mb4_unicode_ci'],
                ['identity', '18', '2,1 GB', 'utf8mb4', 'utf8mb4_unicode_ci'],
                ['inventory', '32', '1,1 GB', 'utf8mb4', 'utf8mb4_unicode_ci'],
                ['mysql', '31', '8,4 MB', 'utf8mb4', 'utf8mb4_general_ci'],
                ['information_schema', '84', '—', 'utf8mb3', 'utf8mb3_general_ci']])
        self.txt(p, 'Zobrazeno 6 z 8 databází', 280, 846, 600, 12, t['muted'])
        self.badge(p, 'Včetně systémových databází', 1130, 836, 270, 'neutral')

    def users(self):
        t = self.t
        p = self.shell('users', 'Uživatelé a oprávnění', 'Přístupy ke každé databázi. Každý účet v kontextu uživatel@host.')
        self.search(p, 'Hledat uživatele nebo hostitele…', 'Přidat uživatele')
        self.table(p, 'Účty na serveru', ['ÚČET', 'HOSTITEL', 'PŘÍSTUP', 'AUTENTIZACE', 'STAV'],
            [22, 258, 440, 690, 970], [
                ['harbor_admin', '10.20.0.%', 'Správa instance', 'mysql_native_password', 'Aktivní'],
                ['commerce_app', '10.20.4.%', 'commerce · čtení / zápis', 'mysql_native_password', 'Aktivní'],
                ['analytics_ro', '10.20.6.%', 'analytics · pouze čtení', 'mysql_native_password', 'Aktivní'],
                ['backup_agent', 'localhost', 'Zálohování', 'mysql_native_password', 'Aktivní'],
                ['root', 'localhost', 'Správa instance', 'unix_socket', 'Lokální']], tags=True)
        c = self.box(p, 'Note / Privileges', 280, 786, 1120, 90,
                     t['inset'], border=self.key == 'nocturne')
        self.icon(c, 'lock', 22, 24, t['accent'], 20)
        self.txt(c, 'Oprávnění pod kontrolou', 62, 17, 1000, 14, weight=True)
        self.txt(c, 'Detail účtu zobrazí databázová i globální privilegia. Změny před uložením zkontrolujete.',
                 62, 49, 1020, 12, t['muted'])

    def activity(self):
        t = self.t
        p = self.shell('activity', 'Aktivní spojení', 'Živý přehled relací a běžících dotazů podle oprávnění vašeho účtu.')
        self.badge(p, '24 spojení celkem', 280, 286, 160)
        self.badge(p, '3 dlouhé dotazy', 454, 286, 151, 'warn')
        self.button(p, 'Obnovit přehled', 1200, 280, 200, kind='refresh')
        self.table(p, 'Procesy', ['ID / UŽIVATEL', 'DATABÁZE', 'ČAS', 'STAV', 'DOTAZ'],
            [22, 262, 420, 520, 770], [
                ['1842 · analytics_ro', 'analytics', '47 s', 'Sending data', 'SELECT date, SUM(total)…'],
                ['1856 · commerce_app', 'commerce', '35 s', 'Waiting for table lock', 'UPDATE orders SET…'],
                ['1860 · analytics_ro', 'analytics', '32 s', 'Sending data', 'SELECT product_id…'],
                ['1871 · commerce_app', 'commerce', '1 s', 'Sleep', '—']], row_h=55)
        c = self.box(p, 'SQL / Query detail', 280, 698, 1120, 178,
                     t['surface'], border=self.key != 'apple', shadow=self.key == 'apple')
        self.txt(c, 'Dotaz #1842', 22, 16, 800, 16, weight=True)
        self.badge(c, '47 sekund', 986, 14, 110, 'warn')
        code = self.box(c, 'SQL / Code preview', 22, 55, 1076, 54, t['inset'], 7)
        self.txt(code, 'SELECT date, SUM(total) FROM daily_sales GROUP BY date ORDER BY date DESC;',
                 14, 18, 1048, 11.5, t['accent'], family=t['mono'])
        self.txt(c, 'Ukončení dotazu bude vyžadovat potvrzení.', 22, 139, 700, 12, t['muted'])
        self.button(c, 'Ukončit dotaz…', 898, 124, 198, danger=True)

    def connect(self):
        t = self.t
        p = self.shell('connect', 'Nové připojení', 'Lokální nebo vzdálený server. Přístupové údaje zůstávají ve vaší správě.')
        c = self.box(p, 'Form / New connection', 280, 280, 710, 596,
                     t['surface'], border=self.key != 'apple', shadow=self.key == 'apple')
        self.txt(c, 'Údaje o připojení', 24, 21, 662, 19, weight=True)
        fields = [('Název připojení', 'Produkční server', 24, 74, 662),
                  ('Hostitel', 'db-prod-01.example.net', 24, 163, 468),
                  ('Port', '3306', 510, 163, 176),
                  ('Uživatel', 'harbor_admin', 24, 252, 320),
                  ('Heslo', '••••••••••••', 366, 252, 320),
                  ('Zabezpečení spojení', 'TLS · ověřit certifikát a název serveru', 24, 341, 662)]
        for label, value, x, y, w in fields:
            self.txt(c, label, x, y, w, 12, t['muted'], True)
            field = self.box(c, 'Field / '+label, x, y+27, w, 44,
                             t['inset'], 10 if self.key == 'apple' else 5, border=True)
            self.txt(field, value, 13, 12, w-26, 13)
        self.icon(c, 'lock', 25, 445, t['muted'], 14)
        self.txt(c, 'Heslo se bez vašeho souhlasu neuloží.', 51, 444, 628, 12, t['muted'])
        self.rule(c, 24, 494, 662)
        self.button(c, 'Otestovat spojení', 24, 527, 194)
        self.button(c, 'Připojit', 492, 527, 194, True, 'overview', kind='arrow')
        c = self.box(p, 'Connection / Guidance', 1014, 280, 386, 322,
                     t['inset'] if self.key != 'nocturne' else '#172A2B', border=self.key == 'nocturne')
        self.icon(c, 'database', 24, 25, t['accent'], 28)
        self.txt(c, 'Jeden prostor.\nObě distribuce.', 24, 79, 338,
                 28 if self.key == 'anthropic' else 25, weight=self.key != 'anthropic', family=t['title_font'])
        self.badge(c, 'AlmaLinux', 24, 174, 120, 'neutral')
        self.badge(c, 'Ubuntu', 154, 174, 100, 'neutral')
        self.txt(c, 'Návrh desktopové aplikace v Rustu.\nStejné prostředí pro lokální i vzdálené\ninstance MariaDB.', 24, 225, 338, 13, t['muted'])
        c = self.box(p, 'Prototype / Information', 1014, 626, 386, 250,
                     t['surface'], border=self.key != 'apple', shadow=self.key == 'apple')
        self.txt(c, 'Jen návrh, žádné spojení', 24, 25, 338, 17, weight=True)
        self.txt(c, 'Tlačítko Připojit otevře ukázkový\npřehled. Formulář neodesílá údaje\na neověřuje skutečný server.',
                 24, 72, 338, 13, t['muted'])
        self.rule(c, 24, 161, 338)
        self.txt(c, 'Správa systémové služby přes SSH\nbude samostatný pracovní postup.', 24, 181, 338, 12, t['muted'])

    def write(self):
        self.overview()
        self.databases()
        self.users()
        self.activity()
        self.connect()
        for source, target in self.links:
            self.cmd('proto.interaction.add', id=self.ref(source), dest=self.ref(self.frames[target]))
        self.cmd('proto.flow.add', start=self.ref(self.frames['overview']), name=f'Harbor / {self.t["label"]}')
        self.cmd('proto.settings', device='None', background=self.t['bg'])
        self.cmd('proto.presentOptions', scale='fit', hotspotHints=True)
        folder = OUT / self.key
        folder.mkdir(parents=True, exist_ok=True)
        path = folder / 'build.commands.json'
        path.write_text(json.dumps(self.commands, ensure_ascii=False, indent=2), encoding='utf-8')
        print(f'{self.key}: {self.count} layers, {len(self.links)} interactions → {path}')


if __name__ == '__main__':
    for theme in THEMES:
        Design(theme).write()
