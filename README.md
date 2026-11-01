# Harbor

Správce MariaDB v Rustu: desktop `harbor` (egui/eframe) a samostatný terminálový
`harbor-tui` (Ratatui/Crossterm). Oba klienti používají `harbor-core`; SQL je pouze
v jádře. Stav vývoje a skutečné výsledky kontrol uvádí [docs/VALIDATION.md](docs/VALIDATION.md).

## Spuštění ze zdrojů

Rust 1.88.0, Linux x86_64. Připnuté závislosti jsou v Cargo.toml a Cargo.lock.
Pro desktop jsou potřeba vývojové knihovny X11/Wayland, xkbcommon, EGL a OpenGL.
Za běhu potřebuje také `libxkbcommon-x11-0` na Ubuntu, respektive
`libxkbcommon-x11` na AlmaLinux; desktopové balíčky ji od revize 2 vyžadují automaticky.
TUI vyžaduje pouze Rust/C linker a při běhu interaktivní UTF-8 terminál.

```bash
cargo build --locked --workspace
cargo run --locked -p harbor-desktop -- --demo
cargo run --locked -p harbor-tui -- --demo
```

Bez `--demo` se klient automaticky nepřipojuje k žádnému serveru. V Připojení /
Profilech zadejte **testovací** server. TCP standardně ověřuje TLS certifikát i
identitu, volitelně s vlastní CA. Nešifrované TCP musí být výslovně zvolené jako
testovací. Lokální Unix socket TLS nepoužívá. Hesla jsou pouze v paměti procesu.
Zapamatování hesla přes systémové úložiště zatím není implementované.

Desktop nabízí Original (výchozí), Terra, Clarity a Nocturne. Terminál přidává
výchozí Turbo Classic. Změna motivu nemění spojení ani formuláře. Desktop používá
přibalená otevřená písma Liberation Sans/Serif/Mono, včetně licence; Terra má patkové
nadpisy. Fonty jsou náhradou za Inter/Merriweather z referencí. FigCraft není
potřeba při sestavení ani při běhu.

V Ubuntu 24.04 se předpokládá i běh pod LXQt: aplikace používá X11/Wayland a nemá
závislost na konkrétním desktopu. Běh přímo v relaci LXQt zatím není ověřený;
sestavení DEB v Ubuntu samo o sobě tuto kontrolu nenahrazuje.

## TUI

F10 (nebo F9 či Alt+M, pokud F10 zachytí terminál) otevře nabídku všech akcí. F1 zobrazí nápovědu, F3 profily a F5
obnoví data. Klávesy 1–9 a 0 přepínají obrazovky (Server, Databáze, Uživatelé, Spojení,
Proměnné, Doktor, Služba, Zálohy, Profily, Nastavení; klávesa 0 = Nastavení). Šipky a PageUp/PageDown posouvají seznam, Enter otevírá detail.
Tab přepíná seznam a detail; v detailu šipky posouvají dlouhý text, na obrazovce
Databáze vybírají tabulku a Enter zobrazí její strukturu. V editoru oprávnění
mezerník přepíná zaškrtnutí a ←/→ střídá existující rozsahy. Vlevo/vpravo
posouvá široký seznam. Ve formuláři Tab/Shift+Tab střídá pole a tlačítka.
Esc ponechá formulář rozpracovaný; otevřete ho přes nabídku. Ctrl+C ukončuje.

```bash
harbor-tui --colors=16 --ascii
harbor-tui --colors=mono
ssh -t test-server harbor-tui
```

Barvy lze měnit i v nastavení. `auto` respektuje NO_COLOR a COLORTERM; jinak volí
16 barev. Monochromatický režim používá inverzi a značku výběru. Pod 80×24 zůstává
možnost Ctrl+C. Terminál se obnovuje guardem, panic hookem a při SIGINT/TERM/HUP.

## Data a změny

Profily a samostatná volba motivu každého klienta: `$XDG_CONFIG_HOME/harbor/settings.json`,
jinak `~/.config/harbor/settings.json`. Zápis je atomický a zamčený s načtením aktuální
konfigurace. Žádný profil neobsahuje heslo. Připojení mají pool nejvýše jedné relace;
pracovní vlákno s Tokio přijímá typované zprávy, má časové limity a rušení při změně
serveru. Výsledky staré generace se zahazují. Obnovování se nehromadí.

