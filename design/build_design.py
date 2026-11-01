"""Build an editable FigCraft prototype through its native command API."""
import json
from pathlib import Path

OUT = Path(__file__).resolve().parent
commands = []
counter = 0
INK = '#172B36'
MUTED = '#71828C'
TEAL = '#087F75'
BORDER = '#E2E9EB'
links = []


def node(kind, name, x, y, w, h, parent=None, **props):
    global counter
    counter += 1
    key = f'n{counter}'
    params = dict(type=kind, name=name, x=x, y=y, width=w, height=h,
                  props=props, select=False)
    if parent:
        params['parent'] = {'$ref': parent}
    commands.append(dict(command='node.create', params=params, **{'as': key}))
    return key


def rect(p, name, x, y, w, h, fill='#FFFFFF', radius=0, stroke=None):
    props = dict(fill=fill, cornerRadius=radius)
    if stroke:
        props.update(stroke=stroke, strokeWeight=1)
    return node('frame', name, x, y, w, h, p, **props)


def text(p, value, x, y, w=300, size=14, color=INK, bold=False, h=30):
    return node('text', value[:60], x, y, w, h, p, text=value,
                fill=color, fontFamily='Inter', fontStyle='Semi Bold' if bold else 'Regular',
                fontSize=size, lineHeight='140%', textAutoResize='height')


def button(p, label, x, y, w=160, primary=False, dest=None):
    b = rect(p, label, x, y, w, 42, TEAL if primary else '#FFFFFF', 8,
             None if primary else BORDER)
    text(b, label, 14, 11, w-24, 13, '#FFFFFF' if primary else INK, True)
    if dest:
        links.append((b, dest))
    return b


def pill(p, label, x, y, w=90, color=TEAL, bg='#E5F4EF'):
    b = rect(p, label, x, y, w, 26, bg, 13)
    text(b, label, 10, 4, w-16, 11, color, True)


def line(p, x, y, w):
    rect(p, 'Oddělovač', x, y, w, 1, BORDER)


nav = [('overview', 'Přehled serveru'), ('databases', 'Databáze'),
       ('users', 'Uživatelé a oprávnění'), ('activity', 'Aktivní spojení')]
frames = {}


