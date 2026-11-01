# Harbor grafické podklady pro vývoj

Implementovat čtyři skiny stejného nástroje. Všechny jsou volitelné
uživatelem za běhu; výchozí je Original. Název aplikace je Harbor.

| Skin | Referenční návrh | Charakter |
| --- | --- | --- |
| Original | `harbor-mariadb.figcraft` | Tmavý tyrkysový panel, světlý obsah a přehledné provozní karty. |
| Terra | `variants/anthropic/harbor-anthropic.figcraft` | Krémová, terakota, patkové nadpisy a střídmé linky; inspirace Anthropic. |
| Clarity | `variants/apple/harbor-apple.figcraft` | Světlé plochy, modré akcenty, zaoblení a měkké stíny; inspirace Apple. |
| Nocturne | `variants/nocturne/harbor-nocturne.figcraft` | Tmavé prostředí a mátové akcenty, dominantní karta aktuálního provozu. |

Souhrn je v `variants/harbor-all-variants.figcraft`. Obsahuje 20 obrazovek
na čtyřech stránkách. Náhledy jednotlivých obrazovek jsou v přiložených
složkách `previews/`; celkové porovnání v `variants/comparison.png`.

## Implementace

`themes.json` obsahuje výchozí barevné a typografické tokeny. Jde o strojově
čitelnou referenci pro vývoj, nikoli hotové schéma runtime konfigurace.
Barvy vybírat podle sémantických rolí; nešířit barevné konstanty do každého
widgetu. Při úpravě kontrastu zachovat základní paletu a vizuální charakter.

Návrhy mají rozměr 1440 × 960. Jejich titulková lišta je pouze součást
vizuální předlohy; ve skutečné aplikaci používat výchozí nativní dekorace
okna podle desktopového prostředí, pokud není důvod pro vlastní.

Nocturne používá jinak členěný přehled. Jeho kompozici zachovat pomocí
varianty rozložení týchž komponent, nikoli oddělenou implementací operací.

Texty a čísla v návrzích jsou příklady. Časový rozsah grafu, stav TLS,
verze serveru i případné informace o systému musí v aplikaci odpovídat
skutečnosti. Vyplněný formulář v návrhu není použitelná konfigurace serveru.

Před distribucí přibalit licence fontů a dalších použitých prostředků.
Návrh Clarity nevyžaduje písmo San Francisco ani žádný jiný proprietární
prostředek Apple. Používat vlastní označení skinů a značku Harbor.

## Terminálové motivy

TUI má vlastní vykreslování podle `../docs/TUI.md` a palet v
`tui-themes.json`: Turbo Classic jako výchozí, dále Original, Terra,
Clarity a Nocturne. Čtyři grafické návrhy ve FigCraftu jsou referencí
pro charakter a barvy; nepředstavují hotové návrhy terminálových obrazovek.
TUI zachovává stejné operace a význam dat, ne pixelové rozměry desktopu.

## Reprodukce návrhů

Generátory `build_design.py` a `build_variants.py` vytvářejí JSON příkazy
pro FigCraft CLI. `variants/combine.py` skládá návrhy do souhrnného souboru
a odděluje identifikátory jejich stylů. Není potřeba spouštět je při
vývoji nebo běhu samotného správce databází.

FigCraft CLI lze pro skládání nastavit proměnnou `FIGCRAFT_CLI` nebo
zpřístupnit jako `figcraft-cli` v `PATH`. Podklady nevyžadují původní
absolutní cesty pracovního adresáře ArtCraft.
