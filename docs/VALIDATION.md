# Stav ověření Harboru

Průběžný záznam, 31. 10. 2026. Implementace ani existence balíčku sama o sobě
neznamená splnění celé cílové matice. Tento soubor se aktualizuje podle výsledků.

## Prostředí a závislosti

- Vývoj: skutečný AlmaLinux 9.8, x86_64, glibc 2.34.
- DEB build a testy workspace: izolovaný Ubuntu 24.04.5 LTS, x86_64, glibc 2.39.
- Rust/Cargo 1.88.0 nainstaloval uživatel. Přímé závislosti mají přesné verze,
  skutečně vyřešené tranzitivní závislosti jsou v Cargo.lock. Workspace se přeložil.
- eframe 0.31.1, Ratatui 0.29.0 / Crossterm 0.28.1, Tokio 1.44.2, SQLx 0.8.6
  s MySQL a rustls. TUI ani core nezávisí na GUI. Profily neobsahují hesla.
- Síť, vytváření socketů, přístup k X serveru a uživatelské namespaces jsou
  v prostředí agenta omezené. Uživatel stáhl závislosti do projektové cache a
  spustil integrační skripty mimo toto omezení. Logy jsou v `.test-data`.
- Původní systémová MariaDB 10.5 nebyla použita jako důkaz cílové kompatibility.
  Pokus založit samostatnou instanci v `.test-data/native` narazil na zákaz
  vytváření socketu; systémový server ani jeho konfigurace nebyly změněny.

## Provedené kontroly

| Kontrola | Výsledek a rozsah |
| --- | --- |
| `cargo check --workspace --offline` | Prošlo na AlmaLinux 9.8. |
| `cargo clippy --workspace --all-targets -- -D warnings` | Prošlo; opakuje se po změnách. |
| Testy workspace | 12 testů domény, konfigurace, asynchronního stavu a obou UI prošlo na AlmaLinux i Ubuntu; síťové integrační testy jsou výslovně ignorované v běžném běhu a spouštějí se zvlášť. |
| GUI vykreslení do egui kontextu | Všech 6 obrazovek, 4 skiny, 1024×720 a 1440×960; zachování rozpracované hodnoty a výběru. Není to kontrola DPI ani vizuální shody. Uživatel potvrdil otevření skutečného desktopového okna v demo režimu. |
| TUI testovací buffer | Všech 6 obrazovek, 5 motivů, truecolor/16/mono, 80×24, 120×35 a malý terminál; stav formuláře, nabídka a výchozí zrušení potvrzení. |
| Skutečné TUI v PTY | `scripts/test-pty.py`: běh binárního souboru, resize, Ctrl+C, SIGINT, SIGTERM, SIGHUP; obnovení termios, alternativní obrazovky a kurzoru. Bez TTY končí s vysvětlením a nenulovým kódem. |
| MariaDB 10.11.19 | Uživatel spustil `test-instances.sh`, uložený log potvrzuje úspěch. Izolovaný kontejner bez TCP, projektový socket a kontrolní tabulka. |
| MariaDB 11.4.13 | Stejný integrační test prošel, viz `.test-data/mariadb-11.4/test.log`. |
| Rozsah databázových testů | Skutečný přehled, vytvoření DB s uvozovaným identifikátorem, detail tabulek s českým názvem, vytvoření účtu s apostrofem, GRANT/REVOKE s doslovným podtržítkem a odmítnutím přístupu k sousední databázi, změna hesla s apostrofem a lomítkem, chybné heslo, omezený účet a odmítnutý zápis, odmítnutí změněného cíle KILL QUERY, ukončení připraveného dotazu a následný SELECT ve stejné relaci. Závěrečné běhy prošly na obou verzích. |
| TLS | Test prošel na MariaDB 10.11 i 11.4: správná CA a identita, odmítnutí chybné CA i identity, špatné heslo a explicitní nešifrovaný režim. Uživatel spustil `test-tls.sh`; uložené logy potvrzují úspěch. |
| RPM | Revize 2 obou balíčků sestavena nativně na AlmaLinux 9.8. V čistém kontejneru prošla samostatná instalace TUI a jeho PTY test, společná instalace s desktopem, načtení runtime knihoven, spuštění GUI v Xvfb, reinstalace stejné verze a odinstalace se zachováním uživatelského souboru. |
| DEB | Revize 2 obou balíčků vytvořena na Ubuntu 24.04 se zachováním původních nativních binárek po bajtech. V čistém Ubuntu prošly stejné instalační a provozní kontroly jako RPM, včetně GUI v Xvfb. |
| Shoda vydání | Obě nativní sestavení aplikace 0.1.0 mají shodný hash zdrojů `15ac6d02c3ecdc3b99592151ea0c6dddc7a70767ec077459003deea55ee91ca1`. Revize balení je oddělená v `packaging/revision` a BUILD-INFO.json. Samotná shoda hashů nepotvrzuje funkčnost instalace. |
| Ubuntu s LXQt | Očekávaná funkčnost: aplikace podporuje X11/Wayland a nevyžaduje konkrétní desktop. Skutečná relace LXQt zatím nebyla testována. |
| SSH a tmux | Na Ubuntu i AlmaLinux prošlo nainstalované samostatné TUI v místním tmux, přes skutečné SSH s PTY i v tmux přes SSH. Testy mění velikost, navigují a ověřují ukončení a obnovu terminálu. SSH poslouchá jen uvnitř testovacího kontejneru, bez publikovaných portů. |
| Aktualizace balíčků | `test-upgrade.sh` prošel na obou distribucích. Přechod na revizi 2 automaticky doinstaloval XKB knihovnu, nainstalované TUI znovu prošlo PTY testem a profily i motivy zůstaly po aktualizaci i následné odinstalaci zachované. Pro RPM se používá níže popsaný kontrolní předchozí balíček. |

