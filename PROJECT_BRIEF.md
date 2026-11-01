# Harbor správce MariaDB

Vytvořit správce lokálních i vzdálených MariaDB serverů v Rustu pro Ubuntu
a AlmaLinux se dvěma rozhraními: **desktopové GUI a terminálové TUI**.
Obě rozhraní jsou povinnou součástí projektu a sdílejí databázové jádro.
Dodat samostatně instalovatelné balíčky **DEB a RPM** pro každé rozhraní.
Desktop má čtyři přepínatelné skiny podle návrhů, TUI jejich barevné
adaptace a navíc výchozí **Turbo Classic** ve stylu Turbo Pascalu 6.

Pracovní název je **Harbor**. Návrhy jsou vizuální reference a klikací
prototypy, nikoli již implementovaná aplikace nebo její chování.

## Priority a cílové prostředí

1. Společné spolehlivé jádro v Rustu a stejná pravidla správy databází.
2. Desktopové GUI a plnohodnotné TUI pro lokální terminál i práci přes SSH.
3. Čtyři desktopové skiny, jejich terminálové adaptace a motiv Turbo Classic.
4. Ověřené DEB a RPM balíčky obou klientů pro oba cílové systémy.

Výchozí testovací matice: **Ubuntu 24.04 LTS, AlmaLinux 9, x86_64**,
MariaDB **10.11 a 11.4**. Jsou to pracovní předpoklady, protože konkrétní
verze cílových instalací nebyly určeny. Další vydání distribucí, ARM64
a další verze MariaDB rozšíří podporu až po vlastním ověření.

Desktop lze instalovat na klientský počítač s grafickým prostředím
a připojovat ho k serveru bez grafického prostředí. Na serveru bez displeje
lze přímo spustit `harbor-tui`, také v interaktivní SSH relaci. TUI může
stejně jako desktop přistupovat přes TCP nebo lokální Unix socket.

## Požadované pracovní postupy

### Připojení

- Vytvořit, upravit, pojmenovat a odstranit připojovací profil.
- Připojení přes TCP k lokálnímu i vzdálenému serveru; lokálně také Unix socket.
- Test spojení, připojení, odpojení a srozumitelná informace o selhání.
- Vzdálené připojení ve výchozím nastavení ověřuje TLS certifikát i identitu
  serveru; umožnit vlastní CA. Neprovádět tichý přechod na nešifrované spojení.
- Připojení k testovací instanci bez TLS musí být výslovně zvolené a v UI
  rozpoznatelné. TLS se nevztahuje na lokální Unix socket.
- Heslo v první verzi držet v paměti relace. Volitelné zapamatování jen přes
  systémové úložiště tajemství; pokud není dostupné, nabídnout zadání znovu.
  Neukládat hesla v otevřeném textu, URL, logu ani argumentech procesu.
- Profil uchovává hostitele nebo socket, port, uživatele a TLS volby, nikoli
  samotné heslo. Podporovat alespoň dvě uložená připojení a přepínání mezi nimi.

### Přehled serveru

- Verze serveru, dostupnost spojení, uptime, aktuální počet relací a jejich limit.
- Počet dotazů za sekundu a průběžně vytvářený graf provozu.
- Přibližný součet velikostí viditelných databází a jejich počet.
- Upozornění na dlouhé dotazy, které má přihlášený účet právo vidět.
- Nastavitelný interval obnovování; výchozí návrh 5 sekund. Omezit souběh
  požadavků a při nedostupnosti nezobrazovat stará data jako aktuální.
- Graf se plní měřením po připojení; historická hodina dat se nevymýšlí.
  Demo režim je oddělený a viditelně označený.
- Operační systém a časové pásmo nezískávat odhadem z názvu hostitele.
  Pokud nejsou údaje dostupné, uvést „Nezjištěno“ nebo metadata profilu
  označit jako uživatelsky zadaná.

### Databáze

- Seznam dostupných databází, vyhledávání a přepínač systémových databází.
- Počty tabulek, přibližné velikosti, znaková sada a výchozí kolace.
- Detail databáze se seznamem tabulek a jejich základními metadaty.
- Vytvoření databáze s platným názvem a podporovaným kódováním/kolací,
  pokud má účet odpovídající oprávnění.
- Název databáze zpracovat jako SQL identifikátor; neřetězit neošetřený vstup.

### Uživatelé a oprávnění

- Účty vždy rozlišovat dvojicí uživatel a hostitel.
- Zobrazit uživatele a detail jejich oprávnění v rozsahu práv přihlášeného účtu.
- Vytvořit účet, změnit jeho heslo a přidat/odebrat vybraná databázová oprávnění.
- Před zápisem zobrazit stručný přehled změn bez vypsání hesla.
- Nepředpokládat jednotnou strukturu všech systémových tabulek ve všech
  verzích MariaDB. Operace a dostupné autentizační mechanismy ověřovat
  podle cílové verze; při neznámém mechanismu nic nepřepisovat automaticky.
