.PHONY: dev test lint migrate

dev:
	docker compose up --build

test:
	cargo test --all

lint:
	cargo fmt -- --check
	cargo clippy --all-targets -- -D warnings

migrate:
	cd apps/api && sqlx migrate run
