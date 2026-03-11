# Use podman-compose if available, fallback to docker-compose
COMPOSE := $(shell command -v podman-compose 2>/dev/null || echo docker-compose)

.PHONY: help setup build deploy dev db-up db-down logs test fmt lint clean

help: ## Show this help message
	@echo 'Usage: make [target]'
	@echo ''
	@echo 'Available targets:'
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z_-]+:.*?## / {printf "  %-15s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

setup: ## Initial setup — copy .env.example and check dependencies
	@echo "Setting up claude-container development environment..."
	@echo "Using compose: $(COMPOSE)"
	@cp -n .env.example .env || true
	@echo ""
	@echo "✓ Setup complete!"
	@echo ""
	@echo "Next steps:"
	@echo "  1. Edit .env with your configuration (DB_PASSWORD, GITHUB_TOKEN, etc.)"
	@echo "  2. Run 'make dev' to start postgres and run the server locally"
	@echo "  3. Or run 'make deploy' to build and deploy the full stack in containers"

build: ## Build the application container image
	@echo "Building claude-container image..."
	@$(COMPOSE) -f docker-compose.yml -f docker-compose.deploy.yml build app

deploy: ## Build and deploy the full stack (app + postgres) in containers
	@echo "Deploying claude-container..."
	@$(COMPOSE) -f docker-compose.yml -f docker-compose.deploy.yml up -d --build
	@echo ""
	@echo "Waiting for server to be ready..."
	@for i in $$(seq 1 30); do \
		if curl -sf http://localhost:3000/api/tasks > /dev/null 2>&1; then \
			echo "✓ Server is up at http://localhost:3000"; \
			exit 0; \
		fi; \
		sleep 2; \
	done; \
	echo "Server did not become ready in time"; \
	$(COMPOSE) -f docker-compose.yml -f docker-compose.deploy.yml logs app; \
	exit 1

dev: ## Start postgres for local development (run backend + frontend natively)
	@$(COMPOSE) up -d
	@echo ""
	@echo "Waiting for PostgreSQL to be ready..."
	@for i in $$(seq 1 30); do \
		if $(COMPOSE) exec -T db pg_isready -U claude -d claude_container > /dev/null 2>&1; then \
			echo "✓ PostgreSQL is ready"; \
			break; \
		fi; \
		if [ $$i -eq 30 ]; then \
			echo "PostgreSQL did not become ready in time"; \
			$(COMPOSE) logs db; \
			exit 1; \
		fi; \
		sleep 2; \
	done
	@echo ""
	@echo "✓ Development environment started!"
	@echo "  - PostgreSQL: localhost:5432"
	@echo "  - DATABASE_URL: postgres://claude:<password>@localhost:5432/claude_container"
	@echo ""
	@echo "To run the backend:"
	@echo "  cargo run --package backend"
	@echo ""
	@echo "To run the frontend dev server:"
	@echo "  cd crates/frontend && dx serve"

db-up: ## Start only the postgres container
	@$(COMPOSE) up -d
	@for i in $$(seq 1 30); do \
		if $(COMPOSE) exec -T db pg_isready -U claude -d claude_container > /dev/null 2>&1; then \
			echo "✓ PostgreSQL is ready on localhost:5432"; \
			exit 0; \
		fi; \
		if [ $$i -eq 30 ]; then \
			echo "PostgreSQL did not become ready in time"; \
			$(COMPOSE) logs db; \
			exit 1; \
		fi; \
		sleep 2; \
	done

db-down: ## Stop and remove the postgres container (data volume is preserved)
	@$(COMPOSE) down

logs: ## Tail logs from the deployed app container
	@$(COMPOSE) -f docker-compose.yml -f docker-compose.deploy.yml logs -f app

logs-db: ## Tail logs from the postgres container
	@$(COMPOSE) logs -f db

test: ## Run backend and shared unit tests
	@cargo test --package backend --package shared

test-all: ## Run tests + dx check for the frontend
	@cargo test --package backend --package shared
	@echo "Checking frontend (WASM)..."
	@cd crates/frontend && dx check

fmt: ## Format all Rust code
	@cargo fmt --all

lint: ## Run clippy across the workspace (backend + shared)
	@cargo clippy --package backend --package shared --all-targets -- -D warnings

clean: ## Stop all containers, remove volumes, and clean Rust build artifacts
	@$(COMPOSE) -f docker-compose.yml -f docker-compose.deploy.yml down -v
	@cargo clean
	@echo "✓ Clean complete"
