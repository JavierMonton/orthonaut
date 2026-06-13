# Orthonaut — Requirements & Architecture Plan

## Purpose

A Rust tool that checks Spanish spelling (ortografía) on Wikipedia article texts.
It processes article content word by word, identifies misspelled or unknown words,
and reports orthographic issues. The tool must run fully offline, with minimal
resource usage, suitable for deployment on a small server.

---

## Functional Requirements

- Accept Spanish Wikipedia article content as input
- Extract plain text from the input (stripping markup, tags, templates, etc.)
- Check each word against a Spanish dictionary
- Report which words are unrecognized / misspelled, and in which article
- Operate completely offline — no external spell-checking API calls
- Support two input modes (see Input Modes below)

---

## Input Format: HTML (not Wikitext)

Wikipedia content can be retrieved as **Wikitext** (raw source) or **HTML** (rendered output).

**HTML is the chosen format** for the following reasons:

- Wikitext requires parsing complex syntax: templates (`{{...}}`), link syntax
  (`[[Article|display text]]`), magic words, parser functions, file references, etc.
  This is a significant parsing problem and Rust wikitext parsers are immature.
- Wikipedia's HTML is already fully rendered — templates are expanded, links resolved,
  numbers and references are in standard HTML elements that are easy to skip.
- Standard, mature HTML parsers exist in Rust (see `scraper` below).
- Unwanted content (reference numbers, infoboxes, navigation, coordinates) can be
  excluded precisely using CSS selectors.

### HTML extraction strategy

- Target only the `#mw-content-text` element (main article body)
- Skip the following elements to avoid false positives:
  - `<sup>` — reference numbers
  - `<math>` — mathematical notation
  - `<table class="infobox">` — structured info tables
  - `<span class="coordinates">` — geo coordinates
  - Navigation boxes, edit links, language links
- Extract all remaining text nodes and split into words

---

## Input Modes

The tool supports two modes, sharing the same core pipeline. Only the input layer differs.

### Mode 1: Wikipedia REST API (polling / on-demand)

Fetch a specific article by title from the Spanish Wikipedia REST API:

```
GET https://es.wikipedia.org/api/rest_v1/page/html/{title}
```

This returns Parsoid HTML — fully rendered, clean, ready for extraction.

Use cases:
- Manual inspection of a specific article
- Batch processing a list of article titles
- Development and testing

### Mode 2: Wikimedia EventStream subscriber (real-time)

Subscribe to Wikimedia's EventStreams SSE feed to receive real-time notifications
of article edits:

```
GET https://stream.wikimedia.org/v2/stream/recentchange
```

**Important:** The EventStream events contain only metadata (page title, editor,
edit summary, timestamp) — not the full article body. Upon receiving an event,
the tool must fetch the article HTML separately via the REST API (same as Mode 1).

A future stream that includes full article body content may replace the secondary
fetch when it becomes available. The architecture is designed so that only
`stream.rs` would need to change in that case.

---

## Architecture

```
[Mode 1: API poll]  ──┐
                      ├──→ wikipedia.rs (fetch HTML) ──→ extractor.rs ──→ checker.rs ──→ reporter.rs
[Mode 2: SSE stream] ─┘
```

### Module breakdown

```
orthonaut/
├── Cargo.toml
├── dictionaries/
│   ├── es_ES.aff          # Spanish Hunspell affix rules
│   └── es_ES.dic          # Spanish Hunspell word stems
└── src/
    ├── main.rs            # CLI entry point; selects operating mode from config/args
    ├── checker.rs         # zspell wrapper + per-session word-level cache
    ├── extractor.rs       # HTML → clean word list using scraper crate
    ├── wikipedia.rs       # Wikipedia REST API client; used by both modes
    ├── stream.rs          # Wikimedia EventStreams SSE subscriber (Mode 2)
    └── reporter.rs        # Formats and outputs results (article title + bad words)
```