- Chybějící oprávnění vysvětlit; neobcházet je přihlášením jako root.

### Aktivní spojení

- Zobrazit procesy, uživatele, databázi, dobu běhu, stav a dostupný text dotazu.
- Filtrovat dlouhé dotazy a zobrazit detail vybraného dotazu.
- U oprávněného účtu umožnit po potvrzení **ukončení dotazu**; nezaměňovat
  ho s ukončením celé relace. Cíl znovu ověřit podle dostupných údajů.
- Rozlišovat běžící SQL a neaktivní relace; dlouhý `Sleep` nepovažovat
  automaticky za dlouhý dotaz. Nezobrazovat vlastní monitorovací relaci
  jako problémový dotaz.

## Grafika a čtyři skiny

Povinné skiny jsou **Original, Terra, Clarity a Nocturne**. Výchozí je
Original. Terra čerpá vizuální inspiraci z Anthropic, Clarity z Apple;
v produktu používat vlastní názvy, značku Harbor a otevřeně licencovaná písma.

Implementovat jeden soubor obrazovek a společné komponenty. Skin mění
barevnost, typografii, okraje, zaoblení, stíny a případně kompozici přehledu;
nemění databázové operace, oprávnění ani obsah. Nocturne má v přehledu
jinak rozmístěné metriky, takže nestačí pouze přebarvit původní obrazovku.

Přepínač v nastavení funguje okamžitě a zvolený skin se uloží. Přepnutí
nesmí odpojit server, vymazat formulář ani zrušit výběr. Všechny dialogy,
tabulky, grafy, chybové, načítací a prázdné stavy musí odpovídat skinu.

Podkladem je `design/variants/harbor-all-variants.figcraft` a náhledy.
`design/themes.json` obsahuje výchozí tokeny, které je nutné při implementaci
ověřit proti návrhům a doplnit pro stavové komponenty. Aplikace vykresluje
skutečné ovládací prvky; nesmí používat screenshot jako své rozhraní.

První jazyk UI je čeština s diakritikou. Rozložení musí fungovat alespoň
v 1024 × 720 logických bodech i ve výchozích 1440 × 960, při HiDPI a ovládání
klávesnicí. Při nedostatku místa obsah správně posouvat; neztratit hlavní
akce mimo okno. Stav nevyjadřovat pouze barvou.

## Doporučené technické řešení

Rust workspace s knihovnou `harbor-core`, desktopem `harbor-desktop`,
terminálovým klientem `harbor-tui` a modulárním zpracováním skinů. Pro GUI
zvolit `egui/eframe`, pro TUI `Ratatui` a `Crossterm`, pro asynchronní
operace Tokio a pro MariaDB SQLx s ovladačem MySQL/MariaDB a TLS.
Před implementací připnout ověřené verze a vložit `Cargo.lock`.

Ani jedno UI nemá obsahovat SQL ani blokovat při síťové operaci. Jádro poskytuje
typované operace a data; demo a reálný backend sdílejí rozhraní. Chyby
rozdělit alespoň na síťové, autentizační, TLS, chybějící oprávnění
a nepodporovanou funkci. Viz `docs/ARCHITECTURE.md`.

Pro desktop ani TUI není potřeba trvale běžící služba, webový server
ani otevřený administrační port. Oba klienti volají Rust knihovnu přímo;
nevytvářet mezi nimi pomocný server. Nedělat druhou implementaci SQL
pro jiné rozhraní nebo každý vzhled.

## DEB a RPM

- Dodat desktopový balíček `harbor` a samostatný balíček `harbor-tui`,
  každý jako DEB pro Ubuntu a RPM pro AlmaLinux. Desktop obsahuje položku
  v nabídce aplikací a ikonu, oba balíčky potřebné prostředky a licence.
- TUI nesmí záviset na instalaci desktopového balíčku, GUI knihoven,
  zobrazovacím serveru ani desktopových písem. Každý klient používá vlastní
  binární soubor a oba lze nainstalovat současně bez kolizí prostředků.
- Balíček instaluje klienta; neinstaluje ani nepřekonfigurovává automaticky
  MariaDB server, firewall, autentizaci nebo SELinux.
- Nevypínat SELinux kvůli funkčnosti. GUI běží pod běžným uživatelem.
- Sestavovat proti odpovídající základně každé distribuce. Převod DEB na RPM
  nebo přejmenování souboru není splnění podpory druhého systému.
- Popsat knihovní a grafické závislosti, konfiguraci a standardní umístění dat.
- Ověřit instalaci, spuštění, upgrade a odinstalaci obou klientů na čistých
  cílových systémech; TUI také samostatně na serveru bez grafického prostředí.
  Zachovat uživatelské profily při upgradu a běžné odinstalaci.
