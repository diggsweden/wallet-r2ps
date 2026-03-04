SHELL := /bin/bash

OS := $(shell uname -s)

# On Linux, k3s uses its own embedded containerd socket separate from the system containerd.
# Point nerdctl at the k3s socket so built images are available to k3s directly.
# On macOS with Lima, use limactl shell to execute nerdctl inside the VM.
ifeq ($(OS), Linux)
NERDCTL := sudo nerdctl --address /run/k3s/containerd/containerd.sock
else
NERDCTL := limactl shell k3s sudo nerdctl --address /run/k3s/containerd/containerd.sock
endif

default: check-prerequisites start-k3s install-backends deploy-hsm-worker deploy-bff deploy-headlamp

check-prerequisites:
	@echo "Checking prerequisites..."
	@MISSING=""; \
	if ! command -v jq &> /dev/null; then \
		echo "❌ jq not found"; \
		MISSING="$$MISSING jq"; \
	else \
		echo "✓ jq found: $$(jq --version)"; \
	fi; \
	if ! command -v newuidmap &> /dev/null; then \
		if [ "$(OS)" = "Linux" ]; then \
			echo "❌ newuidmap not found"; \
			MISSING="$$MISSING newuidmap"; \
		else \
			echo "✓ newuidmap found: $$(newuidmap --version)"; \
		fi; \
	fi; \
	if ! command -v kubectl &> /dev/null; then \
		if [ "$(OS)" = "Linux" ] && command -v k3s &> /dev/null; then \
			echo "⚠ kubectl not found, but k3s is available (kubectl symlink will be created)"; \
		else \
			echo "❌ kubectl not found"; \
			MISSING="$$MISSING kubectl"; \
		fi; \
	else \
		echo "✓ kubectl found: $$(kubectl version --client --short 2>/dev/null || kubectl version --client 2>&1 | head -n1)"; \
	fi; \
	if ! command -v helm &> /dev/null; then \
		echo "❌ helm not found"; \
		MISSING="$$MISSING helm"; \
	else \
		echo "✓ helm found: $$(helm version --short 2>/dev/null || echo $$(helm version 2>&1 | head -n1))"; \
	fi; \
	if [ "$(OS)" = "Darwin" ] && ! command -v limactl &> /dev/null; then \
		echo "❌ limactl not found (required on OSX)"; \
		MISSING="$$MISSING limactl"; \
	elif [ "$(OS)" = "Darwin" ]; then \
		echo "✓ limactl found: $$(limactl --version 2>&1 | head -n1)"; \
	fi; \
	if ! command -v nerdctl &> /dev/null; then \
		echo "❌ nerdctl not found"; \
		MISSING="$$MISSING nerdctl"; \
	else \
		echo "✓ nerdctl found: $$(nerdctl version 2>&1 | grep Version | head -n1)"; \
	fi; \
	if [ "$(OS)" = "Linux" ] && ! command -v buildkitd &> /dev/null; then \
		echo "❌ buildkitd not found (required on Linux)"; \
		MISSING="$$MISSING buildkitd"; \
	elif [ "$(OS)" = "Linux" ]; then \
		echo "✓ buildkitd found: $$(buildkitd --version 2>&1 | head -n1)"; \
	fi; \
	if [ -n "$$MISSING" ]; then \
		echo ""; \
		echo "Missing required tools:$$MISSING"; \
		echo ""; \
		echo "Installation instructions:"; \
		if [[ "$$MISSING" == *"newuidmap"* ]]; then \
			echo ""; \
			echo "newuidmap:"; \
			if [ "$(OS)" = "Linux" ]; then \
				echo "  sudo apt-get update && sudo apt install -y uidmap"; \
			fi; \
		fi; \
		if [[ "$$MISSING" == *"jq"* ]]; then \
			echo ""; \
			echo "jq:"; \
			if [ "$(OS)" = "Darwin" ]; then \
				echo "  brew install jq"; \
			else \
				echo "  sudo apt-get update && sudo apt-get install -y jq"; \
			fi; \
		fi; \
		if [[ "$$MISSING" == *"kubectl"* ]]; then \
			echo ""; \
			echo "kubectl:"; \
			if [ "$(OS)" = "Darwin" ]; then \
				echo "  brew install kubectl"; \
			else \
				echo "  curl -LO \"https://dl.k8s.io/release/$(curl -L -s https://dl.k8s.io/release/stable.txt)/bin/linux/amd64/kubectl\""; \
				echo "  curl -LO \"https://dl.k8s.io/release/$(curl -L -s https://dl.k8s.io/release/stable.txt)/bin/linux/amd64/kubectl.sha256\""; \
				echo "  echo \"\$(cat kubectl.sha256)  kubectl\" | sha256sum --check"; \
				echo "  sudo install -o root -g root -m 0755 kubectl /usr/local/bin/kubectl"; \
			fi; \
		fi; \
		if [[ "$$MISSING" == *"helm"* ]]; then \
			echo ""; \
			echo "helm:"; \
			if [ "$(OS)" = "Darwin" ]; then \
				echo "  brew install helm"; \
			else \
				echo "  curl https://raw.githubusercontent.com/helm/helm/main/scripts/get-helm-3 | bash"; \
			fi; \
		fi; \
		if [[ "$$MISSING" == *"limactl"* ]]; then \
			echo ""; \
			echo "limactl:"; \
			echo "  brew install lima"; \
		fi; \
		if [[ "$$MISSING" == *"nerdctl"* ]]; then \
			echo ""; \
			echo "nerdctl:"; \
			if [ "$(OS)" = "Darwin" ]; then \
				echo "  brew install nerdctl"; \
			else \
				echo "  NERDCTL_VERSION=$(curl -s https://api.github.com/repos/containerd/nerdctl/releases/latest | grep tag_name | cut -d '"' -f 4 | sed 's/v//')"; \
				echo "  wget https://github.com/containerd/nerdctl/releases/download/v\${NERDCTL_VERSION}/nerdctl-full-\${NERDCTL_VERSION}-linux-amd64.tar.gz"; \
				echo "  sudo tar Cxzvf /usr/local nerdctl-full-\${NERDCTL_VERSION}-linux-amd64.tar.gz"; \
			fi; \
		fi; \
		if [[ "$$MISSING" == *"buildkitd"* ]]; then \
			echo ""; \
			echo "buildkitd:"; \
			echo "  BUILDKIT_VERSION=\$$(curl -s https://api.github.com/repos/moby/buildkit/releases/latest | grep tag_name | cut -d '\"' -f 4 | sed 's/v//')"; \
			echo "  wget https://github.com/moby/buildkit/releases/download/v\$${BUILDKIT_VERSION}/buildkit-v\$${BUILDKIT_VERSION}.linux-amd64.tar.gz"; \
			echo "  sudo tar Cxzvf /usr/local buildkit-v\$${BUILDKIT_VERSION}.linux-amd64.tar.gz"; \
			echo "  # Note: buildkitd will be started automatically by 'make start-k3s'"; \
		fi; \
		echo ""; \
		exit 1; \
	fi
	@echo "✓ All prerequisites satisfied"

