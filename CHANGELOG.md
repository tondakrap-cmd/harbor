# Změny

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
