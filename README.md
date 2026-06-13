# Orthonaut

Orthonaut is a self-hosted full-stack app to check Spanish orthography on Wikipedia pages.

## Stack

- Backend: Rust (`axum`, `zspell`, `rusqlite`)
- Frontend: React + Tailwind CSS + Headless UI
- Database: SQLite

## Quick start

```bash
make setup  # download dictionaries + npm install
make dev    # start backend (port 3000) + frontend (port 5173)
```

The frontend runs on `http://localhost:5173` and proxies API calls to `http://localhost:3000`.

See `make help` for all available targets.

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

The config file path can be overridden with `ORTHONAUT_CONFIG_PATH`. Other env vars:

| Variable | Default | Description |
|---|---|---|
| `PORT` | `3000` | Port the backend listens on |
| `ORTHONAUT_DB_PATH` | `$HOME/orthonaut.db` | SQLite database path |
| `ORTHONAUT_DICT_DIR` | `$HOME/dictionaries` | Hunspell dictionary directory |
| `ORTHONAUT_CONFIG_PATH` | `$HOME/orthonaut.toml` | Config file path |

## Deploying to Toolforge

The app runs at `https://orthonaut.toolforge.org/` on Wikimedia's Toolforge platform.
The Rust binary serves both the API and the compiled React frontend.

### First-time setup

**1. Register the OAuth app for production**

In your [Meta-Wiki OAuth registration](https://meta.wikimedia.org/wiki/Special:OAuthConsumerRegistration),
add `https://orthonaut.toolforge.org/api/auth/callback` as an allowed redirect URI.

**2. Upload the config to Toolforge**

The Spanish dictionary files are embedded in the binary — no upload needed.
Only the config file (which contains secrets) needs to be placed on Toolforge.
Write it directly from the SSH session to avoid any path or permission uncertainty:

```bash
ssh -i ~/.ssh/<your-key> <your-username>@login.toolforge.org
become orthonaut
mkdir -p ~/dictionaries
cat > ~/orthonaut.toml << 'EOF'
wikimedia_contact = "https://es.wikipedia.org/wiki/User:<your-wikipedia-username>"

[oauth]
client_id     = "..."
client_secret = "..."
redirect_uri  = "https://orthonaut.toolforge.org/api/auth/callback"
EOF
exit
```

Omit the `[oauth]` section entirely if not yet configured — the app runs without it.

**3. First deploy**

```bash
make deploy-prep   # builds frontend, stages frontend/dist/ for commit
git commit -m "initial Toolforge deployment"
git push
```

Then on Toolforge:
```bash
ssh -i ~/.ssh/<your-key> <your-username>@login.toolforge.org
become orthonaut
toolforge build start https://github.com/JavierMonton/orthonaut
# --mount all keeps the tool home ($HOME) mounted, where the config, SQLite DB,
# and word lists live. Required — the app reads ~/orthonaut.toml at startup.
toolforge webservice buildservice start --mount all
```

### Subsequent deploys

If only backend changed:
```bash
git push
```
Then on Toolforge:
```bash
ssh -i ~/.ssh/<your-key> <your-username>@login.toolforge.org
become orthonaut
toolforge build start https://github.com/JavierMonton/orthonaut
toolforge webservice restart
```

If frontend changed:
```bash
make deploy-prep   # rebuilds frontend/dist/ and stages it
git commit -m "update frontend build"
git push
```
Then on Toolforge (same as above).

### Checking logs

```bash
ssh -i ~/.ssh/<your-key> <your-username>@login.toolforge.org
become orthonaut
toolforge webservice logs
```