start-k3s:
ifeq ($(OS), Darwin)
	@echo "Starting Lima VM with k3s (OSX)"
	@if ! limactl list | grep -q "^k3s"; then \
		echo "Creating and starting Lima VM..."; \
		limactl start --name=k3s --tty=false lima-k3s.yaml; \
	elif ! limactl list | grep "^k3s" | grep -q "Running"; then \
		echo "Starting existing Lima VM..."; \
		limactl start k3s; \
	else \
		echo "Lima VM 'k3s' is already running"; \
	fi
	@echo "Waiting for k3s to be ready..."
	@sleep 5
	@limactl shell k3s sudo k3s kubectl wait --for=condition=Ready nodes --all --timeout=60s 2>/dev/null || true
	@echo "Configuring kubectl access..."
	@mkdir -p ~/.kube
	@limactl shell k3s sudo cat /etc/rancher/k3s/k3s.yaml | sed 's/127.0.0.1/localhost/g' > ~/.kube/config
	@chmod 600 ~/.kube/config
	@echo ""
	@echo "✓ k3s started in Lima VM successfully"
	@echo ""
	@echo "Add the following to your /etc/hosts file:"
	@echo "127.0.0.1 grafana.dev.local kafbat.dev.local bff-rest-api.dev.local headlamp.dev.local"
	@echo ""
