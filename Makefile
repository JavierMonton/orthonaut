SHELL := /bin/bash

REPO := https://github.com/JavierMonton/orthonaut

.PHONY: help dev toolforge-build toolforge-start toolforge-restart toolforge-stop toolforge-logs

help:
	@echo "Local:"
	@echo "  make dev               - Run backend (3000) + frontend (5173) for development"
	@echo ""
	@echo "Toolforge (run on the bastion as the orthonaut tool):"
	@echo "  make toolforge-build   - Build the container image from GitHub"
	@echo "  make toolforge-start   - Start the web service (mounts NFS, 2Gi RAM)"
	@echo "  make toolforge-restart - Restart the running web service"
	@echo "  make toolforge-stop    - Stop the web service"
	@echo "  make toolforge-logs    - Show web service logs"

# --- Local development ---
dev:
	@set -euo pipefail; \
	trap 'echo ""; echo "Stopping services..."; if [ -n "$$BACK_PID" ] && kill -0 $$BACK_PID 2>/dev/null; then kill $$BACK_PID; fi' EXIT INT TERM; \
	if [ ! -f backend/dictionaries/es_ES.aff ] || [ ! -f backend/dictionaries/es_ES.dic ]; then \
		echo "Dictionary files not found. Running ./setup.sh ..."; \
		./setup.sh; \
	fi; \
	if [ ! -d frontend/node_modules ]; then \
		echo "Installing frontend dependencies ..."; \
		(cd frontend && npm install); \
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

# --- Toolforge (run on the bastion after `become orthonaut`) ---
toolforge-build:
	toolforge build start $(REPO)

toolforge-start:
	toolforge webservice buildservice start --mount all --mem 2Gi --cpu 1

toolforge-restart:
	toolforge webservice restart

toolforge-stop:
	toolforge webservice buildservice stop

toolforge-logs:
	toolforge webservice logs
