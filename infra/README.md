# Deployment

Production runs on a single VPS behind the host nginx, at
https://lanka-data-layer.prabhavalabs.com.

## Topology

- **API** — Docker container from [`api/Dockerfile`](../api/Dockerfile), bound
  to `127.0.0.1:8600`, artifacts mounted read-only from
  `foundry/data/artifacts/`. See [`docker-compose.yml`](docker-compose.yml).
- **Web** — static Vite build in `/opt/lanka-data-layer/web-dist`, served
  directly by the host nginx; `/v1/*` is proxied to the API container. Vhost:
  [`nginx/lanka-data-layer.conf`](nginx/lanka-data-layer.conf), TLS via
  certbot.
- **Data artifacts** (`lanka.sqlite`, `tiles/*.pmtiles`, downloads) are NOT
  built on the server and NOT in git — see "Data releases" below.

## CI/CD

Every merge to `main` triggers [`.github/workflows/deploy.yml`](../.github/workflows/deploy.yml):

1. `test` job — full workspace test suites + web production build.
2. `deploy` job — SSH to the server and run
   [`deploy.sh`](deploy.sh), which resets the server checkout to
   `origin/main`, rebuilds the web bundle in a throwaway Node container,
   publishes it to `web-dist/`, rebuilds + restarts the API container, and
   health-checks `127.0.0.1:8600/v1/health`.
3. A final public health check against the live domain.

Repository secrets: `DEPLOY_SSH_KEY` (dedicated ed25519 deploy key),
`DEPLOY_HOST` (server address). Rotate by generating a new keypair, replacing
the public key in the server's `~/.ssh/authorized_keys`, and updating the
secret.

## Rate limiting

Public-API protection lives at the nginx layer (zones in
[`nginx/lanka-data-layer-limits.conf`](nginx/lanka-data-layer-limits.conf),
installed to `/etc/nginx/conf.d/`; per-location `limit_req`/`limit_conn` in
the vhost):

- `/v1/` — 10 req/s per client IP, bursts to 40 absorbed, then `429` with a
  JSON envelope body and `Retry-After: 1`.
- `/v1/tiles/` — separate 40 req/s zone with burst 120: PMTiles range
  requests arrive in bursts of dozens per zoom gesture, and they're cheap
  sendfile reads.
- 30 concurrent connections per IP across both.

The vhost also trusts `CF-Connecting-IP` from Cloudflare's published
ranges, so per-IP keys keep pointing at real clients if the DNS record is
ever flipped to orange-cloud (inert while the record stays DNS-only).
Changing limits: edit the zone/burst numbers, reinstall the two files, and
`nginx -t && systemctl reload nginx` — nginx config is installed manually,
not by `deploy.sh`.

## Data releases

Code deploys never touch data. To ship a new data build:

```bash
pnpm foundry run build          # on a workstation (needs FOUNDRY_SEED_SOURCE, tippecanoe)
rsync -avz --delete foundry/data/artifacts/ root@<server>:/opt/lanka-data-layer/foundry/data/artifacts/
ssh root@<server> 'docker restart lanka-data-layer-api'
```

The API reads the SQLite artifact read-only; a restart picks up the new file
and its `data_version` (which also rotates every ETag).

## First-time server setup

```bash
# on the server
git clone https://github.com/prabhavalabs/lanka-data-layer /opt/lanka-data-layer
# ship artifacts (from a workstation): see "Data releases" above
bash /opt/lanka-data-layer/infra/deploy.sh main
cp /opt/lanka-data-layer/infra/nginx/lanka-data-layer.conf /etc/nginx/sites-available/lanka-data-layer
ln -s /etc/nginx/sites-available/lanka-data-layer /etc/nginx/sites-enabled/
nginx -t && systemctl reload nginx
certbot --nginx -d lanka-data-layer.prabhavalabs.com
```