- Do vydání přiložit verzi, kontrolní součty a výsledky testovací matice.
  Distribuční podpisy a repozitář lze přidat v navazující etapě.

## TUI ve stylu Turbo Pascalu 6

`harbor-tui` je povinný terminálový klient se stejnými databázovými
pracovními postupy jako desktop: profily, přehled, databáze, uživatelé,
oprávnění a aktivní dotazy včetně řízených změn. Jde o samostatné rozhraní
nad `harbor-core`, nikoli výpis SQL výsledků nebo simulaci desktopového okna.

Použít horní nabídku, rámečky, dialogy, tabulky a spodní lištu klávesových
zkratek. Výchozí **Turbo Classic** má modré plochy, světlé rámečky a žluté
zvýraznění. Nabídnout také terminálové barevné adaptace Original, Terra,
Clarity a Nocturne. Písma a grafické efekty desktopu nejsou podmínkou TUI.

Vše musí být dostupné z klávesnice. Myš je pouze doplněk; zajistit práci
v interaktivní SSH relaci a tmux, změnu velikosti terminálu a použitelný
režim alespoň 80 × 24 znaků. Při menším prostoru skrýt vedlejší panely,
posouvat tabulky a dát přednost viditelnosti hlavních akcí.

Po ukončení, přerušení i chybě obnovit terminál. Dostupná data, chyby,
potvrzení změn i omezení účtu musí být srozumitelné v obou rozhraních.
Podrobnosti a návrh kláves jsou v `docs/TUI.md`; výchozí palety v
`design/tui-themes.json`.

## Etapy a kritéria dokončení

| Etapa | Výsledek | Ověření |
| --- | --- | --- |
| 1. Obě UI a skiny | Spustitelné GUI i TUI, společné jádro a označený demo režim; čtyři desktopové a pět terminálových motivů. | Přepnutí motivů bez ztráty stavu, české texty, dvě velikosti okna a terminálu, navigace klávesnicí. |
| 2. Skutečné přehledy | Oba klienti pracují s testovací MariaDB: metriky, databáze, procesy a dostupná oprávnění. | Integrační testy, TLS/špatné heslo/omezený účet/výpadek spojení, porovnání dat se serverem v obou UI. |
| 3. Řízené změny | V obou UI vytvoření DB a účtu, změna hesla, vybraná oprávnění a ukončení dotazu. | Testovací instance, potvrzovací dialogy, správné cílení a nepovolené operace. |
| 4. Distribuce | DEB a RPM pro každý klient s dokumentací, celkem čtyři výstupní balíčky. | Čisté Ubuntu a AlmaLinux, instalace, skutečné spuštění, upgrade a odinstalace; TUI i bez GUI a přes SSH. |

Za dokončený hlavní produkt se považují etapy 1 až 4. Kompilace bez
ověřeného spuštění nebo vytvořený balíček bez instalačního testu nestačí.
Neprovedené kontroly uvádět jako neprovedené, zejména pokud vývoj probíhá
na Windows a cílové prostředí je Linux.

## Mimo první vydání

Restart a aktualizace služby MariaDB, úpravy systémové konfigurace,
SSH tunely, plnohodnotný SQL editor, editace dat, mazání databází a účtů,
zálohovací systém a správa replikace nejsou podmínkou prvního vydání.
Databázová oprávnění a operační systém tvoří různé oblasti správy.

## Podklady k technickým volbám

- [egui](https://docs.rs/egui/latest/egui/): GUI v Rustu, DPI a dostupné možnosti integrace.
- [SQLx](https://github.com/transact-rs/sqlx): ovladač MariaDB/MySQL, Tokio a TLS.
- [TLS režimy SQLx](https://docs.rs/sqlx/latest/sqlx/mysql/enum.MySqlSslMode.html): explicitní `VerifyIdentity`; výchozí `Preferred` nestačí pro uvedené požadavky.
- [MariaDB SHOW GLOBAL STATUS](https://mariadb.com/docs/server/reference/sql-statements/administrative-sql-statements/show/show-status): globální čítače pro provozní metriky.
- [MariaDB SHOW PROCESSLIST](https://mariadb.com/docs/server/reference/sql-statements/administrative-sql-statements/show/show-processlist): viditelnost procesů závisí na oprávněních.
- [cargo-deb](https://github.com/kornelski/cargo-deb) a [cargo-generate-rpm](https://github.com/cat-in-136/cargo-generate-rpm): možné nástroje pro tvorbu distribučních balíčků.
- [Ratatui](https://ratatui.rs/) a [Crossterm](https://github.com/crossterm-rs/crossterm): Rust knihovny pro terminálové rozhraní, vstup a řízení terminálu.

Architektura a etapizace výše jsou rozhodnutí tohoto zadání; odkazy
dokládají možnosti nástrojů, nikoli již provedenou implementaci Harboru.
