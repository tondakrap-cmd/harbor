# Harbor terminálové rozhraní

Implementovat `harbor-tui` v Rustu pomocí Ratatui a Crossterm. TUI je
povinný klient společného `harbor-core` a poskytuje stejné databázové
pracovní postupy jako desktop. Běží v lokálním terminálu nebo v interaktivní
SSH relaci bez zobrazovacího serveru.

## Vzhled a rozložení

Výchozí motiv **Turbo Classic** se inspiruje prostředím Turbo Pascalu 6:
modré plochy, světlé rámečky, žluté zvýraznění zkratek, horní menu,
dialogová okna a spodní lišta funkčních kláves. Není potřeba emulovat DOS
ani používat jeho kódování. Texty zůstanou v UTF-8 s českou diakritikou.

Další volitelné motivy jsou **Original, Terra, Clarity a Nocturne**,
převedené do terminálových barevných palet. Nevyžadovat patková písma,
stíny a zaoblení. Podklady jsou v `design/tui-themes.json`; jde o návrhové
tokeny, jejichž kontrast a mapování do omezených palet je nutné ověřit.

Základní členění:

```text
 Server  Databáze  Uživatelé  Spojení  Nastavení  Nápověda
╔══════════════════ Harbor / db-prod-01 ══════════════════╗
║ MariaDB 11.4       Připojeno       Uptime: 18 dní       ║
╠══════════════════╦═════════════════════════════════════╣
║ Databáze         ║ Tabulky       Velikost              ║
║ > commerce       ║ 128           28,4 GB               ║
║   analytics      ║  64           11,2 GB               ║
║   identity       ║  18            2,1 GB               ║
╚══════════════════╩═════════════════════════════════════╝
 F1 Nápověda  F3 Připojení  F5 Obnovit  F10 Menu  Esc Zpět
```

Schéma je ilustrace, nikoli hotový terminálový program. Hodnoty jsou
ukázkové. Požadované pohledy: profily/připojení, přehled s grafem nebo
sparkline, databáze a tabulky, účty a oprávnění, procesy a detail dotazu,
nastavení motivu a obnovování.

Při 120 × 35 znacích využít více panelů. Při 80 × 24 zobrazovat méně
panelů najednou a nabídnout posuv, detail položky a vodorovný posuv tabulky.
Nedostatek místa nesmí skrýt možnost potvrzení nebo zrušení. Změny velikosti
nevymažou formulář a nepřeruší spojení. Pod 80 × 24 zobrazit srozumitelnou
informaci o nedostatečné velikosti a ponechat možnost bezpečného ukončení.

## Klávesnice

| Klávesa | Akce |
| --- | --- |
| F1 | Kontextová nápověda včetně seznamu kláves. |
| F3 | Připojení a přepínání profilů. |
| F5 | Obnovení aktuálního přehledu (na obrazovce Doktor i nová kontrola). |
| 1–8 | Přímý přechod na obrazovku Server … Nastavení (mimo formuláře). |
| Mezerník | Zaškrtnutí oprávnění v editoru oprávnění. |
| F10 / F9 / Alt+M | Aktivace horní nabídky (F9 a Alt+M pro terminály, které F10 zachytí; Ctrl+M terminál posílá jako Enter). |
| Tab / Shift+Tab | Přechod mezi ovládacími prvky nebo panely. |
| Šipky / PageUp / PageDown | Pohyb v nabídce, tabulce nebo dlouhém textu. |
| Enter | Otevření detailu nebo aktivace vybraného prvku. |
| Esc | Zavření menu/dialogu nebo návrat; rozpracovanou změnu nezahodit bez vysvětlení. |

Všechny akce musí být dostupné také přes nabídku, pokud terminál nebo
SSH klient zachytává některé funkční klávesy. Nezavádět jednoznakové
globální zkratky, které by rušily psaní do formulářových polí. Myš může
usnadnit výběr, ale nesmí být nutná.

Zápisové operace mají potvrzovací dialog se správným serverem, účtem
a cílovým objektem. U ukončení dotazu uvést jeho identifikátor a krátký
náhled. Výchozí volba je zrušení. Nezobrazovat heslo v potvrzení.

## Provoz a barvy

- Podporovat moderní UTF-8 terminál a alternativní ASCII rámečky.
- Palety pro truecolor doplnit variantou pro 16 barev a použitelným
  monochromatickým režimem. Samotné RGB hodnoty nejsou důkaz funkčnosti
  v omezeném terminálu; barevné možnosti umožnit ručně přepsat v nastavení.
- Fokus, chyby a stav spojení vyjádřit také textem nebo značkou, nejen barvou.
- Maskovat hesla. Kontrolní a escape sekvence v datech ze serveru
  vykreslovat jako bezpečný text; nesmějí měnit stav terminálu.
- Při přerušení a běžných chybách obnovit raw mode, alternativní obrazovku
  i kurzor. Nezaručovat úklid po nezachytitelném ukončení procesu.
- Logy nesměřovat do aktivní obrazovky TUI; neukládat do nich tajemství.
- Bez interaktivního TTY vypsat vysvětlení a skončit s nenulovým kódem.
- Připojení, oprávnění, TLS a profily řídí stejné jádro jako v desktopu.
  Přihlášení do SSH samo o sobě nezaručuje přístup k MariaDB.

## Ověření a balení

Kontrolovat 80 × 24 a 120 × 35, resize při otevřeném formuláři, dlouhé
hodnoty, české znaky, fokus a potvrzování změn, všech pět motivů,
16barevný a monochromatický režim. Ověřit návrat terminálu po ukončení,
přerušení i simulované chybě. Doplnit automatizované kontroly stavu UI
a vykreslení do testovacího terminálového bufferu tam, kde zachycují
skutečné chování.

Na Ubuntu i AlmaLinux ověřit lokální terminál, interaktivní SSH s PTY
a tmux. Instalovat balíček `harbor-tui` také samostatně na server bez GUI.
Desktopová a terminálová verze musí dosáhnout stejných výsledků při čtení
i při zápisových operacích nad izolovanými testovacími databázemi.
