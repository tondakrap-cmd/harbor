# Změny

## 0.6.0

- Porovnání dvou serverů: připojený server proti uloženému profilu; proměnné (bez těch,
  které se liší vždy: id, cesty, pozice GTID), účty a role po vlastnostech a oprávněních,
  schémata (databáze, tabulky, sloupce, indexy). Filtr podle oblasti, hledání, export CSV.
- Zapamatování hesla v systémovém úložišti (Secret Service: GNOME Keyring, KWallet),
  jen na požádání u uloženého profilu a jen po úspěšném přihlášení; nikdy do souboru.
  Bez úložiště (server, SSH) Harbor vysvětlí, že je třeba heslo zadat. Příkazová řádka
  použije uložené heslo, není-li zadán soubor s heslem. Zrušení volby nebo smazání
  profilu uložené heslo odstraní.
- Testy úložiště hesel v izolované relaci D-Bus (scripts/test-keyring.sh), porovnání
  instancí 10.11 a 11.4.
- CI: actions/checkout@v5.

## 0.5.0

- Aktivní spojení → Transakce a zámky: otevřené transakce InnoDB, čekání na zámky,
  kořenové blokující spojení, čekání na zámky metadat; ukončení celého spojení
  (KILL CONNECTION) s kontrolou, že číslo stále patří stejnému účtu.
- Výkon a dotazy: nejnáročnější tvary dotazů z performance_schema, z mysql.slow_log nebo
  ze souboru slow logu přes přístup k serveru; normalizace literálů, nejpomalejší vzorek,
  EXPLAIN vzorku v konzoli, zapnutí slow logu, vynulování statistik.
- Replikace: stav všech replikačních spojení (SHOW ALL SLAVES STATUS), start/stop,
  připojené repliky, GTID, Galera; binární logy s velikostí a mazáním starších
  (opsat název; Harbor upozorní, pokud server log ponechal, protože ho čte replika).
- SQL konzole: jeden příkaz na vlastním spojení; čtení v transakci READ ONLY s limitem
  1000 řádků a 60 s, zrušení, chybové zprávy serveru, historie, export do CSV (0600);
  zápisové příkazy jen po potvrzení.
- Profil „jen pro čtení“: zakáže změny, obnovy, akce služby, zápis konfigurace a zápisy v konzoli.
- Doktor: zastavená nebo zpožděná replika, stav uzlu Galera, flow control.
- Testy proti skutečnému páru primár/replika (scripts/test-replication.sh).
- TUI: obrazovky Výkon, Replikace a Konzole (klávesy [ a ]), záhlaví se posouvá.

## 0.4.0

- Přístup k serveru v profilu: lokálně nebo přes SSH (jen klíče/agent, BatchMode),
  práva správce přes sudo bez hesla nebo pkexec (lokálně). Návrh pravidla sudoers v aplikaci.
- Nová obrazovka Služba a logy: stav systemd jednotky, PID, doba běhu, spuštění po startu,
  obsazení disku s daty; spuštění, restart a zastavení (zastavení vyžaduje opsat název);
  journal nebo error log s filtrem a úrovní.
- Trvalá konfigurace: Harbor zapisuje jen vlastní soubor `zz-harbor.cnf` (skupina [mariadb])
  v adresáři konfigurace; před zápisem ho ověří binárkou serveru nanečisto, předchozí verzi
  uloží jako .bak. Proměnnou lze uložit do konfigurace přímo z obrazovky Proměnné.
- Ověřeno i proti MariaDB 10.5 (AlmaLinux 9) včetně restartu a projevení uložené hodnoty.
- TUI: obrazovka Služba (7), klávesa 0 pro Nastavení, dlouhé formuláře se posouvají.

## 0.3.0

- Zálohy do ZIP vlastním dumpem v Rustu (konzistentní snímek InnoDB, SHA-256 manifest,
  volitelné šifrování AES-256, práva 0600), obnova do nové nebo nahrazované databáze,
  ověření archivu. Archiv obnoví i klient `mariadb`.
- Úlohy na pozadí s vlastním spojením, průběhem a zrušením.
- Příkazová řádka `backup`, `restore`, `verify`, `backups` v obou programech; hesla jen
  ze souborů s právy 600; retence `--keep`.
- Plánované zálohy jako uživatelské časovače systemd s náhledem jednotek.
- Nová obrazovka Zálohy v desktopu i TUI (TUI: klávesy 1–9).
- Desktop: jednotné rozložení „seznam vlevo, detail vpravo“ s posuvným oddělovačem,
  pevným panelem filtrů a akcí nahoře a akcemi vybrané položky v záhlaví detailu;
  tabulky s pevnou hlavičkou a vlastním posuvníkem; velké formuláře v oknech;
  jednotná velikost písma tlačítek, polí i řádků; čitelný výběr řádku v motivu Original.
- TUI: nabídka F10 začíná akcemi aktuální obrazovky.

## 0.2.0

- Databáze: velikosti dat/indexů/nevyužitého místa, seznam tabulek s upozorněními,
  struktura tabulky (sloupce, indexy, cizí klíče, DDL), údržba ANALYZE/CHECK/OPTIMIZE/REPAIR,
  smazání databáze s opsáním názvu.
- Uživatelé: oprávnění na úrovni serveru, databáze a tabulky (zaškrtávání, provádí se jen
  rozdíl), šablony, role, výchozí role, zámek účtu, vypršení hesla, požadavek TLS, limity,
  smazání účtu/role.
- Nová obrazovka Proměnné: všechny systémové proměnné s původem a popisem klíčových
  parametrů, změna přes SET GLOBAL s kontrolou typu a rozsahu.
- Nová obrazovka Doktor: kontrola bezpečnosti, výkonu a údržby podle pravidel.
- Potvrzení každé změny ukazuje přesné SQL; desktop i TUI mají stejné funkce.
- TUI: klávesy 1–8 pro obrazovky, nabídka s akcemi seskupenými podle objektu.
- Desktop: čitelná navigace v bočním panelu tématu Original.

## 0.1.0

- První verze: přehled serveru, databáze, účty, aktivní spojení, profily, motivy,
  balíčky DEB a RPM.
