# Palpo for Docker

## Docker compose

```bash
cd deploy/docker
docker compose up -d
```

Don't forget to modify and adjust `compose.yml` and `palpo.toml` to your needs.

## `compose.yml`

- Modify `postgres` environment settings, you can set it's user, password and database name.
- Modify `palpo` service, you can set `palpo.toml` config file path and in volumes section, you can set local media path for store media files.

## `palpo.yml`
- You need to edit `server_name = "matrix.palpo.im"` to your server name.
- Modify `server` and `client` in `[well_known]` section.

To run and use Palpo you should probably use it with a Domain or Subdomain behind a reverse proxy (like Nginx, Traefik, Apache, ...) with a Lets Encrypt certificate.

For example, if you use Caddy, you can use setting like this for reverse proxy:

```
matrix.palpo.im {
    reverse_proxy /* 0.0.0.0:8008
}
```

Modify `matrix.palpo.im` to your custom domain.