# flock

A fuzzer for LogosChat group chats. Synthetic clients, the **owls**, each
create a group, grow groups by adding other owls, and post to them over
logos-delivery, continuously. Each owl reports what it sends, receives, and
sees of each group's membership and history to **roost**, the management
node, which judges the protocol's health from them. roost lives in its own
repository.

## Run owls: quick start

This adds owls to an existing flock; there's nothing to build. You need
Docker with Compose, plus two things from roost's admin: roost's address and
the flock key.

1. Get the compose file:

   ```sh
   git clone https://github.com/jazzz/o11y-flock.git
   cd o11y-flock
   ```

2. Create `deploy/.env` with these lines, replacing each `<…>`:

   ```ini
   ROOST_URL=<roost's address, e.g. http://roost.example.org:30117>
   FLOCK_KEY=<the flock key>
   OWL_OPERATOR=<your name or handle>
   OWLS=5
   ```

   - `OWL_OPERATOR` is shown next to your owls in roost, so whoever watches
     the flock knows whom to ask about them.
   - `OWLS` is how many owls to run.
   - If roost runs on this same machine, under Docker Desktop, `ROOST_URL` is
     `http://host.docker.internal:30117`. Inside a container, `localhost` is
     the container itself.

   Git ignores `deploy/.env`, so the key stays out of commits.

3. Start them. The first start downloads the image.

   ```sh
   docker compose -f deploy/compose.owls.yml up -d
   ```

   They appear in roost's dashboard under your name. They keep running until
   you stop them, and come back whenever Docker restarts. To check on them
   here:

   ```sh
   docker compose -f deploy/compose.owls.yml ps
   docker compose -f deploy/compose.owls.yml logs -f
   ```

4. Stop them with `stop`, and start them again with `up -d`:

   ```sh
   docker compose -f deploy/compose.owls.yml stop
   ```

   They come back as the same owls, with the same names. **Avoid `down`.**
   It removes the containers, so the next `up` starts owls that need new
   names, and roost keeps the old names held for a week. The whole flock
   shares the named owls; when they're all held, new owls wait.

5. Update to the latest owl:

   ```sh
   docker compose -f deploy/compose.owls.yml pull
   docker compose -f deploy/compose.owls.yml up -d
   ```

   The recreated containers keep their names.

To run extras you'll add and remove often, also set `OWLETS=20` (or however
many) in `deploy/.env`. Owlets give their names back two minutes after they
stop (see [Identity](#identity)).

## Running owls

Every owl needs three things: where roost is (`ROOST_URL`), the **flock
key** (`FLOCK_KEY`), the same key for every owl, which roost's admin hands
out, and who runs it (`OWL_OPERATOR`): your name or handle, shown next to
your owls in roost so whoever watches the flock knows whom to ask about them.
It is not checked.

### In Docker

On any machine that can reach roost, [compose.owls.yml](deploy/compose.owls.yml)
reads `deploy/.env`. In it, `OWLS` and `OWLETS` set how many owls and owlets
`up` runs (default 1 and 0). `--scale` overrides them until the next `up`, and
naming a service leaves the other alone:

```sh
docker compose -f deploy/compose.owls.yml up -d --scale owl=10
docker compose -f deploy/compose.owls.yml up -d --scale owlet=20 owlet
docker compose -f deploy/compose.owls.yml rm -sf owlet     # owlets gone again
```

### The image

`up` pulls the image, `ghcr.io/jazzz/flock-owl` (`OWL_IMAGE` overrides it).
If it can't (a private package needs `docker login ghcr.io` with a
`read:packages` token first), or with `up --build`, it builds the image
locally, compiling liblogosdelivery inside Nix, which takes a while.

CI publishes the image for amd64 and arm64
([owl-image.yml](.github/workflows/owl-image.yml)):

| Tag | From |
|---|---|
| `:latest` | main, when the crates, `Cargo.lock` or `docker/` change |
| `:sha-<12 hex>` | the same, and every release; the `OWL_REV` an owl reports |
| `:1.2.3`, `:1.2` | a `v1.2.3` tag |

Pin a sha or release tag to keep every machine on the same owl.

### Without Docker

[deploy/owls.sh](deploy/owls.sh) runs owls from the release binary (see
[Developing](#developing)), each in its own directory under `run/`:

```sh
deploy/owls.sh start 5       # owls 1..5; FLOCK_KEY, or the key in run/flock.key
deploy/owls.sh owlets 10     # owlets 1..10
deploy/owls.sh status
deploy/owls.sh stop owlets   # just the owlets
deploy/owls.sh stop
```

`OWL_OPERATOR` defaults to your login name here.

### Settings

| Setting | Meaning |
|---|---|
| `ROOST_URL` | where roost is, default `http://localhost:30117` |
| `FLOCK_KEY` | the flock key |
| `OWL_OPERATOR` | who runs the owl: required, at most 64 characters |
| `OWL_EPHEMERAL` | `true` to run as an owlet (below) |
| `OWL_INSTANCE` | overrides the owl's instance ID (below) |
| `DELIVERY_LOG_LEVEL` | the embedded logos-delivery node's log level, default `ERROR` |
| `OWLS`, `OWLETS` | compose only: how many owls and owlets `up` runs, default 1 and 0 |
| `OWL_IMAGE` | compose only: the image, default `ghcr.io/jazzz/flock-owl:latest` |

## Identity

An owl has no identity of its own. On start it sends roost its **instance
ID**, which roost assigns an identity to. The ID is made on first start and
kept in `./instance` in the owl's working directory. In Docker that
directory is a volume, so the ID survives the container restarting and being
recreated, but not `docker compose down`, which gives the owls new IDs and so
new names. Never share one directory or volume between owls: roost refuses a
second live process with the same ID.

An owl gets one of the **named owls** (`barn-01`, `snowy-02`, …) and keeps
it: the same instance gets the same name every time it starts, through
restarts, outages and roost being down. Only once an instance has not been
heard from for a week (roost's `ROOST_CLAIM_DAYS`), or roost's admin releases
the name, can another instance have it. When every name is held, a new owl
waits.

An **owlet** (`OWL_EPHEMERAL=true`) is an extra: it gets a name like
`owlet-07` from a pool roost grows as owlets want them. An owlet keeps its
name through a restart, but two minutes after it goes the name is free for
the next owlet, so the name says nothing about which process had it before;
roost's record of each run, and who ran it, does. Owlets and named owls add
each other to groups. A member that leaves stays listed in its groups;
roost holds that against the group for a named owl, but not for an owlet,
so extras can come and go without showing as dropped messages or unclean
groups.

## Pausing

roost can pause an owl. Within about 5 seconds the owl flushes its reports
and shuts its libchat client and logos-delivery node down, so it puts nothing
on the network and asks nothing of the auth service, then restarts itself as
a fresh process that waits. Resumed, it starts a new run with a new group:
libchat's state is in memory. SIGTERM stops an owl for good.

## Developing

```
crates/flock-proto   the owl ↔ roost contract: reports, API types
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

Without it, libchat's build script falls back to `nix build` on its own.
