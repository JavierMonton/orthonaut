# Orthonaut

Orthonaut is a self-hosted full-stack app to check Spanish orthography on Wikipedia pages.

## Stack

- Backend: Rust (`axum`, `zspell`, `rusqlite`)
- Frontend: React + Tailwind CSS + Headless UI
- Database: SQLite

## Local development

```bash
make dev
```

Starts the backend on `http://localhost:3000` and the frontend on `http://localhost:5173`
(the frontend proxies API calls to the backend). On first run it downloads the dictionaries
and installs frontend dependencies automatically.

## Configuration

Copy `orthonaut.toml.example` to `orthonaut.toml` and fill in your details:

```toml
wikimedia_contact = "https://es.wikipedia.org/wiki/User:<your-wikipedia-username>"

[oauth]
client_id     = "..."
client_secret = "..."
redirect_uri  = "http://localhost:5173/api/auth/callback"
token         = "..."  # optional: skip OAuth flow locally
```

`orthonaut.toml` contains secrets and is **not** committed to git — it must be created
manually wherever the app runs (see the Toolforge section below).

### Word lists: local files vs. Wikipedia

Orthonaut keeps two word lists — **valid words** (false positives to suppress) and
**always-wrong words** (always flagged). By default both live in local files
(`suppressions.txt` / `always_wrong.txt`) and the UI lets you manage both, exporting each to
its file.

Set the optional `wordlist_page` key to back the lists with a Wikipedia page instead
(Replacer-style), recommended for production:

```toml
wordlist_page = "Usuario:Jmlarraz/Orthonaut/Palabras"
```

When `wordlist_page` is set:

- Both lists are **read** from that page at startup. The "add wrong words" input is hidden —
  the always-wrong list is managed entirely on-wiki.
- The "Valid word" button still works; new valid words are buffered locally until you click
  **Export valid words to Wikipedia**, which **requires you to be logged in** (the edit is made
  with your account).
- Export re-reads the page and **merges** before writing, so words anyone added to the page
  manually are never lost. Only the valid-words block is written; the always-wrong block is
  left untouched.

Create the page with free text plus the two sentinel blocks below (markers must match exactly,
one lowercase word per line; anything outside the markers is ignored and safe for docs):

```
<!-- ORTHONAUT:VALIDAS:START -->
superestrella
<!-- ORTHONAUT:VALIDAS:END -->

<!-- ORTHONAUT:INCORRECTAS:START -->
concecuencia
<!-- ORTHONAUT:INCORRECTAS:END -->
```

Paths and the port can be overridden with environment variables:

| Variable | Default (debug / release) | Description |
|---|---|---|
| `PORT` | `3000` / `8000` | Port the backend listens on |
| `ORTHONAUT_CONFIG_PATH` | `$HOME/orthonaut.toml` | Config file path |
| `ORTHONAUT_DB_PATH` | `$HOME/orthonaut.db` | SQLite database path |
| `ORTHONAUT_DICT_DIR` | `$HOME/dictionaries` | Dictionary / word-list directory |

## Deploying to Toolforge

The app runs at `https://orthonaut.toolforge.org/`. A single Rust binary serves both the API
and the React frontend. The Hunspell dictionaries and the compiled frontend are **embedded in
the binary at build time**, so the only thing that must exist on Toolforge is `orthonaut.toml`.

> The frontend is embedded from `frontend/dist/`, which is committed to git. Whenever the
> frontend changes, rebuild it (`cd frontend && npm run build`) and commit `frontend/dist/`
> before deploying.

### First-time setup (once)

SSH into the bastion and switch to the tool account:

```bash
ssh -i ~/.ssh/<your-key> <your-username>@login.toolforge.org
become orthonaut
```

**1. Create the config file manually.** It is not in the repo, so write it directly:

```bash
cat > ~/orthonaut.toml << 'EOF'
wikimedia_contact = "https://es.wikipedia.org/wiki/User:<your-wikipedia-username>"
wordlist_page     = "Usuario:Jmlarraz/Orthonaut/Palabras"
EOF
```

`wordlist_page` makes production read/write the word lists from that Wikipedia page (see
[Word lists](#word-lists-local-files-vs-wikipedia) above); omit it to use local files.

To enable Wikipedia editing, add an `[oauth]` section with a `redirect_uri` of
`https://orthonaut.toolforge.org/api/auth/callback` (register it first at the
[Meta-Wiki OAuth consumer registration](https://meta.wikimedia.org/wiki/Special:OAuthConsumerRegistration)).
Without it, the app still runs — only editing is disabled.

**2. Point the app at the config and data on NFS** (inside the container `$HOME` is not the
tool home, so absolute paths are required; these persist across deploys):

```bash
toolforge envvars create ORTHONAUT_CONFIG_PATH /data/project/orthonaut/orthonaut.toml
toolforge envvars create ORTHONAUT_DB_PATH /data/project/orthonaut/orthonaut.db
toolforge envvars create ORTHONAUT_DICT_DIR /data/project/orthonaut/dictionaries
```

**3. Fetch the `toolforge.sh` script onto the bastion.** The Toolforge build produces a
container image from GitHub and never writes to the bastion, so the script has to be placed
here separately (`make` is not installed on the bastion either). Its commands only call the
`toolforge` CLI, so the full repo isn't needed — just the single file:

```bash
curl -sL https://raw.githubusercontent.com/JavierMonton/orthonaut/main/toolforge.sh -o ~/toolforge.sh
chmod +x ~/toolforge.sh
```

**4. First build and start.** With the script in place you can use the shortcuts:

```bash
./toolforge.sh build
./toolforge.sh start
```

`./toolforge.sh start` runs `toolforge webservice buildservice start --mount all --mem 2Gi --cpu 1`:
`--mount all` keeps the tool's NFS storage mounted; `--mem 2Gi` is required because building
the Hunspell dictionary at startup exceeds the default 512Mi limit (otherwise the container is
OOMKilled into a crash loop). A healthy start logs `backend listening on 0.0.0.0:8000`.

### Updating

After pushing changes to GitHub (including a rebuilt `frontend/dist/` if the frontend changed),
deploy from the bastion home:

```bash
./toolforge.sh build     # rebuild the image from GitHub
./toolforge.sh restart   # roll out the new image
```

If you changed `toolforge.sh` itself, re-run the `curl` from step 3 first to refresh it on the
bastion.

Other shortcuts: `./toolforge.sh logs`, `./toolforge.sh stop`, `./toolforge.sh start`.
Run `./toolforge.sh help` for the full list.
