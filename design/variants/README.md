# Harbor — čtyři varianty návrhu

Čtyři upravitelné návrhy FigCraft. Každý obsahuje pět obrazovek
1440 × 960 a 27 navigačních interakcí. Tři nové varianty mají navíc
sdílené barevné a typografické styly.
Původní koncept je uložen samostatně v `../harbor-mariadb.figcraft`.

**Nejrychlejší porovnání:** otevřete `harbor-all-variants.figcraft`. Čtyři stránky
Terra, Clarity, Nocturne a Original obsahují všechny varianty v jednom dokumentu,
včetně samostatných prototypových toků. `comparison.png` zobrazuje čtyři hlavní
obrazovky v mřížce 2 × 2.

| Varianta | Soubor | Vizuální směr |
| --- | --- | --- |
| Anthropic / Terra | `anthropic/harbor-anthropic.figcraft` | Krémový podklad, terakota, patkové nadpisy Merriweather, střídmé rámečky a oddělovací linky. |
| Apple / Clarity | `apple/harbor-apple.figcraft` | Světlé povrchy, modré akcenty, vsazený postranní panel, měkké stíny a zaoblené ovládací prvky. |
| Nocturne | `nocturne/harbor-nocturne.figcraft` | Vlastní tmavý návrh s mátovými akcenty, dominantní metrikou provozu a přeskládaným přehledem serveru. |
| Original | `../harbor-mariadb.figcraft` | Původní koncept se světlým obsahem, tmavým postranním panelem a tyrkysovými akcenty. |

První dva směry jsou volné vizuální inspirace. Harbor má vlastní značku;
nejde o oficiální produkty Anthropic ani Apple. Cílovou aplikací zůstává
desktopový správce MariaDB v Rustu pro AlmaLinux a Ubuntu.

## Prohlížení

Ve FigCraftu otevřete zvolený soubor a v panelu Prototype spusťte tok:

- **Harbor / Terra**
- **Harbor / Clarity**
- **Harbor / Nocturne**
- **Harbor / Original** (ve společném souboru)

Všechny varianty obsahují přehled serveru, databáze, uživatele a oprávnění,
aktivní spojení a nové připojení. Kliká levá navigace, Přidat připojení,
Připojit a odkaz z upozornění na dlouhé dotazy.

Ostatní tlačítka a formuláře jsou pouze vizuální návrh. Data jsou smyšlená;
návrhy nekomunikují s MariaDB. Konfigurace systému vyžaduje samostatnou
implementaci. Náhledové PNG jsou ve složkách `previews/`.

## Editace a reprodukce

Veškerý obsah tvoří nativní rámečky, texty a vektory, nikoli vložené snímky.
U tří nových variant jsou barevné a typografické styly propojené s vrstvami. Ikony jsou vlastní
jednoduché vektory; používají se písma dostupná v místní instalaci FigCraftu.

Z kořene projektu:

```powershell
python design/build_variants.py
& 'C:\Program Files\ArtCraft\FigCraft\figcraft-cli.exe' run design/variants/anthropic/build.commands.json --out design/variants/anthropic/harbor-anthropic.figcraft
& 'C:\Program Files\ArtCraft\FigCraft\figcraft-cli.exe' run design/variants/apple/build.commands.json --out design/variants/apple/harbor-apple.figcraft
& 'C:\Program Files\ArtCraft\FigCraft\figcraft-cli.exe' run design/variants/nocturne/build.commands.json --out design/variants/nocturne/harbor-nocturne.figcraft
```

Generátor `../build_variants.py` sdílí obsah obrazovek a odděluje vzhledové
volby. Pro každou variantu uchovává přesný seznam příkazů v `build.commands.json`.

`python design/variants/combine.py` znovu vytvoří společný soubor. Při importu
odděluje identifikátory sdílených stylů jednotlivých variant. Ověření v
`combined-verification.json` potvrzuje 20 obrazovek, 108 platných navigačních
odkazů a 174 oddělených stylů; hlavní obrazovky ve společném souboru mají
stejný render jako samostatné projekty.
