# corvus-server

Backend server for [Corvus](https://github.com/huskago/corvus), a custom Minecraft launcher. It exposes a public JSON API consumed by the launcher client and an admin panel for managing instances, news, manifests, and file uploads.

Built with [Axum](https://github.com/tokio-rs/axum) and Tokio. All data is stored as JSON files on disk, no database required.

## Features

- Instance and news management with per-instance changelogs and pinned news
- Launcher auto-update hosting, serves `latest.json` and binaries for the Tauri updater
- GitHub Actions build trigger from the admin panel (optional)
- Per-instance manifest management (mods, resource packs, shaders, extra files)
- Extra files tree with recursive folder support and public download endpoint
- Phantom file detection (`/scan`) and one-shot integration into the manifest (`/integrate`)
- SHA-1 rehash to recompute checksums for all indexed files (`/rehash`)
- Streaming file upload with SHA-1 checksums and configurable size limit
- JWT authentication with Argon2 password hashing
- Built-in admin panel served at `/admin`
- Docker-ready

## Setup

### Requirements

- Rust 1.85+ (edition 2024)
- Or Docker

### From source

```sh
git clone https://github.com/huskago/corvus-server
cd corvus-server
cargo build --release
ADMIN_PASSWORD=yourpassword ./target/release/corvus-server
```

The server writes `config.toml` and `./data/` on first run. On subsequent runs it reads the config from there.

### Docker

```sh
docker compose up -d
```

Edit `compose.yml` to set your `ADMIN_PASSWORD` and `PUBLIC_URL` before the first run.

The admin panel is then available at `http://localhost:8080/admin`.

## Configuration

Configuration is loaded from `config.toml` (or the path set in `CORVUS_CONFIG_PATH`), then overridden by environment variables using the `CORVUS_` prefix with `__` as separator.

```toml
[server]
port = 8080
data_dir = "./data"
public_url = "http://localhost:8080"
max_upload_mb = 512

[auth]
username = "admin"
password_hash = ""        # auto-generated on first run
jwt_secret = ""           # auto-generated on first run
jwt_expiry_secs = 86400

[github]
pat = ""                  # Personal Access Token with `workflow` scope
repo = ""                 # e.g. "yourname/corvus"
workflow = "release.yml"  # workflow file to dispatch
branch = "main"           # branch to dispatch on
```

Environment variable examples:
```
CORVUS_SERVER__PORT=9000
CORVUS_SERVER__PUBLIC_URL=https://launcher.example.com
CORVUS_AUTH__JWT_EXPIRY_SECS=3600
CORVUS_GITHUB__PAT=ghp_...
CORVUS_GITHUB__REPO=yourname/corvus
```

The `ADMIN_PASSWORD` env var sets the password on first run if `password_hash` is empty.

The `[github]` section is optional. If `pat` or `repo` is empty, the "Trigger Build" button in the admin panel will return an error.

## Data layout

```
data/
├── instances.json
├── news.json
├── launcher-release.json
├── instances/
│   └── {game_dir}/
│       ├── manifest.json
│       ├── files.json
│       ├── extra-files.json
│       ├── files/
│       │   └── *.jar / *.zip
│       └── extra/
│           └── **/*   (arbitrary nested files)
└── launcher-updates/
    └── {platform}/
        ├── corvus_{version}_x64-setup.exe
        └── corvus_{version}_x64-setup.exe.sig
```

`launcher-release.json` stores the current version metadata (version, notes, pub_date) and per-platform download URLs and signatures in Tauri updater format. `launcher-updates/{platform}/` stores the binary and signature files served to the Tauri updater.

Valid platform identifiers: `windows-x86_64`, `linux-x86_64`, `darwin-aarch64`, `darwin-x86_64`.

## API

All admin endpoints require `Authorization: Bearer <token>`.

Errors are returned as `{"error": "message"}` with the appropriate HTTP status code.

### Auth

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/api/auth/login` | Get a JWT token |
| `POST` | `/api/auth/change-password` | Change password and rotate JWT secret |

`POST /api/auth/login` - body: `{"username": "admin", "password": "..."}` - returns `{"token": "...", "expiresAt": 1234567890}`.

Login is rate-limited to 10 attempts per IP per 60 seconds.

### Public (no auth)

| Method | Path | Description                                                                                                                         |
|--------|------|-------------------------------------------------------------------------------------------------------------------------------------|
| `GET` | `/health` | Health check                                                                                                                        |
| `GET` | `/instances.json` | Instance list (pinned news sorted first)                                                                                            |
| `GET` | `/news.json` | News list (pinned items sorted first)                                                                                               |
| `GET` | `/{game_dir}/manifest.json` | Instance manifest                                                                                                                   |
| `GET` | `/files/{game_dir}/{filename}` | Download a file                                                                                                                     |
| `GET` | `/extra/{id}/{*path}` | Download an extra file (arbitrary path)                                                                                             |
| `GET` | `/updates/latest.json` | Tauri updater endpoint, returns version, notes, pub_date, and per-platform URLs/signatures. Returns 404 if no release is configured |
| `GET` | `/updates/{platform}/{filename}` | Serve a launcher binary or `.sig` file                                                                                              |
| `GET` | `/admin` | Admin panel                                                                                                                         |

### Instances

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/admin/instances` | List instances |
| `POST` | `/api/admin/instances` | Create instance |
| `PUT` | `/api/admin/instances/{id}` | Update instance |
| `DELETE` | `/api/admin/instances/{id}` | Delete instance and its files |
| `PUT` | `/api/admin/instances/order` | Reorder instances |

`{id}` is the `gameDirName` field. Deleting an instance removes its directory from disk.

### News

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/admin/news` | List news items |
| `POST` | `/api/admin/news` | Create or update a news item (upsert by `id`) |
| `DELETE` | `/api/admin/news/{id}` | Delete a news item |
| `PUT` | `/api/admin/news/order` | Reorder news items |

News items have a `pinned` boolean field (default `false`). Pinned items appear first in the public `/news.json` response regardless of order.

### Manifest & Files

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/admin/instances/{id}/manifest` | Get manifest |
| `PUT` | `/api/admin/instances/{id}/manifest` | Replace manifest |
| `PATCH` | `/api/admin/instances/{id}/manifest/entry` | Update a single manifest entry's status |
| `POST` | `/api/admin/instances/{id}/upload` | Upload mod/resource pack/shader files (`multipart/form-data`) |
| `GET` | `/api/admin/instances/{id}/files` | List uploaded files |
| `DELETE` | `/api/admin/instances/{id}/files/{filename}` | Delete a file |

Upload accepts `.jar` and `.zip` only. Files are streamed to disk and SHA-1 is computed incrementally. The size limit is set by `max_upload_mb` in the config (default 512 MB).

`PATCH /api/admin/instances/{id}/manifest/entry` body: `{"name": "mod.jar", "section": "mods", "status": "required"}`. Valid sections: `mods`, `resourcePacks`, `shaders`.

### Extra Files

Per-instance arbitrary files served publicly at `/extra/{id}/{*path}`. Organized in a recursive folder tree.

| Method | Path | Description                                                           |
|--------|------|-----------------------------------------------------------------------|
| `GET` | `/api/admin/instances/{id}/extra-files/tree` | List files and subdirectories at `?dir=` (default: root)              |
| `POST` | `/api/admin/instances/{id}/extra-files/mkdir` | Create a directory, body: `{"path": "subdir/name"}`                   |
| `POST` | `/api/admin/instances/{id}/extra-files/upload` | Upload extra files (`multipart/form-data`, `?dir=` for target folder) |
| `DELETE` | `/api/admin/instances/{id}/extra-files/{*path}` | Delete an extra file                                                  |

### Scan, Integrate & Rehash

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/admin/instances/{id}/scan` | Detect files on disk that are not indexed in the manifest |
| `POST` | `/api/admin/instances/{id}/integrate` | Index untracked files into the manifest and compute their SHA-1 |
| `POST` | `/api/admin/instances/{id}/rehash` | Recompute SHA-1 for all already-indexed files; returns `{"updated": N}` |

`/scan` returns `{"files": [...], "extra_files": [...]}`, phantom entries in both the `files/` and `extra/` directories.

`/integrate` body: `{"files": [{"name": "mod.jar", "section": "mods", "status": "required"}], "extra_files": [{"path": "config/file.json"}]}`. Hashes each file, writes entries to `files.json` / `extra-files.json`, and updates `manifest.json`.

### Launcher Updates

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/admin/updates` | Get current release metadata |
| `PUT` | `/api/admin/updates` | Update version, notes, pub_date |
| `POST` | `/api/admin/updates/{platform}/upload` | Upload binary + signature for a platform (`multipart/form-data` with `binary` and `signature` fields) |
| `DELETE` | `/api/admin/updates/{platform}` | Remove a platform entry and its files |
| `POST` | `/api/admin/trigger-build` | Dispatch a GitHub Actions workflow to build all platforms |

The trigger-build endpoint accepts an optional JSON body `{"version": "...", "release_notes": "..."}` which is forwarded to the workflow as inputs. Requires `[github]` config to be set.

Instance objects have a `changelog` field: an array of `{version, date, notes}` entries. It is displayed in the launcher below the PLAY button and is editable in the admin panel's instance editor.

### Dashboard

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/admin/dashboard` | Instance count, news count, total files, total size, uptime |

## HTTPS

The server itself speaks plain HTTP. Put it behind a reverse proxy (nginx, Caddy, Traefik) that handles TLS termination, then set `PUBLIC_URL` to your HTTPS domain.

## License

MIT, see [LICENSE](LICENSE).
