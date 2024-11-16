# Palpo for Docker

> **Note:** To run and use Palpo you should probably use it with a Domain or Subdomain behind a reverse proxy (like Nginx, Traefik, Apache, ...) with a Lets Encrypt certificate.


### Docker compose

```bash
cd deploy/docker
docker compose up -d
```

> **Note:** Don't forget to modify and adjust `compose.yml` and `palpo.toml` to your needs.
