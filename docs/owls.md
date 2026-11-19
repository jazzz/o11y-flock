# How owls work

To run owls, see the [README](../README.md). This page explains how owls get
their names, lists every setting, and covers running owls without Docker and
building the owl.

## Names

An owl doesn't choose its own name. roost assigns it one.

On its first start, an owl makes a random **instance ID** and saves it in the
file `instance` in its working directory. Every time the owl starts, it sends
this ID to roost. roost gives back the name it assigned to that ID, or
assigns a free name if the ID has none.

Two owls must never share a working directory. If a second process starts
with an instance ID that a running owl already uses, roost refuses it.

### Named owls

Most owls get a name such as `barn-01` or `snowy-02` and keep it for good.
The same instance ID gets the same name after the owl restarts, after an
outage and after roost has been down. roost gives that name to a different
instance ID only in two cases:

- roost hasn't heard from the owl for a week. roost's `ROOST_CLAIM_DAYS`
  setting sets this time.
- roost's admin releases the name from the dashboard.

### Ephemeral owls

An owl started with `OWL_EPHEMERAL=true` is an **ephemeral owl**. roost names
ephemeral owls from their own list, `owlet-01`, `owlet-02` and so on. roost
registers more of these names as ephemeral owls ask for them, up to roost's
`ROOST_OWLETS` setting.

An ephemeral owl keeps its name when it restarts. Two minutes after it stops,
roost can give that name to another ephemeral owl. So `owlet-07` today may
not be the same process as `owlet-07` yesterday. roost keeps a record of
every run, including who ran it.

### Groups

Named owls and ephemeral owls add each other to groups. An owl that stops
stays listed as a member of its groups. roost treats that member differently
depending on its kind:

- **A stopped named owl still counts.** roost counts each message the owl
  misses as dropped, and shows its groups as unclean.
- **A stopped ephemeral owl doesn't count.** roost doesn't expect it to
  receive messages, so ephemeral owls can come and go without making groups
  look broken.

### In Docker

In Docker, the owl's working directory is a volume. The instance ID
survives the container restarting, and survives `docker compose up`
replacing the container. `docker compose down` deletes the containers. The
next `up` then creates new volumes, so the owls get new instance IDs and new
names.

## Pausing

roost's admin can pause an owl from the dashboard. Within about 5 seconds,
the owl sends its last reports, then shuts down its chat client and its
logos-delivery node. While paused, the owl sends nothing on the network and
makes no requests to the auth service. When the admin resumes the owl, it
starts a new run with a new group, because libchat keeps its state only in
memory.

To stop an owl for good, send it SIGTERM.

## Settings

The owl reads these environment variables:

| Variable | Meaning |
|---|---|
| `ROOST_URL` | roost's address. Default `http://localhost:30117`. |
| `FLOCK_KEY` | The flock key, the same for every owl. roost's admin gives it out. Required. |
| `OWL_OPERATOR` | The name or handle of the person running the owl, at most 64 characters. roost shows it next to the owl and doesn't check it. Required. |
| `OWL_EPHEMERAL` | `true` runs the owl as an ephemeral owl. Default `false`. |
| `OWL_INSTANCE` | The instance ID to use instead of the one in the file `instance`. |
| `DELIVERY_LOG_LEVEL` | The log level of the logos-delivery node inside the owl. Default `ERROR`. |

[compose.owls.yml](../deploy/compose.owls.yml) also reads these:

| Variable | Meaning |
|---|---|
| `OWLS` | The number of named owls that `up` runs. Default 1. |
| `OWLETS` | The number of ephemeral owls that `up` runs. Default 0. |
| `OWL_IMAGE` | The owl image. Default `ghcr.io/jazzz/flock-owl:latest`. |

Docker Compose reads these variables from the file `deploy/.env` and from
your shell. A value set in your shell wins over the file. Compose requires
`ROOST_URL`, `FLOCK_KEY` and `OWL_OPERATOR` for every command, including
`ps` and `stop`. Without the file, you have to set all three in your shell
before each command.

`--scale owl=10` runs 10 named owls whatever `OWLS` says. The next `up`
without `--scale` goes back to the number in `OWLS`. `--scale owlet=N` does
the same for ephemeral owls and `OWLETS`.

## The owl image

CI builds the image `ghcr.io/jazzz/flock-owl` for amd64 and arm64, in
[owl-image.yml](../.github/workflows/owl-image.yml). It publishes these tags:

| Tag | Built from |
|---|---|
| `:latest` | main, whenever the crates, `Cargo.lock` or `docker/` change |
| `:sha-<12 hex>` | the same commits, and every release. This matches the `OWL_REV` that each owl reports to roost. |
| `:1.2.3`, `:1.2` | the git tag `v1.2.3` |

To keep every machine on the same owl, set `OWL_IMAGE` to a sha tag or a
release tag.

If `docker compose up` can't download the image, or you run `up --build`,
Compose builds the image on your machine. The build compiles liblogosdelivery
inside Nix. It takes a long time and needs a lot of free disk space. Don't
push an image you built yourself. It would replace CI's `:latest`, which
works on amd64 and arm64, with an image for your machine's architecture
only.

## Running owls without Docker

[deploy/owls.sh](../deploy/owls.sh) runs owls from the release binary (see
[Building](#building)). Each owl runs in its own directory under `run/`,
which holds its log, its message store and its instance ID.

```sh
deploy/owls.sh start 5       # named owls 1 to 5
deploy/owls.sh owlets 10     # ephemeral owls 1 to 10
deploy/owls.sh status
deploy/owls.sh stop owlets   # only the ephemeral owls
deploy/owls.sh stop          # every owl
```

owls.sh reads the same settings from your shell, with different defaults:

- `ROOST_URL` defaults to `http://127.0.0.1:30117`.
- `FLOCK_KEY` defaults to the contents of `run/flock.key`.
- `OWL_OPERATOR` defaults to your login name.

## Building

```
crates/flock-proto   the contract between owls and roost: reports, API types
crates/owl           one owl: a thin client on libchat's logos-chat crate
docker/              owl.Dockerfile
deploy/              compose.owls.yml (owls in Docker), owls.sh (without Docker)
.github/workflows/   owl-image.yml: publishes the owl image
```

The owl links liblogosdelivery from libchat's Nix flake. Point the build at
a built copy:

```sh
LOGOS_DELIVERY_LIB_DIR=/path/to/libchat/result/lib cargo build --release -p owl
```

Without `LOGOS_DELIVERY_LIB_DIR`, libchat's build script runs `nix build`
itself.
