# corvus-server

Backend server for [Corvus](https://github.com/huskago/corvus), a custom Minecraft launcher. It exposes a public JSON API consumed by the launcher client and an admin panel for managing instances, news, manifests, and file uploads.

Built with [Axum](https://github.com/tokio-rs/axum) and Tokio. All data is stored as JSON files on disk, no database required.

## Features

- Instance and news management
- Per-instance manifest management (mods, resource packs, shaders, extra files)
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
```

Environment variable examples:
```
CORVUS_SERVER__PORT=9000
CORVUS_SERVER__PUBLIC_URL=https://launcher.example.com
CORVUS_AUTH__JWT_EXPIRY_SECS=3600
```

The `ADMIN_PASSWORD` env var sets the password on first run if `password_hash` is empty.

## Data layout

```
data/
├── instances.json
├── news.json
└── instances/
    └── {game_dir}/
        ├── manifest.json
        ├── files.json
        └── files/
            └── *.jar / *.zip
```

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

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/health` | Health check |
| `GET` | `/instances.json` | Instance list |
| `GET` | `/news.json` | News list |
| `GET` | `/{game_dir}/manifest.json` | Instance manifest |
| `GET` | `/files/{game_dir}/{filename}` | Download a file |
| `GET` | `/admin` | Admin panel |

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

### Manifest & Files

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/admin/instances/{id}/manifest` | Get manifest |
| `PUT` | `/api/admin/instances/{id}/manifest` | Replace manifest |
| `POST` | `/api/admin/instances/{id}/upload` | Upload files (`multipart/form-data`) |
| `GET` | `/api/admin/instances/{id}/files` | List uploaded files |
| `DELETE` | `/api/admin/instances/{id}/files/{filename}` | Delete a file |

Upload accepts `.jar` and `.zip` only. Files are streamed to disk and SHA-1 is computed incrementally. The size limit is set by `max_upload_mb` in the config (default 512 MB).

### Dashboard

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/admin/dashboard` | Instance count, news count, total files, total size, uptime |

## HTTPS

The server itself speaks plain HTTP. Put it behind a reverse proxy (nginx, Caddy, Traefik) that handles TLS termination, then set `PUBLIC_URL` to your HTTPS domain.

## License

MIT, see [LICENSE](LICENSE).
