SHELL := /bin/bash

.PHONY: help setup backend frontend dev build deploy-prep

help:
	@echo "Available targets:"
	@echo "  make setup        - Download dictionaries and install frontend dependencies"
	@echo "  make backend      - Run backend only (port 3000)"
	@echo "  make frontend     - Run frontend only (port 5173, with HMR)"
	@echo "  make dev          - Run backend + frontend in one terminal"
	@echo "  make build        - Build frontend for production (required before deploying)"

setup:
	./setup.sh
	cd frontend && npm install

backend:
	cd backend && cargo run

frontend:
	cd frontend && npm run dev

build:
	cd frontend && npm run build

dev:
	@set -euo pipefail; \
	trap 'echo ""; echo "Stopping services..."; if [ -n "$$BACK_PID" ] && kill -0 $$BACK_PID 2>/dev/null; then kill $$BACK_PID; fi' EXIT INT TERM; \
	if [ ! -f backend/dictionaries/es_ES.aff ] || [ ! -f backend/dictionaries/es_ES.dic ]; then \
		echo "Dictionary files not found. Running ./setup.sh ..."; \
		./setup.sh; \
	fi; \
	BACK_LOG="$$(mktemp)"; \
	echo "Starting backend on http://localhost:3000 ..."; \
	(cd backend && cargo run) > "$$BACK_LOG" 2>&1 & \
	BACK_PID=$$!; \
	for i in $$(seq 1 240); do \
		if (echo > /dev/tcp/127.0.0.1/3000) >/dev/null 2>&1; then \
			echo "Backend is ready."; \
			break; \
		fi; \
		if ! kill -0 $$BACK_PID 2>/dev/null; then \
			echo "Backend failed to start. Output:"; \
			sed -n '1,200p' "$$BACK_LOG"; \
			exit 1; \
		fi; \
		sleep 0.5; \
	done; \
	if ! (echo > /dev/tcp/127.0.0.1/3000) >/dev/null 2>&1; then \
		echo "Backend did not become ready in time. Output:"; \
		sed -n '1,200p' "$$BACK_LOG"; \
		exit 1; \
	fi; \
	echo "Starting frontend on http://localhost:5173 ..."; \
	cd frontend && npm run dev