QPS = rozdíl globálního `Questions` / skutečný monotónní čas; první vzorek nemá
rychlost, restart/reset začíná nový úsek. Čítač zahrnuje také monitoring. Graf drží
nejvýše 120 skutečně odebraných vzorků; historie před připojením se nevymýšlí.
Velikosti a počty řádků jsou odhady z information_schema. OS se neodhaduje.
Při chybě se data označí jako neaktuální. Práva omezují viditelnost dat.

### Správa (od verze 0.2)

- **Databáze:** velikost dat, indexů a nevyužitého místa; tabulky s enginem, odhadem
  řádků, kolací a upozorněními (bez primárního klíče, MyISAM, jiná kolace, fragmentace);
  počty pohledů, procedur, triggerů a událostí. Detail tabulky: sloupce, indexy, cizí
  klíče a `SHOW CREATE`. Údržba ANALYZE / CHECK / OPTIMIZE / REPAIR s výstupem serveru.
  Vytvoření a smazání databáze (systémové databáze smazat nelze).
- **Uživatelé a oprávnění:** oprávnění na úrovni serveru, databáze a tabulky jako
  zaškrtávací seznam (jen platná oprávnění dané úrovně, GRANT OPTION); Harbor provede
  jen rozdíl proti současnému stavu. Granty se zástupnými znaky (`shop_db.*` bez
  escapování) se zobrazují jako vzor a upravují beze změny. Šablony: jen čtení,
  aplikační, zálohovací a monitorovací účet. Role MariaDB (vytvoření, přidělení,
  výchozí role), zámek účtu, vypršení hesla, požadavek TLS a limity v jednom
  `ALTER USER`. Sloupcová oprávnění se zobrazují. Harbor odmítne smazat nebo zamknout
  účet, kterým je připojen.
- **Proměnné:** všechny systémové proměnné s původem (výchozí, konfigurace, změněno za
  běhu), výchozí hodnotou, rozsahem a popisem; přes 30 klíčových parametrů s českým
  vysvětlením. `SET GLOBAL` s kontrolou typu a rozsahu (velikosti s příponou K/M/G)
  a s ověřením, že se vlastnosti proměnné na serveru mezitím nezměnily. Změna platí do
  restartu serveru; trvalý zápis do konfigurace přinese verze s přístupem k hostiteli.
- **Doktor:** kontrola podle pravidel bez zápisu: anonymní účty, účty bez hesla,
  vzdálený root, využití spojení, neúspěšná připojení, buffer pool, dočasné tabulky na
  disku, tabulky bez PK a MyISAM, fragmentace, binlogy bez expirace, slow log, dlouhé
  transakce, TLS. Kontroly, na které účet nemá právo, uvede jako neprovedené.

### Zálohy (od verze 0.3)

- **Záloha do ZIP** vlastním logickým dumpem v Rustu přes SQL spojení: funguje i vzdáleně
  a nepotřebuje `mariadb-dump`. Tabulky InnoDB se čtou v jednom konzistentním snímku,
  netransakční (MyISAM, Aria) jsou v manifestu označené. Archiv obsahuje jeden SQL soubor
  na databázi (tabulky, data, pohledy, procedury, funkce, triggery, události, sekvence)
  a `manifest.json` s kontrolními součty SHA-256. Binární data jsou v hexadecimálním zápisu,
  generované sloupce se neukládají. Volitelné šifrování AES-256 heslem. Soubor má práva
  0600 a vzniká pod dočasným názvem; zrušená nebo neúspěšná záloha nic nezanechá.
- **Obnova** do nové databáze (kopie vedle originálu) nebo nahrazení existující (vyžaduje
  opsat název). Před obnovou se ověří kontrolní součet. Pohledy závislé na jiných pohledech
  se vytvoří ve správném pořadí. SQL soubor obnoví i samotný klient:
  `unzip -p záloha.zip shop.sql | mariadb`.
- **Ověření** archivu: kontrolní součty a úplné rozparsování SQL bez zápisu na server.
- Zálohy a obnovy běží na pozadí s vlastním spojením, ukazují průběh a lze je zrušit;
  sledování serveru mezitím pokračuje.
