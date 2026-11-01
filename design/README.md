# Harbor — návrh správce MariaDB

Upravitelný koncept desktopové aplikace v Rustu pro AlmaLinux a Ubuntu.
Tato fáze obsahuje pouze návrh ve FigCraftu; žádný databázový klient ani backend.
Veškeré údaje jsou ukázkové.

## Otevření

Otevřete `harbor-mariadb.figcraft` ve FigCraftu.
V panelu Prototype spusťte tok **Harbor · správa MariaDB**.
Levá navigace přepíná čtyři hlavní obrazovky. Přidat připojení otevírá
pátou obrazovku; Připojit vede zpět na přehled. Zobrazit spojení vede
na aktivní spojení. Ostatní ovládací prvky jsou vizuální návrhy, včetně
vyhledávání, formulářových polí, vytvoření databáze a ukončení dotazu.

## Obrazovky

1. Přehled serveru — metriky, provoz, instance, upozornění.
2. Databáze — přehled velikostí, tabulek a kolací.
3. Uživatelé a oprávnění — účet a hostitel, kontext přístupů.
4. Aktivní spojení — seznam procesů a náhled vybraného SQL.
5. Nové připojení — hostitel, port, účet a TLS.

## Rozhodnutí pro budoucí implementaci

- Správa přes databázový protokol je oddělena od správy služby v OS.
- Provozní graf předpokládá vlastní periodický sběr metrik.
- Údaje o operačním systému nejsou zaručeným výstupem MariaDB protokolu;
  vyžadují metadata profilu nebo budoucí systémovou integraci.
- Dlouhé dotazy závisí na oprávněních k zobrazení procesů.
- Změny oprávnění a ukončení dotazů budou mít vlastní potvrzovací krok.
- Balíčky a skutečná kompatibilita s AlmaLinux a Ubuntu se ověří při implementaci.

## Reprodukce

Generátor vytváří nativní vrstvy a prototypové odkazy pomocí FigCraft CLI.
Nejde o obrázky vložené do plátna. Náhledy jsou v `previews/`.

```powershell
python design/build_design.py
& 'C:\Program Files\ArtCraft\FigCraft\figcraft-cli.exe' run design/harbor.commands.json --out design/harbor-mariadb.figcraft
& 'C:\Program Files\ArtCraft\FigCraft\figcraft-cli.exe' export design/harbor-mariadb.figcraft -o design/previews --format png
```
