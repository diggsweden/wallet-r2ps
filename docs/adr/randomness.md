<!--
SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government

SPDX-License-Identifier: EUPL-1.2
-->

# Hantering av slumptalsgenerering (rand)

Status: **BESLUTAD**

## Kontext / Problem

Projektet lider av fragmentering i beroendeträdet för biblioteket `rand`. På grund av transitiva beroenden (indirekta beroenden via andra bibliotek) finns flera inkompatibla versioner av `rand` (0.8, 0.9 och 0.10) i projektet samtidigt.

Detta skapar flera problem:

1. **Trait-inkompatibilitet:** En `RngCore` från `rand 0.8` kan inte användas där `rand 0.10` förväntas, vilket gör det svårt att skriva gemensam kod.
2. **Kryptografisk hygien:** Bibliotek som `p256` och `opaque-ke` är låsta till `rand_core 0.6` (som hör till `rand 0.8`). Att använda högnivå-API:er som `thread_rng()` för känsliga nycklar innebär att man förlitar sig på en PRNG (Pseudo-Random Number Generator) i användarrymden snarare än direkta systemanrop för entropi.
3. **Onödig komplexitet:** Flera versioner av `getrandom` och olika implementationer av ChaCha-algoritmen inkluderas i den slutgiltiga binären, vilket ökar dess storlek och attackyta.

## Beslut

Vi inför en "hybridmetod" för att hantera slumpmässighet:

1. **`hsm-worker` (Kryptografisk kärna):** Vi går ifrån det generella biblioteket `rand` och använder istället `rand_core 0.6` med funktionen `getrandom` direkt. Kod som genererar nycklar ska använda `rand_core::OsRng`.
2. **`integration-load-tests` (Verktyg/Tester):** Vi använder den senaste versionen, `rand 0.10`, för att dra nytta av moderna API:er, bättre prestanda och enklare ergonomi i testlogiken.

## Motivering

Eftersom fragmentering är oundviklig så länge beroenden som `opaque-ke` (låst till 0.8) och `bollard` (låst till 0.9) används, är det bättre att vara explicit med vilken version som används var:

- **Säkerhet framför allt:** I `hsm-worker` prioriterar vi direkt tillgång till operativsystemets entropi (`OsRng`) utan onödiga abstraktionslager. Genom att använda `rand_core 0.6` direkt matchar vi exakt de traits som våra kryptobibliotek kräver, vilket eliminerar typ-konflikter vid kompilering.
- **Modernisering där det är möjligt:** I testerna är prestanda och användarvänlighet viktigare än minimala beroenden. Genom att använda `rand 0.10` här följer vi ekosystemets utveckling.
- **Framtidssäkring:** Genom att frikoppla `hsm-worker` från det publika `rand`-API:et blir det enklare att uppgradera i framtiden när de underliggande kryptobiblioteken väl flyttar till nyare versioner av `rand_core`.

## Konsekvenser av beslutet

- **Inkompatibilitet i delad kod:** Gemensamma hjälpfunktioner i `hsm-common` kan inte enkelt använda slumpmässighet som fungerar i både worker och tester utan att duplicera logik för olika trait-versioner.
- **Kognitiv belastning:** Utvecklare måste vara medvetna om att olika API:er används (`OsRng` i workern vs `rng().random()` i tester).
- **Bibehållen fragmentering:** `Cargo.lock` kommer fortsätta att innehålla flera versioner av `rand` och `getrandom` så länge de transitiva beroendena kräver det, men projektets egen kod bidrar inte längre till ytterligare spridning.
- **Prestanda:** Direkta anrop till `OsRng` i workern innebär ett systemanrop (syscall) per slumpmässigt värde, vilket är säkrare men långsammare än `thread_rng()`. Detta bedöms som en acceptabel avvägning i en HSM-kontext.
