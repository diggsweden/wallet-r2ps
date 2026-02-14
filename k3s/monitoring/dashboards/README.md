# Grafana Dashboards

This directory contains declarative Grafana dashboards for monitoring Kafka and Valkey clusters.

## Available Dashboards

### Kafka Cluster Overview (`kafka-overview.json`)

Comprehensive monitoring dashboard for Kafka clusters managed by Strimzi.

**Metrics Included:**

- **Cluster Health**
  - Active Brokers
  - Under-Replicated Partitions
  - Offline Replicas

- **Throughput & Performance**
  - Messages In Per Second
  - Network Throughput (Bytes In/Out)
  - Request Latency (p99) for Produce and Fetch operations

- **Consumer Monitoring**
  - Consumer Group Lag by topic and consumer group

- **JVM Metrics**
  - Heap Memory Usage
  - Garbage Collection frequency

**Variables:**
- `datasource`: Prometheus data source (auto-detected)
- `cluster`: Kafka cluster name (auto-populated from metrics)

### Valkey Overview (`valkey-overview.json`)

Comprehensive monitoring dashboard for Valkey/Redis instances.

**Metrics Included:**

- **Instance Health**
  - Valkey Status (up/down)
  - Connected Clients
  - Memory Usage (percentage)
  - Hit Rate (percentage)

- **Performance Metrics**
  - Commands Per Second
  - Keyspace Hits vs Misses
  - Network I/O (input/output bytes)

- **Memory & Storage**
  - Memory Usage (used vs max)
  - Keys Per Database
  - Evicted & Expired Keys

- **Operations**
  - Client Connections (total and blocked)
  - Replication Status (for Sentinel/cluster mode)

**Variables:**
- `datasource`: Prometheus data source (auto-detected)
- `instance`: Valkey service name (auto-populated from metrics)

## Deployment

The dashboards are automatically deployed as ConfigMaps when you run:

```bash
make install-loki-grafana-promtail
```

Or manually:

```bash
kubectl apply -f k3s/monitoring/grafana-dashboards.yaml
```

## How It Works

1. **ConfigMaps**: Dashboard JSON files are stored in Kubernetes ConfigMaps with the label `grafana_dashboard: "1"`
2. **Sidecar Discovery**: Grafana's sidecar container automatically discovers ConfigMaps with this label
3. **Auto-Loading**: Dashboards are automatically loaded into Grafana without manual import

## Accessing Dashboards

1. **Via Ingress**: https://grafana.dev.local (add to your hosts file)
2. **Via NodePort**: http://localhost:30080
3. **Default Credentials**: 
   - Username: `admin`
   - Password: `changeme` (change this in production!)

## Customization

To modify dashboards:

1. Edit the JSON files in this directory
2. Update the ConfigMap: `kubectl apply -f k3s/monitoring/grafana-dashboards.yaml`
3. Grafana will automatically reload the updated dashboard (may take 30-60 seconds)

Alternatively, edit in Grafana UI and export the JSON to update these files.

## Dashboard UIDs

- Kafka Overview: `kafka-overview`
- Valkey Overview: `valkey-overview`

These UIDs allow direct linking to dashboards via URL: `https://grafana.dev.local/d/<uid>`

## Metrics Sources

- **Kafka Metrics**: Collected via JMX Prometheus Exporter and Kafka Exporter
  - Label: `strimzi_io_cluster` for cluster selection
- **Valkey Metrics**: Collected via Redis Exporter (enabled in Bitnami Helm chart)
  - Label: `job` for instance selection (typically `valkey-metrics`)
  - The Bitnami chart creates a ServiceMonitor that exposes metrics on the `http-metrics` port
- **Data Source**: Prometheus (configured in kube-prometheus-stack)

### Important: Metric Labels

The Valkey dashboard uses the `job` label to select instances. When deployed with the Bitnami Valkey chart, the job name will be `valkey-metrics` by default. If you deploy with a different release name, the job will be `<release-name>-metrics`.

To verify the job name:
```bash
kubectl get servicemonitor -n default -l app.kubernetes.io/name=valkey -o yaml | grep jobLabel
```

Or check in Prometheus:
```promql
label_values(redis_up, job)
```

## Troubleshooting

### Dashboards not appearing

1. Check ConfigMap exists: `kubectl get configmap -n monitoring | grep grafana-dashboard`
2. Verify labels: `kubectl get configmap grafana-dashboard-kafka -n monitoring -o yaml | grep grafana_dashboard`
3. Check Grafana logs: `kubectl logs -n monitoring -l app.kubernetes.io/name=grafana -c grafana-sc-dashboard`

### Missing metrics

1. Verify Prometheus is scraping targets:
   - Go to Grafana > Explore > Prometheus
   - Query: `up{job=~".*kafka.*|.*valkey.*|.*redis.*"}`
2. Check PodMonitors and ServiceMonitors:
   - `kubectl get podmonitor -n kafka`
   - `kubectl get servicemonitor -n monitoring`

### Wrong cluster/instance shown

The dashboards use variables that auto-populate from available metrics. If multiple clusters exist, use the dropdown at the top of the dashboard to select the desired instance.
