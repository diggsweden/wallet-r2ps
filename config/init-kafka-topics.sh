#!/bin/sh

echo 'Waiting for Kafka cluster...'

/opt/kafka/bin/kafka-topics.sh --bootstrap-server kafka-1:19092 --list

/opt/kafka/bin/kafka-topics.sh --bootstrap-server kafka-1:19092,kafka-2:19092,kafka-3:19092 \
	--create --if-not-exists --topic r2ps-requests \
	--partitions 50 --replication-factor 2 \
	--config min.insync.replicas=1 \
	--config retention.ms=600000 \
	--config cleanup.policy=delete

/opt/kafka/bin/kafka-topics.sh --bootstrap-server kafka-1:19092,kafka-2:19092,kafka-3:19092 \
	--create --if-not-exists --topic r2ps-responses \
	--partitions 50 --replication-factor 2 \
	--config min.insync.replicas=1 \
	--config retention.ms=600000 \
	--config cleanup.policy=delete

/opt/kafka/bin/kafka-topics.sh --bootstrap-server kafka-1:19092,kafka-2:19092,kafka-3:19092 \
	--create --if-not-exists --topic state-versions \
	--partitions 10 --replication-factor 3 \
	--config min.insync.replicas=2 \
	--config cleanup.policy=delete

/opt/kafka/bin/kafka-topics.sh --bootstrap-server kafka-1:19092,kafka-2:19092,kafka-3:19092 \
	--create --if-not-exists --topic state-snapshot \
	--partitions 10 --replication-factor 3 \
	--config min.insync.replicas=2 \
	--config cleanup.policy=compact

echo 'Topics:'
/opt/kafka/bin/kafka-topics.sh --bootstrap-server kafka-1:19092 --list

echo 'Topic details:'
/opt/kafka/bin/kafka-topics.sh --bootstrap-server kafka-1:19092 --describe --topic r2ps-requests
/opt/kafka/bin/kafka-topics.sh --bootstrap-server kafka-1:19092 --describe --topic r2ps-responses
/opt/kafka/bin/kafka-topics.sh --bootstrap-server kafka-1:19092 --describe --topic state-versions
/opt/kafka/bin/kafka-topics.sh --bootstrap-server kafka-1:19092 --describe --topic state-snapshot
