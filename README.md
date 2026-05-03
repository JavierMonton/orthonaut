# Ortobot

Ortobot is a self-hosted full-stack app to check Spanish orthography on Wikipedia pages.

## Stack

- Backend: Rust (`axum`, `zspell`, `rusqlite`)
- Frontend: React + Tailwind CSS + Headless UI
- Database: SQLite

## Quick start

1. Download dictionaries:

```bash
./setup.sh
```

2. Run backend:

```bash
cd backend
cargo run
```

3. Run frontend:

```bash
cd frontend
npm install
npm run dev
```

The frontend runs on `http://localhost:5173` and proxies API calls to `http://localhost:3000`.
