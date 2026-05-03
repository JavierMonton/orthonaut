# Ortobot Project Guide

## What this project is

Ortobot is a self-hosted full-stack app that checks Spanish orthography on Wikipedia articles.

You give the app a URL, and it:

1. Fetches article HTML from Wikipedia
2. Extracts clean text from the article body
3. Splits text into words
4. Checks each word against a Hunspell Spanish dictionary
5. Stores pages with detected issues in SQLite
6. Shows results in the frontend with delete support

The app is designed to run locally with:

- Rust backend
- React frontend (Headless UI + Tailwind)
- SQLite database

---

## High-level architecture

Frontend (`localhost:5173`) talks to Backend (`localhost:3000`) through `/api/*` endpoints.

Backend components:

- `wikipedia.rs`: fetches HTML + metadata (title, revision)
- `extractor.rs`: converts HTML into normalized word tokens
- `checker.rs`: validates words with `zspell` + in-memory cache
- `db.rs`: persists results into SQLite
- `api.rs`: request handling and orchestration
- `reporter.rs`: shapes API responses
- `main.rs`: server bootstrapping + routing + CORS

Data flow:

1. User submits URL in frontend
2. Frontend calls `POST /api/check`
3. Backend normalizes URL (supports both `/wiki/...` and REST HTML URLs)
4. Backend fetches article HTML
5. Backend extracts/checks words
6. If errors exist, backend stores row in DB and returns it
7. Frontend refreshes list state

---

## Directory structure

```text
ortobot/
├── backend/
│   ├── Cargo.toml
│   ├── dictionaries/
│   │   ├── es_ES.aff
│   │   └── es_ES.dic
│   └── src/
│       ├── main.rs
│       ├── api.rs
│       ├── wikipedia.rs
│       ├── extractor.rs
│       ├── checker.rs
│       ├── db.rs
│       └── reporter.rs
├── frontend/
│   ├── package.json
│   ├── vite.config.ts
│   ├── tailwind.config.js
│   ├── postcss.config.js
│   └── src/
│       ├── main.tsx
│       ├── App.tsx
│       ├── api.ts
│       ├── types.ts
│       └── components/
│           ├── CheckForm.tsx
│           ├── LoadingSpinner.tsx
│           └── ResultRow.tsx
├── setup.sh
├── Makefile
├── README.md
└── claude.md
```

---

## Backend details

## API endpoints

- `POST /api/check`
  - Body: `{ "url": "..." }`
  - Accepts:
    - Wikipedia page URL (example: `https://es.wikipedia.org/wiki/Madrid`)
    - Wikipedia REST HTML URL (example: `https://es.wikipedia.org/api/rest_v1/page/html/Madrid`)
  - Behavior:
    - Converts `/wiki/...` URLs to REST HTML fetch URLs internally
    - If REST fetch returns `403`, retries with canonical `/wiki/...` URL
  - Responses:
    - `status: "ok"` when no spelling issues were found
    - `status: "errors"` with row payload when issues were found and stored

- `GET /api/results`
  - Returns all saved rows from database (newest first)

- `DELETE /api/results/:id`
  - Deletes a saved row

## Database schema

SQLite file is `backend/ortobot.db` by default.

Table: `articles`

- `id` (INTEGER PRIMARY KEY AUTOINCREMENT)
- `page_title` (TEXT)
- `page_url` (TEXT)
- `revision_id` (TEXT)
- `wrong_words` (TEXT, JSON array)
- `checked_at` (TEXT, ISO 8601)

## Dictionary

- Dictionary files are expected in `backend/dictionaries/`
- `setup.sh` downloads `es_ES.aff` and `es_ES.dic`
- `checker.rs` supports UTF-8 and Latin-1 dictionary file decoding

---

## Frontend details

Main behaviors in `App.tsx`:

- Loads persisted rows on startup (`GET /api/results`)
- Submits checks (`POST /api/check`)
- Shows loading spinner while checking
- Shows success message when no errors are found
- Prepends new error rows when found
- Deletes rows (`DELETE /api/results/:id`)

UI components:

- `CheckForm.tsx`: URL input + start button
- `LoadingSpinner.tsx`: busy overlay
- `ResultRow.tsx`: row with title link, wrong words list, delete button

Vite proxy:

- `/api/*` is proxied to backend `http://localhost:3000`

---

## Running the project

Recommended one-terminal flow:

```bash
make setup
make dev
```

Useful targets:

- `make backend` (backend only)
- `make frontend` (frontend only)
- `make dev` (both, one terminal)

Manual flow:

```bash
./setup.sh
cd backend && cargo run
cd frontend && npm install && npm run dev
```

---

## Notes and current limitations

- `cargo test` currently includes URL normalization tests and passes.
- Spell-check quality depends on extraction heuristics and dictionary coverage.
- Proper nouns, scientific names, and rare terms may be flagged depending on dictionary data.
- Current implementation focuses on per-page checks (stream mode is not implemented yet).
