# Run access mechanism in k3s

## Pre-requirements 

Required:
- make - to run the Makefile 

Note: `make` is usually pre-installed on macOS (part of Xcode Command Line Tools). If not, run `xcode-select --install`

Nice to have (optional):
- k9s - for easier cluster management

## Quick start

```
make
```
or in separate targets
```
make start-k3s
make install-backends
make deploy-hsm-worker
make deploy-bff
```

Endpoints
* http://grafana.dev.local
  Get admin password: `kubectl --namespace monitoring get secrets kube-prometheus-stack-grafana -o jsonpath="{.data.admin-password}" | base64 -d ; echo`
* http://kafbat.dev.local
* http://headlamp.dev.local
* http://bff-rest-api.dev.local

## Backends services

Deploy monitoring, kafka and valkey.

* monitoring with Loki Grafana
* strimzi kafka operator
* deploy a kafka cluster
* deploy kafbat ui
* deploy a valkey cluster
* deploy headlamp

```
make install-backends
```

## Build and deploy hsm-worker


```
make deploy-hsm-worker
```

## Build and deploy "BFF" REST API


```
make deploy-bff
```