else ifeq ($(OS), Linux)
	@echo "Starting k3s (Linux/Ubuntu)"
	@if [ ! -f /usr/local/bin/k3s ]; then \
		echo "k3s not found, installing..."; \
		curl -sfL https://get.k3s.io | sh -s - --write-kubeconfig-mode 644; \
	fi
	@echo "Starting k3s service..."
	@sudo systemctl enable k3s --now || sudo service k3s start
	@echo "Waiting for k3s to be ready..."
	@sudo k3s kubectl wait --for=condition=Ready nodes --all --timeout=60s || true
	@mkdir -p ~/.kube
	@sudo cp /etc/rancher/k3s/k3s.yaml ~/.kube/config 2>/dev/null || true
	@sudo chown $$(id -u):$$(id -g) ~/.kube/config 2>/dev/null || true
	@if ! command -v kubectl &> /dev/null; then \
		echo "Creating kubectl symlink..."; \
		sudo ln -sf /usr/local/bin/k3s /usr/local/bin/kubectl; \
	fi
	@echo "k3s started successfully"
	@echo "Kubeconfig: ~/.kube/config"
	@echo "Starting buildkitd against k3s containerd..."
	@if ! pgrep -x buildkitd > /dev/null 2>&1; then \
		sudo buildkitd \
			--containerd-worker-addr /run/k3s/containerd/containerd.sock \
			--addr unix:///run/buildkit/buildkitd.sock \
			>/var/log/buildkitd.log 2>&1 & \
		echo "Waiting for buildkitd..."; \
		sleep 3; \
		echo "buildkitd started (logs: /var/log/buildkitd.log)"; \
	else \
		echo "buildkitd already running"; \
	fi
else
	@echo "Unsupported OS: $(OS)"
	@echo "Supported OS: Darwin (macOS), Linux (Ubuntu/Debian)"
endif

install-traefik:
	@if helm status traefik -n kube-system &>/dev/null; then \
		echo "Traefik already installed via Helm, skipping..."; \
	elif kubectl get deployment traefik -n kube-system &>/dev/null 2>&1; then \
		echo "Traefik already installed (k3s built-in), skipping..."; \
	else \
		helm repo add traefik https://traefik.github.io/charts; \
		helm repo update; \
		helm upgrade --install traefik traefik/traefik \
			--namespace kube-system \
			--set ingressClass.enabled=true \
			--set ingressClass.isDefaultClass=true \
			--set service.type=LoadBalancer \
			--wait; \
	fi

install-sealed-secrets: 
	helm repo add sealed-secrets https://bitnami-labs.github.io/sealed-secrets
	helm repo update

	helm upgrade --install sealed-secrets sealed-secrets/sealed-secrets \
	-n kube-system
	
install-strimzi: install-loki-grafana-promtail
	@echo "Installing strimzi kafka operator..."
	kubectl apply -f ./strimzi/kafka-namespace.yaml
	helm repo add strimzi https://strimzi.io/charts/ 
	helm upgrade --install strimzi-kafka-operator strimzi/strimzi-kafka-operator \
  		--namespace kafka \
  		--set watchAnyNamespace=true
	@echo "Strimzi installed"
	
install-loki-grafana-promtail: install-traefik
	helm repo add grafana https://grafana.github.io/helm-charts
	helm repo add prometheus-community https://prometheus-community.github.io/helm-charts
	helm repo update

	kubectl apply -f ./monitoring/monitoring-namespace.yaml
	
	helm upgrade --install kube-prometheus-stack prometheus-community/kube-prometheus-stack \
		--namespace monitoring \
		--values ./monitoring/kube-prometheus-values.yaml \
		--wait
	helm upgrade --install loki grafana/loki \
		--namespace monitoring \
		--values ./monitoring/loki-values.yaml \
		--wait
	helm upgrade --install promtail grafana/promtail \
		--namespace monitoring \
		--values ./monitoring/promtail-values.yaml \
		--wait
	
	kubectl apply -f ./monitoring/kafka-servicemonitor.yaml
	kubectl apply -f ./monitoring/kafka-podmonitors.yaml
	
	@echo "Deploying Grafana dashboards..."
	kubectl apply -f ./monitoring/grafana-dashboards.yaml
	
