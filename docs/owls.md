# How owls work

To run an owl, see the [README](../README.md). This page explains how owls
get their names and lists every setting. It also covers running owls with
Docker Compose or without Docker, and building the owl.

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

Owls also remove each other from groups, as often as roost's settings say.
An owl never removes itself, and removes only from a group of three or more.
An owl that has been removed tells roost, which stops expecting it to receive
the group's messages. If another owl adds it to the group again, it joins as
new.

### What owls tell roost

Besides what it sends and receives and what it sees of its groups, an owl
sends roost libchat's warnings and errors, and its own panics, as anomalies.

Each sent and received message carries the ID libchat's SDS layer gave it,
which is the same at every member.

### In Docker

In Docker, the owl's working directory, `/var/lib/owl`, is a volume.

- **`docker run -v owl-1:/var/lib/owl`**, as in the README, names the
  volume. The volume stays when you delete the container, so the next
  `docker run` with the same volume gets the same instance ID and the same
  name.
- **Docker Compose** gives each container an unnamed volume. The volume
  survives `docker compose up` replacing the container. `docker compose
  down` deletes the containers, and the next `up` creates new volumes, so
  the owls get new instance IDs and new names.

## Pausing

roost's admin can pause an owl from the dashboard. Within about 5 seconds,
the owl sends its last reports, then shuts down its chat client and its
logos-delivery node. While paused, the owl sends nothing on the network and
makes no requests to the auth service. When the admin resumes the owl, it
starts a new run with a new group, because libchat keeps its state only in
memory.

To stop an owl for good, send it SIGTERM.

## Fork recovery

roost can order an owl to ask a group to re-add it, when the owl has forked
off that group. The owl picks up the order within about 5 seconds, when it
next asks roost whether it is paused. Then libchat sends a request to the
branch that outranks the owl's, naming one member of that branch. That
member removes the owl and adds it back. The owl joins that branch and
reports its new epoch as a commit.

The owl carries out each order once and reports what came of it:

- **asked**: libchat sent the request.
- **not asked**: libchat sent nothing. It only asks when it sees the owl as
  forked, from the beacons other members send, and when a member of the
  other branch can answer.
- **not in group**: the owl isn't a member of the group.
- **failed**: libchat returned an error.

The report also says where libchat saw the owl just before: synced,
lagging, suspect, forked, or unknown.

An order can ask the next member of that branch instead of the one asked
before. Give the first member time to answer before sending one: if two
members answer at once, the group forks again.

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

With `docker run`, pass each one as `-e NAME=value`. With Docker Compose,
set `ROOST_URL`, `FLOCK_KEY` and `OWL_OPERATOR` in the file `.env`, next to
the compose file.

## Running owls with Docker Compose

[compose.owls.yml](../deploy/compose.owls.yml) runs several owls at once.
It works on its own, without the rest of the repository:

```sh
mkdir flock-owls && cd flock-owls
curl -fsSL -o compose.yaml https://raw.githubusercontent.com/jazzz/o11y-flock/main/deploy/compose.owls.yml
curl -fsSL -o .env https://raw.githubusercontent.com/jazzz/o11y-flock/main/deploy/.env.example
```

Open `.env` and fill in roost's address, the flock key and your name.
`OWLS` sets how many named owls to run. Never commit `.env`: it holds the
flock key.

Until all three values are filled in, `docker compose` refuses to start and
names the missing one. A wrong value still lets the owls start, but they
can't reach roost. Their logs show `Dns Failed` for the address, or
`status code 401` for the key.

Then run these commands in that directory:

```sh
docker compose up -d     # start the owls
docker compose ps        # list them
docker compose logs -f   # watch their logs
docker compose stop      # stop them; `up -d` starts them again, with the same names
```

**Don't run `docker compose down`.** It deletes the containers, and the
owls that the next `up` starts get new names. roost holds each old name for
a week.

To update the owls to the newest build:

```sh
docker compose pull && docker compose up -d
```

The updated owls keep their names.

Ephemeral owls run in the service `owlet`:

```sh
docker compose up -d --scale owlet=20 owlet   # run 20 ephemeral owls
docker compose rm -sf owlet                   # stop and delete every ephemeral owl
```

`--scale owlet=20` sets how many ephemeral owls run. The final `owlet`
limits the command to ephemeral owls, so it leaves the other owls alone.
`--scale` lasts until the next `up` without it, which goes back to `OWLS`
named owls and no ephemeral owls. To keep some ephemeral owls running
instead, add `OWLETS=20` (or however many) to `.env`.

The file names its Compose project `flock-owls`, the same on every machine.
So a machine has one set of these owls, and every `docker compose` command
that uses this file acts on that set, whichever directory it runs in.

## The owl image

CI builds the image `ghcr.io/jazzz/flock-owl` for amd64 and arm64, in
[owl-image.yml](../.github/workflows/owl-image.yml). It publishes these tags:

| Tag | Built from |
|---|---|
| `:latest` | main, whenever the crates, `Cargo.lock` or `docker/` change |
| `:sha-<12 hex>` | the same commits, and every release. This matches the `OWL_REV` that each owl reports to roost. |
| `:1.2.3`, `:1.2` | the git tag `v1.2.3` |

The README's commands and compose.owls.yml use `:latest`. Docker downloads
an image once and keeps using that copy, so each machine runs the build it
last downloaded, until someone runs the update commands. roost's dashboard
shows which build each owl runs. To keep a machine on one build, use a sha
tag in place of `latest`.

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

To build the image on your own machine:

```sh
docker build -f docker/owl.Dockerfile --build-arg OWL_REV=$(git rev-parse --short=12 HEAD) -t flock-owl:local .
```

The build compiles liblogosdelivery inside Nix. It takes a long time and
needs a lot of free disk space. To run the result, use `flock-owl:local` in
place of the image name, in the `docker run` command or on the compose
file's `image:` line.
Don't push an image you built yourself. It would replace CI's `:latest`,
which works on amd64 and arm64, with an image for your machine's
architecture only.
