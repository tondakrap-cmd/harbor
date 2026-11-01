# Změny

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