deploy-kafka-cluster: install-strimzi
	@echo "Deploy kafka cluster"
	kubectl apply -f ./access-kafka-cluster/kafka-deployment.yaml

deploy-kaf-bat: deploy-kafka-cluster
	@echo "Deploy KafBat"
	kubectl apply -f ./kafbat/kafbat-ui-helm-values.yaml
	helm repo add kafbat-ui https://kafbat.github.io/helm-charts
	helm upgrade --install kafbat-ui kafbat-ui/kafka-ui --set existingConfigMap="kafbat-ui-helm-values"
	kubectl apply -f ./kafbat/kafbat-ingress.yaml


deploy-valkey: install-loki-grafana-promtail
	@echo "Deploy Valkey"
	helm repo add bitnami https://charts.bitnami.com/bitnami
	helm upgrade --install valkey bitnami/valkey \
		--namespace default --create-namespace \
		--values ./monitoring/valkey-values.yaml \
		--wait
	@echo "Adding Prometheus discovery label to ServiceMonitor..."
	kubectl label servicemonitor valkey -n monitoring release=kube-prometheus-stack --overwrite
	@echo "Restarting Prometheus operator to pick up ServiceMonitor..."
	kubectl delete pod -n monitoring -l app.kubernetes.io/name=kube-prometheus-stack-operator

install-backends: deploy-kaf-bat deploy-valkey deploy-headlamp

deploy-hsm-worker: 
	@echo "Deploy HSM Worker"
	cd  ../rust-r2ps-worker && $(NERDCTL) --namespace k8s.io build -t hsm-worker:dev .
	kubectl apply -f ./hsm-worker/hsm-worker-deployment.yaml

deploy-bff: deploy-hsm-worker
	@echo "Deploy BFF REST API"
	cd .. && $(NERDCTL) --namespace k8s.io build -t bff-rest-api:dev -f r2ps-rest-api/Containerfile  .
	kubectl apply -f ./rest-api/rest-api.yaml

deploy-headlamp: install-traefik
	@echo "Deploy Headlamp Kubernetes UI"
	kubectl apply -f ./headlamp/headlamp.yaml

apply:
	kubectl apply -f ./monitoring/kube-prometheus-values.yaml

stop-k3s:
ifeq ($(OS), Darwin)
	@echo "Stopping Lima k3s VM..."
	@if limactl list | grep -q "^k3s"; then \
		limactl stop k3s; \
		echo "✓ Lima k3s VM stopped"; \
	else \
		echo "Lima k3s VM does not exist"; \
	fi
else ifeq ($(OS), Linux)
	@echo "Stopping k3s service..."
	@sudo systemctl stop k3s || sudo service k3s stop
	@echo "✓ k3s service stopped"
else
	@echo "Unsupported OS: $(OS)"
endif

clean-k3s:
ifeq ($(OS), Darwin)
	@echo "Deleting Lima k3s VM..."
	@if limactl list | grep -q "^k3s"; then \
		limactl delete k3s; \
		echo "✓ Lima k3s VM deleted"; \
	else \
		echo "Lima k3s VM does not exist"; \
	fi
	@echo "Cleaning up kubeconfig..."
	@rm -f ~/.kube/config
else ifeq ($(OS), Linux)
	@echo "Uninstalling k3s..."
	@if [ -f /usr/local/bin/k3s-uninstall.sh ]; then \
		sudo /usr/local/bin/k3s-uninstall.sh; \
		echo "✓ k3s uninstalled"; \
	else \
		echo "k3s not installed"; \
	fi
else
	@echo "Unsupported OS: $(OS)"
endif

# Display this help message
help:
	@echo "Available targets:"
	@grep -E '^[a-zA-Z0-9_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2}'
