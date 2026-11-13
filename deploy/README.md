# Self-hosting the web page with server-side cleaning

`web/` works on any static host (everything runs in the browser). Served next to `server.py`, the page detects
`/api/health` and sends files to the native CLI instead, which uses all the server's cores.

On an Ubuntu 22.04+ host with nginx, certbot and ffmpeg:

```bash
useradd --system --no-create-home --shell /usr/sbin/nologin cleanplate
mkdir -p /opt/cleanplate /var/www/cleanplate
curl -sL https://github.com/dieplai/cleanplate/releases/latest/download/cleanplate-linux-x86_64.tar.gz \
  | tar xz -C /opt/cleanplate --strip-components=1
cp deploy/server.py /opt/cleanplate/ && cp -r web/. /var/www/cleanplate/
cp deploy/cleanplate.service /etc/systemd/system/ && systemctl enable --now cleanplate
cp deploy/nginx.conf /etc/nginx/sites-available/<your domain>   # edit server_name and certificate paths
```

Limits (environment of the service): `CLEANPLATE_MAX_MB` (95; Cloudflare's free plan caps uploads at 100 MB),
`CLEANPLATE_TIMEOUT` (95 s; Cloudflare drops responses after 100 s). One file is cleaned at a time; uploads and
results are deleted as soon as the response is sent.