Databázové testy objevily a pomohly opravit rozdíl mezi prázdným heslem a
nepředaným heslem v SQLx, binární kolace metadat účtů a příkaz `Execute` u
připravených běžících SQL. Jsou to skutečně provedené testy, nikoli závěr z kompilace.

První spuštění nainstalovaného GUI v čistém Ubuntu odhalilo chybějící runtime
knihovnu `libxkbcommon-x11.so.0`. Tuto knihovnu načítá grafický backend přes `dlopen`,
takže se neobjeví jako běžná ELF závislost v `ldd`. Receptury DEB/RPM ji nyní
vyžadují výslovně; instalační test kontroluje načtení runtime knihoven i spuštění
GUI v Xvfb. Knihovna není závislostí TUI. Staré balíčky se uchovávají v
`previous-revision` a aktuální instalátory mají revizi 2, aby fungovala aktualizace.
Na AlmaLinuxu instalační test odhalil i chybný název balíčku Waylandu; receptura
už používá ověřený `libwayland-client`. Opakovaný čistý instalační test prošel.

`test-upgrade.sh` ověřuje balicí aktualizaci z revize 1 na 2. Pro DEB používá
původní skutečné balíčky. Původní desktopové RPM mělo nesplnitelnou závislost
Waylandu, proto pro RPM slouží výslovně označený kontrolní předchozí balíček:
`prepare-upgrade-fixture.sh` zabalí nativní AlmaLinux binárky s verzí 0.1.0-1,
opraveným názvem Waylandu a bez nové XKB závislosti. Je to test přechodu verzí,
automatického doplnění závislosti a zachování konfigurace, nikoli historického
vydání s jinou implementací aplikace. Výsledky obou aktualizačních testů potvrzují
závěrečné řádky `PASS revision 1 to 2 upgrade` v `dist/*/upgrade.log`.

## Zbývající kontroly a omezení

- Skutečné GUI ve všech skinech, 1024×720/1440×960, HiDPI, klávesnice, X11/Wayland.
  Spuštění agentem selhalo na XOpenDisplayFailed; uživatel potvrdil otevření okna
  ve svém prostředí. Snímky návrhů nejsou důkazem běhu aplikace.
- Ruční porovnání obou skutečných UI se serverem, včetně zápisových dialogů;
  integrační test zatím ověřuje sdílené jádro.
- Výpadek serveru během čtení i zápisu; časový limit zápisu má neurčitý výsledek
  a před opakováním vyžaduje ověření. Během zápisu klient nepřepíná server.
- Simulovaná I/O chyba a panic v TUI; běžné ukončení, signály a SSH/tmux jsou ověřené.
- Demo zatím simuluje jen vytvoření DB/účtu a KILL QUERY; jiné zápisy odmítá.
- Hesla nejsou zapamatována v systémovém úložišti. Automatické přihlášení neexistuje.
- Změna hesla podporuje pouze ověřený samostatný mysql_native_password;
  vícenásobné nebo neznámé mechanismy odmítá. Bez práva číst autentizační metadata
  se operace neprovede.
- Mezi opakovanou kontrolou cíle a KILL QUERY zůstává krátké neatomické časové okno;
  server neposkytuje podmíněné KILL se srovnáním celého cíle.
- Vzdálené SSH tunely, zálohování, editor SQL a systémová služba jsou mimo zadání.

Vývojové vydání 0.1.0, revize balení 2, má všechny čtyři instalační balíčky,
kontrolní součty, metadata sestavení a zaznamenané testy instalace, provozu,
aktualizace, odstranění, SSH a tmux pro oba cílové systémy. Soubory jsou v
`dist/ubuntu-24.04` a `dist/almalinux-9.8`; strojový přehled je `dist/TEST-MATRIX.json`.
Etapy 1–4 se ještě jako celek nepovažují za plně ověřené: zbývá zejména kontrola
skutečných desktopových relací, vizuální shody, HiDPI a ručních databázových postupů
v obou UI. Kontejnerový Xvfb test tyto kontroly nenahrazuje.
