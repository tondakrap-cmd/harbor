# Harbor nový projekt

Zkopírujte **celý obsah této složky** do prázdné pracovní složky nového projektu,
například `Harbor`. Nezachovávejte nad ní obalové adresáře `handoff/harbor`.
Stejný obsah je připraven v archivu `harbor-project-handoff.zip` vedle této složky.

Po zkopírování bude v kořeni projektu:

```text
Harbor/
  START_HERE.md
  PROJECT_BRIEF.md
  START_PROMPT.txt
  TRANSFER_MANIFEST.json
  docs/
    ARCHITECTURE.md
    TUI.md
  design/
    README.md
    DESIGN_HANDOFF.md
    themes.json
    tui-themes.json
    harbor-mariadb.figcraft
    previews/
    build_design.py
    build_variants.py
    variants/
      harbor-all-variants.figcraft
      comparison.png
      anthropic/
      apple/
      nocturne/
      original/
      combine.py
```

V novém projektu vložte obsah `START_PROMPT.txt` jako první zadání.
Úplné požadavky jsou v `PROJECT_BRIEF.md`; doporučené technické řešení
v `docs/ARCHITECTURE.md`. Terminálové rozhraní a ovládání popisuje `docs/TUI.md`.

Ve FigCraftu otevřete `design/variants/harbor-all-variants.figcraft`.
Stránky **Original, Terra, Clarity a Nocturne** obsahují čtyři vzhledy
téže aplikace. Jejich hlavní obrazovky porovnáte také v
`design/variants/comparison.png`.

## Co je v balíčku

Zadání desktopového i terminálového správce, technické rozhodnutí, barevné
podklady pro obě UI, všechny čtyři grafické návrhy a jejich náhledy,
generátory návrhů a prototypové příkazy. Přenosový manifest obsahuje
kontrolní součty souborů. TUI má navíc motiv Turbo Classic ve stylu
Turbo Pascalu 6; jeho specifikace je textová, nikoli návrh ve FigCraftu.

Jde o podklady k vývoji. Balíček zatím neobsahuje implementaci GUI či TUI
ani instalátory DEB/RPM. Neukládá přístupové údaje
a nevyžaduje připojení k produkční MariaDB.

## Co není třeba přenášet z původního projektu

Žádné další části pracovního adresáře ArtCraft nejsou potřeba. Ladicí logy,
pracovní inspekční JSON, instalaci FigCraftu, osobní nastavení ani tuto
konverzaci nekopírujte. FigCraft je užitečný pro úpravy návrhů; budoucí
spuštěná aplikace na něm nebude záviset.

## Pracovní předpoklady

Povinné výstupy jsou desktopový balíček `harbor` a terminálový
`harbor-tui`, každý ve formátu DEB i RPM. Oba používají společné Rust jádro.
Terminálový klient funguje také bez grafického prostředí a přes SSH.

První cíle jsou **Ubuntu 24.04 LTS a AlmaLinux 9, x86_64**. Kompatibilitu
s dalšími verzemi a ARM64 lze přidat po ověření základních balíčků. Jde
o výchozí rozsah projektu, nikoli o tvrzení, že jiné verze nelze podporovat.
Testovací databáze jsou MariaDB 10.11 a 11.4. Pokud máte jiné konkrétní cíle,
upravte tyto předpoklady před zahájením balení aplikace.
