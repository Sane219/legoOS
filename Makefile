.PHONY: setup dev down logs test lint migrate

# Full onboarding for a fresh clone: creates local env files if they don't already
# exist, builds and starts the whole stack (Postgres + api + web) in the background,
# and waits for the API to come up healthy. Safe to re-run on an existing clone.
setup:
	@command -v docker >/dev/null 2>&1 || { echo "Docker is required: https://docs.docker.com/get-docker/"; exit 1; }
	@test -f apps/api/.env || cp apps/api/.env.example apps/api/.env
	@test -f apps/web/.env.local || cp apps/web/.env.example apps/web/.env.local
	docker compose up --build -d
	@echo "Waiting for the API to become healthy..."
	@for i in $$(seq 1 60); do \
		curl -sf http://localhost:8080/health >/dev/null 2>&1 && break; \
		sleep 2; \
	done
	@if curl -sf http://localhost:8080/health >/dev/null 2>&1; then \
		echo "legoOS is up:"; \
		echo "  Frontend: http://localhost:3000"; \
		echo "  API:      http://localhost:8080"; \
		echo ""; \
		echo "Run 'make logs' to follow logs, 'make down' to stop."; \
	else \
		echo "API did not become healthy in time. Run 'make logs' to investigate."; \
		exit 1; \
	fi

dev:
	docker compose up --build

down:
	docker compose down

logs:
	docker compose logs -f

test:
	cargo test --all

lint:
	cargo fmt -- --check
	cargo clippy --all-targets -- -D warnings

migrate:
	cd apps/api && sqlx migrate run
