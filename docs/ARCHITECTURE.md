# Harbor technický návrh

Základ tvoří společné doménové jádro v Rustu a dva povinní klienti:
desktopové GUI a terminálové TUI. Oba volají knihovnu jádra přímo.
Nezavádět webový server, distribuované služby ani správu systémové služby MariaDB.

## Struktura workspace

```text
crates/
  harbor-core/       modely, typované operace, backend, schopnosti účtu
  harbor-desktop/    egui/eframe, UI stav, komponenty a vykreslování skinů
  harbor-tui/        Ratatui/Crossterm, terminálový vstup, obrazovky a dialogy
  harbor-themes/     sémantické palety, definice motivů bez závislosti na UI
packaging/
  deb/
  rpm/
tests/
  integration/
design/             návrhy a vizuální podklady
docs/
```

Pokud to zjednoduší první verzi, definice motivů může být sdílený modul.
Jádro ani terminálový klient nesmějí tranzitivně záviset na GUI knihovnách.
Mapování palet na egui a Ratatui patří do příslušného klienta.

## Rust a databázové operace

Navržené knihovny jsou egui/eframe, Ratatui/Crossterm, Tokio,
SQLx s MySQL/MariaDB backendem, serde, tracing a podle potřeb thiserror.
Verze ověřit společně, připnout
toolchain a lockfile. Vyhnout se systémové závislosti na klientovi MariaDB,
pokud není potřeba pro konkrétní chybějící funkci ovladače.

`harbor-core` přijímá typovaná data, nikoli příkazy UI. Příklady operací:
`connect`, `server_snapshot`, `list_databases`, `list_tables`, `list_accounts`,
`account_grants`, `create_database`, `create_account`, `change_password`,
`update_database_grants`, `list_processes`, `kill_query`.

Jádro neimportuje egui, Ratatui ani Crossterm a neví, který skin je vybraný.
SQL není rozptýlené
v obsluze tlačítek. Zápisové operace validují parametry a pracují s minimálními
potřebnými oprávněními; názvy objektů a účtů vyžadují vlastní bezpečné
zpracování, protože SQL identifikátor není běžný vázaný parametr.

Tokio runtime a databázový pool spravuje oddělená pracovní vrstva.
Oba klienti dostávají výsledky přes zprávy; nepoužívají `block_on` uvnitř
vykreslovací smyčky. Timeouty, zrušení práce a odpojení jsou součástí modelu.
Změna vybraného profilu musí zabránit zobrazení opožděného výsledku
z předchozího serveru. Pool je omezený a pravidelné dotazy se nehromadí.

Demo backend používá stejnou strukturu dat a předvídatelné příklady.
Připojený provoz nikdy nenahrazuje skutečná data demo hodnotami.

## Metriky a dostupné schopnosti

Pro QPS použít rozdíl vhodného globálního čítače dotazů mezi dvěma měřeními
dělený skutečně uplynulým monotónním časem. Vybraný čítač, například
`Questions`, zdokumentovat. Po restartu, resetu čítače nebo změně spojení
založit nový úsek; první vzorek nezobrazovat jako vypočtenou rychlost.
Uchovávat omezený počet vzorků. Přibližné velikosti tabulek neoznačovat
jako přesné využití souborového systému.

Dostupné operace určují verze serveru a oprávnění aktuálního účtu.
Chybějící globální přehled nebo systémové tabulky mají vést k částečnému
zobrazení s vysvětlením, nikoli k pádu celého přehledu. Backend stále
kontroluje chyby i tehdy, když UI operaci považovalo za dostupnou.

TLS pro vzdálená připojení nastavit výslovně na ověření identity; výchozí
režim ovladače nepovažovat automaticky za bezpečný. Profily oddělit od
tajemství, konfigurační soubory ukládat standardně podle XDG a zapisovat
atomicky. Logy nezahrnují hesla, spojovací URL ani celý obsah citlivých SQL.

## Vzhledy a návrhy

Typ `ThemeId` rozlišuje `Original`, `Terra`, `Clarity`, `Nocturne`.
Téma zahrnuje sémantické barvy, písma, měřítko, zaoblení, rámečky a stíny.
Výchozí tokeny jsou v `design/themes.json`. Layout přehledu může mít
varianty `Standard` a `HeroMetric`, ale používá stále stejný datový model.

