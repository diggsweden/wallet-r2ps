<!--
SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government

SPDX-License-Identifier: EUPL-1.2
-->

# Routing av svar från hsm-worker till rätt BFF-instans

Status: **BESLUTAD**

## Kontext / Problem

BFF:en körs som flera instanser bakom en lastbalanserare. En klient skickar en HTTP-förfrågan till valfri BFF-instans, som vidarebefordrar den till hsm-worker via Kafka. Klienten blockerar och väntar på svar på samma HTTP-anslutning. Svaret från hsm-worker måste därför levereras tillbaka till exakt den BFF-instans som håller klientens öppna HTTP-socket – det är bara den instansen som kan slutföra HTTP-svaret till klienten.

Det finns två separata svarflöden som behandlas oberoende av varandra: reguljära HSM-operationer (`hsm-worker-responses`) och tillståndsinitiering (`state-init-responses`). Separationen motiveras av att state-init kan utföras av en annan uppsättning HSM:er och hsm-worker-instanser än de övriga operationerna.

## Beslut

Varje BFF-instans får vid uppstart ett eget par Kafka-svarstopics tilldelade via miljövariablerna `HSM_WORKER_RESPONSE_TOPIC` och `STATE_INIT_RESPONSE_TOPIC`. Topics skapas av plattformen (i lokal compose-miljö av `init-kafka`, i produktion av plattformsteamet) och ingår i en pool av förutbestämda topics. BFF-instansen skapar och raderar aldrig topics själv.

Varje utgående Kafka-förfrågan bär med sig `response_topic`-fältet, som anger instansens egna svarstopic. hsm-worker skickar svaret dit. Korrelation sker via in-memory oneshot-kanaler i `ResponseService` respektive `StateInitCorrelationService` – ingen delad extern lagring används för svarskoppling.

## Motivering

Tidigare lösning använde Valkey för svarsbuffring och pending-kontextlagring: BFF:en pollerade Valkey i en loop för att hämta svaret, och pending-kontexten (state-nyckel, TTL) lagrades där i väntan på svaret från workern. Det innebar nätverksrundresor i den heta sökvägen och extra Redis-beroenden.

Följande alternativ övervägdes och avvisades:

- **Delad svarstopic med korrelations-ID** – alla BFF-instanser konsumerar alla svar och ignorerar de som inte tillhör dem. Enkelt men skapar N-faldig läsförstärkning på Kafka där N är antalet BFF-instanser. Vid 1 miljon användare med 10 förfrågningar per dag, koncentrerat till ~16 aktiva timmar, uppstår ett snitt på ~175 req/s och ett peak på ~350 req/s (med ~2× toppfaktor). Med 5 BFF-instanser innebär det 5 × 350 = 1 750 Kafka-läsningar per sekund för 350 faktiska svar – 5× amplifiering. Med 10 instanser 10× amplifiering. Skalning mot 10 miljoner användare multiplicerar dessa tal med 10. Per-instanstopics eliminerar amplifieringen helt: varje pod läser enbart sina egna svar.

- **Valkey för svarsbuffring** – bevarar cross-instans-tillgänglighet för GET-polling, men kräver polling-loop och lägger svarsfördröjning på nätverket. Behålls *enbart* för enhetstillstånd (`device-state`), inte för svarskoppling.

- **Sticky sessions i lastbalanseraren** – hade löst cross-instans-routing men är ett infrastrukturberoende som gör lastbalansering svårare att konfigurera och sämre vid instansomstarter.

- **BFF skapar och raderar egna topics dynamiskt** – eliminerar behovet av extern förabsättning men kräver Kafka-adminrättigheter i BFF-containern, inklusive bred `Describe`-behörighet för orphan-städning. Det är en onödig privilegieökning för en webbvänd tjänst. Topic lifecycle och BFF lifecycle hålls nu isär: plattformen äger topics, BFF:en bara producerar och konsumerar.

## Felscenarier

### hsm-worker kraschar under bearbetning

Förfrågan har skickats till Kafka men inget svar återkommer. BFF:ens oneshot-kanal förblir öppen tills HTTP-timeout löper ut, varefter klienten får ett timeout-svar. Den väntande posten rensas ur minnet. Topics påverkas inte.

### BFF-instans kraschar

Klientens HTTP-anslutning bryts omedelbart. Pågående förfrågningar förloras – hsm-worker kan fortfarande slutföra bearbetningen och skicka ett svar till instansens topic, men ingen konsument lyssnar längre. Svaret ligger kvar i topic tills `retention.ms` löper ut. Klienten måste göra om sin förfrågan.

### BFF-instans startar om och återanvänder samma topic

Om en ny pod tilldelas samma topic som en kraschad föregångare kan gamla svar finnas kvar. Dessa är ofarliga – request-ID är UUID och matchas inte av den nya poddens pending-register.

## Konsekvenser av beslutet

- In-memory oneshot-kanaler ger minimal latens i svarssökvägen – ingen polling, inga nätverksrundresor för korrelation.
- Valkey används enbart för enhetstillstånd (`device-state`), inte för svarskoppling.
- BFF-containern behöver endast `Produce`- och `Consume`-rättigheter på sina tilldelade topics – inga Kafka-adminrättigheter.
- Topic lifecycle och assignment policy är plattformens ansvar, inte applikationens. Pool-storlek, namngivning och återanvändningsstrategi beslutas separat.
- **Asynkron GET-polling** (när `serve_sync = false`) kräver sticky sessions i lastbalanseraren: svar cachas in-memory på den instans som tog emot POST-förfrågan, och en poll som hamnar på en annan instans missar svaret.
- **Graceful drain saknas** vid nedstängning: förfrågningar som är under flygning när SIGTERM tas emot kan förlora sitt svar. En drain-mekanism bör implementeras (stoppa nya förfrågningar, invänta att `ResponseService::pending` och `StateInitCorrelationService::pending` töms).
- Olevererbara svar (t.ex. svar som anländer till en topic från en tidigare pod-session) loggas och kastas.