- **Příkazová řádka** (oba programy, bez okna): `harbor-tui backup --profile NÁZEV
  [--databases a,b] [--dir ADRESÁŘ] [--keep N] [--password-file SOUBOR]`, dále `restore`,
  `verify` a `backups`. Heslo nikdy v argumentech, jen v souboru s právy 600.
  `--keep N` maže jen nejstarší zálohy daného profilu pojmenované Harborem.
- **Plánované zálohy** jako uživatelský časovač systemd (`~/.config/systemd/user`),
  s náhledem jednotek před instalací. Pro běh bez přihlášení je třeba `loginctl enable-linger`.

### Služba, logy a konfigurace (od verze 0.4)

V profilu lze nastavit **přístup k serveru**: lokálně (Harbor běží na stejném stroji) nebo
přes **SSH** (`uživatel@hostitel`, přihlášení klíčem nebo agentem; SSH běží v dávkovém režimu
a nikdy nečeká na heslo). Administrativní příkazy získají práva přes `sudo -n` (pravidlo
NOPASSWD jen pro vybrané příkazy, návrh je v aplikaci) nebo lokálně přes `pkexec`.

- **Služba a logy:** stav jednotky systemd, PID, doba běhu, obsazení disku s daty;
  spuštění, restart, zastavení (opsat název služby); journal nebo error log s filtrem.
- **Trvalá konfigurace:** Harbor spravuje jen vlastní soubor `zz-harbor.cnf` ve skupině
  `[mariadb]` (`/etc/my.cnf.d` na AlmaLinux, `/etc/mysql/mariadb.conf.d` na Ubuntu).
  Před zápisem nový soubor ověří binárkou serveru (`mariadbd --defaults-file=… --help`),
  předchozí verzi uloží jako `.bak`. Změny se projeví po restartu.
- Všechny příkazy jsou pevné programy s argumenty; hodnoty jsou ověřené a pro SSH
  escapované pro vzdálený shell.

Každá změna má potvrzení se serverem, popisem a **přesným SQL**, které se provede
(hesla i jejich hashe jsou skryté). Smazání databáze, účtu nebo role vyžaduje opsat
název. Heslo se serveru posílá jen jako hash `mysql_native_password`. Změna hesla
odmítne neznámý nebo vícenásobný autentizační mechanismus. KILL QUERY znovu porovná
údaje o cíli a ukončuje podle QUERY_ID; mezi kontrolou a KILL zůstává krátké okno.

Demo simuluje databáze, účty, role, proměnnou max_connections, údržbu a ukončení
dotazu; ostatní zápisy výslovně odmítá. Demo hodnoty se nikdy nepoužívají jako náhrada
selhaného skutečného spojení.

## Kontroly a balíčky

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
python3 scripts/test-pty.py
bash scripts/test-instances.sh
bash scripts/test-host.sh
bash scripts/package.sh
bash scripts/smoke-packages.sh
bash scripts/test-upgrade.sh
bash scripts/test-ssh-tmux.sh
```

Změna pouze balicích metadat může na vývojovém AlmaLinux využít dříve sestavené
Ubuntu binárky: `bash scripts/build-ubuntu.sh --repackage`. Skript ověří shodu
zdrojů s původním sestavením, přebalí DEB v Ubuntu 24.04 a porovná binárky po bajtech.
Původní balíčky zachová v `previous-revision`. Při změně kódu vyžaduje úplné sestavení.

Skript test-instances vyžaduje rootless Podman a založí pouze projektové izolované
MariaDB 10.11/11.4 bez TCP. Integrační test vyžaduje explicitní cestu socketu pod
`.test-data` a kontrolní tabulku instance. Testuje čtení, chybné heslo, omezený účet,
zápisy i ukončení dotazu při zachování relace. Produkční připojení nepoužívejte.

package.sh povoluje DEB pouze na Ubuntu 24.04, RPM pouze na AlmaLinux 9. Každý systém
vytvoří samostatné `harbor` a `harbor-tui` do `dist/<distribuce-verze>/` s SHA256SUMS.
Neinstaluje MariaDB ani službu a nemění firewall nebo SELinux. Balíčky nemají
odinstalační skripty odstraňující uživatelské profily. Instalace/upgrade/odinstalace,
skutečné GUI X11/Wayland, SSH a tmux vyžadují vlastní ověření v cílových systémech.
