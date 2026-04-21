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

Varje BFF-instans skapar vid uppstart egna tillfälliga Kafka-svarstopics (`hsm-worker-responses-{instans-id}-{YYYYMMDD}` och `state-init-responses-{instans-id}-{YYYYMMDD}`) och raderar dem vid nedstängning. Instans-ID sätts via miljövariabeln `BFF_INSTANCE_ID` (typiskt pod-namn för enkel felsökning i drift).

Varje utgående Kafka-förfrågan bär med sig `response_topic`-fältet, som anger instansens egna svarstopic. hsm-worker skickar svaret dit. Korrelation sker via in-memory oneshot-kanaler i `ResponseService` respektive `StateInitCorrelationService` – ingen delad extern lagring används för svarskoppling.

Topic-namnen datumstämplas (`YYYYMMDD`). Vid uppstart skrivs ett initialt heartbeat-meddelande synkront innan HTTP-lyssnaren öppnas – servern accepterar aldrig förfrågningar utan ett bekräftat aktivt svarstopic. Därefter skriver en bakgrundsuppgift ett `__heartbeat__`-meddelande var 307 sekunder (~5 min). Topics konfigureras med `retention.ms = 661000` (~11 min). Båda värdena är primtal, vilket innebär att intervallen aldrig sammanfaller periodiskt och undviker "thundering herd"-effekter där retention-rensning och heartbeat-skrivning konsekvent träffar varandra samtidigt. Kvoten ~2.15x ger tillräcklig marginal mot att topics töms mellan två heartbeats. Vid uppstart städas orphan-topics från tidigare dagar bort om de är tomma (heartbeats har löpt ut och retention har tömt dem). Topics från innevarande dag lämnas alltid ifred.

## Motivering

Tidigare lösning använde Valkey för svarsbuffring och pending-kontextlagring: BFF:en pollerade Valkey i en loop för att hämta svaret, och pending-kontexten (state-nyckel, TTL) lagrades där i väntan på svaret från workern. Det innebar nätverksrundresor i den heta sökvägen och extra Redis-beroenden.

Följande alternativ övervägdes och avvisades:

- **Delad svarstopic med korrelations-ID** – alla BFF-instanser konsumerar alla svar och ignorerar de som inte tillhör dem. Enkelt men innebär onödig deserialisering och lastbalansering av Kafka-meddelanden på applikationsnivå; skalar dåligt med fler instanser.

- **Valkey för svarsbuffring** – bevarar cross-instans-tillgänglighet för GET-polling, men kräver polling-loop och lägger svarsfördröjning på nätverket. Behålls *enbart* för enhetstillstånd (`device-state`), inte för svarskoppling.

- **Sticky sessions i lastbalanseraren** – hade löst cross-instans-routing men är ett infrastrukturberoende som gör lastbalansering svårare att konfigurera och sämre vid instansomstarter.

## Felscenarier

### hsm-worker kraschar under bearbetning

Förfrågan har skickats till Kafka men inget svar återkommer. BFF:ens oneshot-kanal förblir öppen tills HTTP-timeout löper ut, varefter klienten får ett timeout-svar. Den väntande posten rensas ur minnet. Topics påverkas inte.

### BFF-instans kraschar

Klientens HTTP-anslutning bryts omedelbart. Pågående förfrågningar förloras – hsm-worker kan fortfarande slutföra bearbetningen och skicka ett svar till instansens topic, men ingen konsument lyssnar längre. Svaret ligger kvar i topic tills `retention.ms` (10 minuter) löper ut. Därefter är topic:n tom och betraktas som orphan av nästa pod som startar. Klienten måste göra om sin förfrågan.

### Midnattsrace: uppstart precis före midnatt

En pod startar kl. 23:59 och skapar topics med dagens datum (t.ex. `*-20260421`). Det initiala heartbeat-meddelandet skrivs synkront under uppstart, men om en annan pod startar kl. 00:01 och kör orphan-städning *innan* det initiala heartbeat-meddelandet har hunnit skrivas (liten race-window), ser den `*-20260421`-topics som tillhör "gårdagen". Eftersom inga heartbeats har skrivits ännu är topics tomma, och städningen raderar dem.

När den kvarlevande pod:en sedan försöker skriva sitt första heartbeat misslyckas det mot en icke-existerande topic. Eftersom det initiala heartbeat-meddelandet skrivs synkront under uppstart – innan HTTP-lyssnaren öppnas – innebär detta att pod:en ännu inte accepterar förfrågningar. `write_initial_heartbeat` hanterar felet: den väntar 1 sekund, återskapar topics med aktuellt datum (`*-20260422`), väntar tills topics är synliga i metadata, och försöker på nytt. Om retry lyckas fortsätter uppstarten normalt och HTTP-lyssnaren öppnas. Hela återhämtningen sker transparent utan att någon förfrågan förloras. Om retry misslyckas avslutas processen med panik och Kubernetes startar om pod:en — vid omstart är datumet redan `20260422` och någon race mot gårdagens topics kan inte uppstå igen.

### Midnattsrace: städning raderar en levande pods topics

Om en pod kraschar kl. 23:58 och dess sista heartbeat skrevs kl. 23:55 (retention löper ut 00:05), kan en pod som startar kl. 00:01 se topics som icke-tomma (meddelanden finns ännu kvar) och lämna dem ifred. Retention rensar dem vid 00:05. En nästa pod som startar efter 00:05 raderar dem korrekt. Mellanperioden (00:01–00:05) lämnar alltså orphan-topics kvar – acceptabelt givet att de försvinner inom kort.

## Konsekvenser av beslutet

- In-memory oneshot-kanaler ger minimal latens i svarssökvägen – ingen polling, inga nätverksrundresor för korrelation.
- Valkey används enbart för enhetstillstånd (`device-state`), inte för svarskoppling.
- **Asynkron GET-polling** (när `serve_sync = false`) kräver sticky sessions i lastbalanseraren: svar cachas in-memory på den instans som tog emot POST-förfrågan, och en poll som hamnar på en annan instans missar svaret.
- **Graceful drain saknas** vid nedstängning: förfrågningar som är under flygning när SIGTERM tas emot kan förlora sitt svar. En drain-mekanism bör implementeras (stoppa nya förfrågningar, invänta att `ResponseService::pending` och `StateInitCorrelationService::pending` töms, sedan radera topics).
- **Heartbeat-fel** behandlas idag som fatalt vid andra fel i rad, vilket riskerar att döda friska pods vid kortvariga Kafka-avbrott (t.ex. broker-omstart). Bör ersättas med retry med backoff i några minuter innan processen avslutas.
- Olevererbara svar (t.ex. svar som anländer till en topic från en tidigare pod-session) loggas och kastas. En mekanism för att parkera sådana svar i ett delat lager för klient-återanslutning saknas för närvarande.
- Om en pod startar om och återanvänder samma `BFF_INSTANCE_ID` och datum, återanvänds den befintliga topic-instansen. Eventuella kvarliggande meddelanden är ofarliga – request-ID är UUID och matchas inte av den nya poddens pending-register.