def shell(key, name, index, title, subtitle):
    root = rect(None, name, (index % 2)*1540, (index // 2)*1060, 1440, 960, '#F5F8F9', 12)
    frames[key] = root
    bar = rect(root, 'Lišta okna Linux', 0, 0, 1440, 40, '#EAF0F2')
    text(bar, 'Harbor  /  MariaDB Manager', 24, 10, 600, 12, MUTED)
    text(bar, '—     □     ×', 1310, 8, 110, 16, MUTED)
    side = rect(root, 'Navigace', 0, 40, 240, 920, '#142D35')
    logo = rect(side, 'Značka', 24, 26, 34, 34, '#27B6A2', 10)
    text(logo, 'H', 9, 5, 25, 20, '#FFFFFF', True)
    text(side, 'harbor', 70, 23, 140, 26, '#FFFFFF', True)
    text(side, 'MARIADB WORKSPACE', 25, 78, 200, 10, '#85A6AD', True)
    server = rect(side, 'Aktivní server', 16, 120, 208, 76, '#203D45', 9)
    text(server, '●   Produkční server', 14, 13, 185, 13, '#DBF2EF', True)
    text(server, 'db-prod-01  ·  AlmaLinux', 14, 39, 185, 11, '#9BB7BD')
    text(side, 'PRACOVNÍ PROSTOR', 25, 228, 200, 10, '#85A6AD', True)
    for i, (target, label) in enumerate(nav):
        item = rect(side, label, 16, 258+i*52, 208, 44,
                    '#2C4D53' if key == target else '#142D35', 8)
        text(item, ['◈', '▤', '◎', '≋'][i], 14, 10, 22, 17, '#70D6C7' if key == target else '#85A6AD')
        text(item, label, 43, 12, 160, 12, '#FFFFFF' if key == target else '#B7CDD1', key == target)
        links.append((item, target))
    button(side, '+  Přidat připojení', 20, 770, 200, dest='connect')
    text(side, 'DESKTOP PRO LINUX', 25, 842, 200, 10, '#85A6AD', True)
    text(side, 'AlmaLinux  /  Ubuntu', 25, 863, 200, 12, '#B7CDD1')
    top = rect(root, 'Kontext serveru', 240, 40, 1200, 70)
    text(top, 'Servery   /   db-prod-01', 32, 25, 500, 13, MUTED)
    pill(top, '● Připojeno', 862, 22, 108)
    pill(top, 'TLS ověřeno', 982, 22, 128, '#455E70', '#EDF2F5')
    text(root, title, 280, 144, 900, 30, INK, True)
    text(root, subtitle, 280, 192, 1000, 13, MUTED)
    line(root, 280, 902, 1120)
    text(root, 'UKÁZKOVÁ DATA  ·  Interaktivní návrh, bez spojení se serverem', 280, 919, 800, 11, MUTED)
    text(root, 'Harbor / koncept 01', 1210, 919, 190, 11, MUTED)
    return root


def card(p, title, value, detail, x, accent=TEAL):
    c = rect(p, title, x, 250, 265, 136, '#FFFFFF', 12, BORDER)
    text(c, title, 20, 17, 225, 12, MUTED)
    text(c, value, 20, 43, 225, 30, INK, True)
    text(c, detail, 20, 99, 230, 11, accent)


p = shell('overview', '01 Přehled serveru', 0, 'Server pod kontrolou.',
          'Produkční prostředí  ·  db-prod-01.example.net:3306  ·  MariaDB 11.4')
for i, args in enumerate([
    ('DOSTUPNOST', '18 dní 04 h', '● Server odpovídá'),
    ('DOTAZY ZA SEKUNDU', '248', '+ 12 % za poslední hodinu'),
    ('AKTIVNÍ SPOJENÍ', '24 / 150', '16 % nastaveného limitu'),
    ('VELIKOST DATABÁZÍ', '42,8 GB', '8 databází celkem')]):
    card(p, *args, 280+i*285)
c = rect(p, 'Graf zatížení', 280, 410, 740, 270, '#FFFFFF', 12, BORDER)
text(c, 'Provoz serveru', 22, 20, 400, 17, INK, True)
text(c, 'Dotazy za sekundu', 22, 51, 300, 12, MUTED)
pill(c, 'Poslední hodina', 567, 21, 150, '#455E70', '#F0F4F6')
for yy, val in [(99, '400'), (151, '200'), (203, '0')]:
    text(c, val, 22, yy-8, 38, 10, MUTED)
    rect(c, 'Mřížka grafu', 65, yy, 648, 1, '#EDF2F4')
values = [158, 155, 166, 142, 145, 139, 149, 115, 130, 108, 120, 104, 123, 139, 131, 87, 108, 97, 109, 119, 103, 124, 96, 116, 107, 131, 118, 125, 114, 119]
path = 'M ' + ' L '.join(f'{65+i*22},{v}' for i, v in enumerate(values))
node('vector', 'Křivka ukázkového provozu', 65, 99, 638, 104, c, path=path,
     fill='none', stroke=TEAL, strokeWeight=3)
for i, val in enumerate(['14:00', '14:15', '14:30', '14:45', '15:00']):
    text(c, val, 65+i*152, 225, 70, 10, MUTED)
c = rect(p, 'Informace o instanci', 1040, 410, 360, 270, '#FFFFFF', 12, BORDER)
text(c, 'Informace o instanci', 22, 20, 310, 17, INK, True)
for i, (label, val) in enumerate([('Operační systém', 'AlmaLinux 9'), ('Verze serveru', '11.4.5-MariaDB'), ('Účet', 'harbor_admin'), ('Šifrování', 'TLS 1.3'), ('Čas serveru', 'Europe/Prague')]):
    text(c, label, 22, 72+i*34, 150, 12, MUTED)
    text(c, val, 175, 72+i*34, 165, 12)
c = rect(p, 'Doporučení', 280, 704, 1120, 160, '#FFFFFF', 12, BORDER)
text(c, 'Na co se zaměřit', 22, 20, 500, 17, INK, True)
pill(c, 'Upozornění', 22, 62, 102, '#94631A', '#FFF2D8')
text(c, '3 dotazy běží déle než 30 sekund', 140, 66, 580, 14, INK, True)
text(c, 'Prohlédněte si aktivní spojení a jejich stav.', 140, 96, 700, 12, MUTED)
button(c, 'Zobrazit spojení →', 897, 67, 198, dest='activity')


def table(p, title, columns, widths, rows, y=310, row_h=62):
    c = rect(p, title, 280, y, 1120, 88+row_h*len(rows), '#FFFFFF', 12, BORDER)
    text(c, title, 22, 20, 750, 17, INK, True)
    rect(c, 'Záhlaví tabulky', 1, 62, 1118, 36, '#F1F5F6')
    for i, label in enumerate(columns):
        text(c, label, widths[i], 72, 1120-widths[i]-12, 10, MUTED, True)
    for r, row in enumerate(rows):
        yy = 100+r*row_h
        if r:
            line(c, 20, yy-3, 1080)
        for i, value in enumerate(row):
            text(c, value, widths[i], yy+18, (widths[i+1]-widths[i]-12) if i+1 < len(widths) else 1100-widths[i],
                 12, TEAL if i == 0 else INK, i == 0)
    return c


p = shell('databases', '02 Databáze', 1, 'Databáze', 'Databáze a jejich využití na serveru db-prod-01.')
rect(p, 'Hledat databázi', 280, 246, 520, 42, '#FFFFFF', 8, BORDER)
text(p, 'Hledat databázi…', 296, 258, 450, 13, MUTED)
button(p, '+ Vytvořit databázi', 1190, 246, 210)
table(p, 'Databáze na serveru', ['NÁZEV', 'TABULKY', 'VELIKOST', 'ZNAKOVÁ SADA', 'KOLACE'],
      [22, 355, 485, 655, 855], [
          ['commerce', '128', '28,4 GB', 'utf8mb4', 'utf8mb4_unicode_ci'],
          ['analytics', '64', '11,2 GB', 'utf8mb4', 'utf8mb4_unicode_ci'],
          ['identity', '18', '2,1 GB', 'utf8mb4', 'utf8mb4_unicode_ci'],
          ['inventory', '32', '1,1 GB', 'utf8mb4', 'utf8mb4_unicode_ci'],
          ['mysql', '31', '8,4 MB', 'utf8mb4', 'utf8mb4_general_ci'],
          ['information_schema', '84', '—', 'utf8mb3', 'utf8mb3_general_ci']])
text(p, 'Zobrazeno 6 z 8 databází', 280, 812, 500, 12, MUTED)
pill(p, 'Systémové databáze zahrnuty', 1117, 800, 283, '#455E70', '#EAF0F2')

p = shell('users', '03 Uživatelé a oprávnění', 2, 'Uživatelé a oprávnění',
          'Přístupy jsou přiřazeny účtu ve tvaru uživatel@host.')
rect(p, 'Hledat uživatele', 280, 246, 520, 42, '#FFFFFF', 8, BORDER)
text(p, 'Hledat uživatele nebo hostitele…', 296, 258, 450, 13, MUTED)
button(p, '+ Přidat uživatele', 1190, 246, 210)
table(p, 'Účty na serveru', ['ÚČET', 'HOSTITEL', 'PŘÍSTUP', 'AUTENTIZACE', 'STAV'],
      [22, 260, 445, 700, 970], [
          ['harbor_admin', '10.20.0.%', 'Správa instance', 'mysql_native_password', 'Aktivní'],
          ['commerce_app', '10.20.4.%', 'commerce · čtení / zápis', 'mysql_native_password', 'Aktivní'],
          ['analytics_ro', '10.20.6.%', 'analytics · pouze čtení', 'mysql_native_password', 'Aktivní'],
          ['backup_agent', 'localhost', 'Zálohování', 'mysql_native_password', 'Aktivní'],
          ['root', 'localhost', 'Správa instance', 'unix_socket', 'Lokální']])
c = rect(p, 'Poznámka k oprávněním', 280, 758, 1120, 106, '#EDF5F6', 10)
text(c, 'Oprávnění vždy v kontextu databáze', 20, 18, 1000, 14, INK, True)
text(c, 'Detail účtu nabídne databázová oprávnění i globální privilegia. Změny se před uložením zobrazí ke kontrole.', 20, 51, 1070, 12, MUTED)

p = shell('activity', '04 Aktivní spojení', 3, 'Aktivní spojení',
          'Přehled relací a běžících dotazů. Obsah závisí na oprávněních připojeného účtu.')
pill(p, '24 spojení celkem', 280, 252, 155)
pill(p, '3 dlouhé dotazy', 449, 252, 150, '#94631A', '#FFF2D8')
button(p, 'Obnovit přehled', 1210, 246, 190)
table(p, 'Procesy', ['ID / UŽIVATEL', 'DATABÁZE', 'ČAS', 'STAV', 'DOTAZ'],
      [22, 250, 410, 520, 755], [
          ['1842 · analytics_ro', 'analytics', '47 s', 'Sending data', 'SELECT date, SUM(total)…'],
          ['1856 · commerce_app', 'commerce', '35 s', 'Waiting for table lock', 'UPDATE orders SET…'],
          ['1860 · analytics_ro', 'analytics', '32 s', 'Sending data', 'SELECT product_id…'],
          ['1871 · commerce_app', 'commerce', '1 s', 'Sleep', '—']])
c = rect(p, 'Detail vybraného dotazu', 280, 680, 1120, 184, '#FFFFFF', 12, BORDER)
text(c, 'Dotaz #1842', 22, 18, 800, 16, INK, True)
code = rect(c, 'SQL náhled', 22, 57, 1076, 58, '#F0F4F6', 8)
text(code, 'SELECT date, SUM(total) FROM daily_sales GROUP BY date ORDER BY date DESC;', 16, 19, 1040, 12, '#365B66')
text(c, 'Ukončení dotazu bude vyžadovat potvrzení.', 22, 140, 650, 12, MUTED)
button(c, 'Ukončit dotaz…', 898, 130, 200)

p = shell('connect', '05 Nové připojení', 4, 'Připojit MariaDB server',
          'Pojmenujte připojení a nastavte bezpečný přístup k instanci.')
c = rect(p, 'Nastavení připojení', 280, 246, 710, 618, '#FFFFFF', 12, BORDER)
text(c, 'Údaje o připojení', 24, 22, 650, 19, INK, True)
for label, value, xx, yy, ww in [
    ('Název připojení', 'Produkční server', 24, 80, 662),
    ('Hostitel', 'db-prod-01.example.net', 24, 174, 468),
    ('Port', '3306', 510, 174, 176),
    ('Uživatel', 'harbor_admin', 24, 268, 320),
    ('Heslo', '••••••••••••', 364, 268, 322),
    ('Zabezpečení spojení', 'TLS · ověřit certifikát a název serveru', 24, 362, 662)]:
    text(c, label, xx, yy, ww, 12, MUTED, True)
    f = rect(c, label+' — hodnota', xx, yy+27, ww, 44, '#FAFCFC', 7, BORDER)
    text(f, value, 13, 12, ww-24, 13)
text(c, 'Heslo se bez vašeho souhlasu neuloží.', 24, 467, 660, 12, MUTED)
line(c, 24, 507, 662)
button(c, 'Otestovat spojení', 24, 540, 186)
button(c, 'Připojit →', 492, 540, 194, True, dest='overview')
c = rect(p, 'Podporované prostředí', 1014, 246, 386, 325, '#EAF3F3', 12)
text(c, 'Stejná práce na Linuxu', 24, 24, 340, 20, INK, True)
text(c, 'AlmaLinux a Ubuntu', 24, 68, 340, 15, TEAL, True)
text(c, 'Návrh desktopové aplikace v Rustu.\n\nPřipojení k lokální i vzdálené instanci přes databázový protokol.\n\nSpráva systémové služby přes SSH bude samostatný pracovní postup.', 24, 110, 332, 13, MUTED, h=200)
c = rect(p, 'Vysvětlení prototypu', 1014, 595, 386, 269, '#FFFFFF', 12, BORDER)
text(c, 'Toto je návrh', 24, 24, 340, 18, INK, True)
text(c, 'Tlačítko Připojit otevře ukázkový přehled.\n\nFormulář neodesílá údaje a neověřuje skutečný server.', 24, 71, 332, 13, MUTED, h=160)

for source, target in links:
    commands.append({'command': 'proto.interaction.add', 'params': {
        'id': {'$ref': source}, 'dest': {'$ref': frames[target]}}})
commands.append({'command': 'proto.flow.add', 'params': {
    'start': {'$ref': frames['overview']}, 'name': 'Harbor · správa MariaDB'}})
commands.append({'command': 'proto.settings', 'params': {'device': 'None', 'background': '#DDE6E9'}})
OUT.mkdir(exist_ok=True)
(OUT / 'harbor.commands.json').write_text(json.dumps(commands, ensure_ascii=False, indent=2), encoding='utf-8')
print(f'{len(commands)} commands written')