Pro TUI přidat výchozí `TurboClassic` a barevné adaptace čtyř témat.
`design/tui-themes.json` popisuje palety a doporučené ANSI náhrady.
TUI nepoužívá fonty, zaoblení ani stíny desktopu. Volbu tématu ukládat
pro každé rozhraní zvlášť, profily připojení mají společné schéma.
Při souběžném běhu klientů chránit konfiguraci před přepisem cizí změny.

Rozpracované formuláře, otevřený profil, výběr, filtry a stav spojení
nepatří do objektu tématu. Přepnutí tématu pouze změní způsob vykreslení.
Používat vlastní typografii a otevřená písma s přibalenou licencí, například
Inter, Merriweather a Liberation Mono; nekopírovat ochranné známky ani
proprietární fonty z předloh.

Návrhy obsahují pouze pět referenčních obrazovek. Nastavení, potvrzení,
chyby a prázdné stavy implementovat ve stejném systému komponent.
Pokud kvůli použitelnosti upravíte rozměry, zachovejte charakter skinu
a zaznamenejte důvod v dokumentaci.

## Linuxové balíčky a ověřování

Pro DEB je vhodný kandidát cargo-deb; pro RPM cargo-generate-rpm nebo
distribuční spec při potřebě složitějších pravidel. Způsob zvolit podle
reálných závislostí GUI a možností cílové distribuce. Nastavení balíčků
a skripty uchovávat v repozitáři, nevytvářet je ručně mimo projekt.

Desktopový balíček `harbor` a terminálový `harbor-tui` stavět samostatně
pro každý cílový systém. TUI sestavení se nesmí opírat o výchozí workspace
features, které přitáhnou egui nebo grafický backend. Ověřit i instalaci
pouze `harbor-tui` na čistý server bez grafického prostředí.

Nativní závislosti a glibc omezují přenositelnost i u programu napsaného
v Rustu. Budovat samostatně na kompatibilním Ubuntu a AlmaLinux prostředí.
Kontejner postačí pro část sestavení a databázových testů; spuštění GUI
a integraci s desktopem ověřit na skutečném grafickém systému nebo VM.
Otestovat Wayland a X11 tam, kde jsou v cílovém prostředí dostupné.

CI zahrne `cargo fmt --check`, `cargo clippy` pro podporované cíle, testy
jádra a integrační testy proti izolovaným MariaDB. Chování důležité pro
bezpečnost a správnost má mít testy: TLS, oprávnění, zpracování identifikátorů,
zápisové operace a reset čítačů. Vizuální kontrola musí projít všemi skiny
obou klientů. Pro TUI přidat testy vstupu, resize, potvrzení změn a obnovy
terminálu, včetně interaktivní SSH relace a tmux.

## Dvě rozhraní nad jedním jádrem

```text
Desktop egui/eframe ──┐
                     ├── harbor-core ── MariaDB
TUI Ratatui/Crossterm ┘
```

TUI má vlastní stav navigace, fokus, formuláře a obsluhu terminálu,
ale sdílí datové typy, validaci, databázové operace a pravidla oprávnění.
Potvrzovací dialog zobrazí každý klient vlastními prostředky; samotné
provedení změny a validace cíle patří do jádra.

Smyčka TUI zpracovává vstup, resize a výsledky asynchronních operací.
Vykreslování odlišit od síťové práce; při čekání na server zůstává funkční
navigace a zrušení. Správu raw mode, alternativní obrazovky a kurzoru
zapouzdřit do guardu, který obnoví terminál při ukončení a běžných chybách.
Panic hook a obsluha zachytitelných signálů doplní řízený úklid.

V aktivní TUI relaci nesmějí běžné logy rozbíjet obrazovku; používat
redigovaný log mimo terminál. Při chybě bez TTY vypsat krátkou zprávu
a skončit, nepřecházet do rozbité interaktivní relace. Podrobné požadavky
na klávesy, barvy a chování jsou v `TUI.md`.