### Data flow

1. **Input layer** (`stream.rs` or `wikipedia.rs`): obtain article title/URL
2. **Fetch** (`wikipedia.rs`): retrieve article HTML from Wikipedia REST API
3. **Extract** (`extractor.rs`): parse HTML, strip noise, return list of words
4. **Check** (`checker.rs`): run each word through zspell; cache results for repeated words
5. **Report** (`reporter.rs`): emit findings (article title, misspelled words, positions)

---

## Crates & Libraries

### Core

| Crate | Version | Purpose |
|---|---|---|
| `tokio` | latest | Async runtime for non-blocking I/O |
| `reqwest` | latest | Async HTTP client for Wikipedia API calls |
| `scraper` | latest | HTML parsing with CSS selector support (built on `html5ever`) |
| `zspell` | latest | Pure Rust Hunspell-compatible spell checker |

### Supporting

| Crate | Version | Purpose |
|---|---|---|
| `eventsource-client` | latest | SSE (Server-Sent Events) stream subscriber for Wikimedia EventStreams |
| `serde` + `serde_json` | latest | Deserialize JSON payloads from EventStream events |
| `clap` | latest | CLI argument parsing (select mode, provide article title, etc.) |
| `tracing` + `tracing-subscriber` | latest | Structured logging |

---

## Dictionary

- **Source:** [rla-es project](https://github.com/sbosio/rla-es) — the same dictionaries
  used by LibreOffice for Spanish
- **Format:** Standard Hunspell `.dic` + `.aff` files
- **Variant:** `es_ES` (Spain Spanish); can be swapped for `es_MX`, `es_AR`, etc.
- **Why Hunspell format:** Spanish is a morphologically rich language. A flat word list
  would need to enumerate every verb conjugation, noun form, etc. (millions of entries).
  Hunspell's affix system encodes root words + transformation rules, so it can validate
  any valid inflected form from a compact dictionary.

The dictionary files are loaded once at startup and reused for all requests.
Loading takes approximately 1–2 seconds and is a one-time cost.

---

## Performance & Resource Profile

- **Language:** Rust — compiled to a single native binary, no runtime needed
- **Memory:** ~30–50 MiB total (Rust binary + loaded Spanish dictionary, ~20 MiB per zspell)
- **CPU:** Each word lookup is ~1–10 µs. At 5 texts/second × 5,000 words = 25,000 lookups/sec,
  this consumes roughly 25% of one CPU core at the pessimistic end
- **Caching:** Words seen previously in the session are cached, making repeated common words
  (el, la, de, que, en...) essentially free after the first lookup
- **Concurrency:** `tokio` allows concurrent fetching and processing of multiple articles
  without blocking

Compared to Python alternatives (pyspellchecker, CyHunspell):
- No interpreter overhead (~30–50 MB saved on runtime alone)
- No GC pauses
- Single deployable binary

---

## Build Order

1. **Phase 1 — Core pipeline (Mode 1 only)**
   - Set up Cargo project and dependencies
   - Bundle or load Spanish dictionary at startup
   - Implement `extractor.rs`: fetch an article HTML, extract words
   - Implement `checker.rs`: wrap zspell, add word cache
   - Implement `reporter.rs`: print results
   - Implement `wikipedia.rs`: Wikipedia REST API client
   - Wire together in `main.rs` with a `--article` CLI argument

2. **Phase 2 — Stream mode**
   - Implement `stream.rs`: subscribe to Wikimedia EventStreams SSE
   - Filter events to Spanish Wikipedia (`es.wikipedia.org`) article edits
   - On each event, trigger the same pipeline as Phase 1
   - Add `--stream` CLI flag to select this mode

3. **Phase 3 — Hardening**
   - Tune HTML extraction rules to reduce false positives (proper nouns, abbreviations, etc.)
   - Add reconnection logic for stream drops
   - Add configurable output (stdout, file, future: webhook/queue)
