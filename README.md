# o11y-flock

Flock is an observability and canary test platform for LogosChat group chats. It runs many chat clients, called
**owls**. Each owl creates a group, adds other owls to groups and posts
messages, without stopping. Every owl reports what it sends and receives to the
**roost**, which checks that each message reached every member.

This page shows how to run owls. [How owls work](docs/owls.md) covers how
owls get their names, every setting, running owls with Docker Compose or
without Docker, and building the owl.

## Run an owl

You need Docker. Run this command, with the three values filled in:

```sh
docker run -d --pull always --name owl-1 --restart unless-stopped -v owl-1:/var/lib/owl -e ROOST_URL=http://roost.example.org:30117 -e FLOCK_KEY=the-flock-key -e OWL_OPERATOR=your-name ghcr.io/jazzz/flock-owl:latest
```

- `ROOST_URL` is roost's address and `FLOCK_KEY` is the flock key. roost's
  admin gives you both, usually as this command with both already filled
  in.
- `OWL_OPERATOR` is your name or handle. It appears next to your owl on
  roost's dashboard, so people know whom to ask about it.

The owl appears on roost's dashboard within a minute. It runs until you stop
it, and Docker starts it again whenever Docker itself restarts. The volume
`owl-1` holds the owl's ID, so the owl keeps its name even if you delete the
container and run the command again.

## Look after it

```sh
docker logs -f owl-1     # watch the owl's log
docker stop owl-1        # stop it
docker start owl-1       # start it again, with the same name
```

To update the owl to the newest build, delete the container, then run the
same `docker run` command again:

```sh
docker stop owl-1 && docker rm owl-1
```

`--pull always` in the command downloads the newest owl. The new owl reads
its ID from the volume `owl-1`, so it keeps its name.

## Run more owls

Run the command again with `owl-2` in place of `owl-1`, then `owl-3`, and so
on. `owl-1` appears twice in the command, in `--name owl-1` and in
`-v owl-1:/var/lib/owl`; change both. Every owl needs a volume of its own.

For many owls, Docker Compose is easier. Download the compose file and the
example settings:

```sh
curl -fsSLo compose.yaml https://raw.githubusercontent.com/jazzz/o11y-flock/main/deploy/compose.owls.yml
curl -fsSLo .env https://raw.githubusercontent.com/jazzz/o11y-flock/main/deploy/.env.example
```

Open `.env`, fill in roost's address, the flock key and your name, and set
how many owls to run. Then, in the same directory, run
`docker compose up -d`. [How owls work](docs/owls.md#running-owls-with-docker-compose)
lists the other commands.

## Run ephemeral owls

To add owls for a short while, run **ephemeral owls**. roost names them
`owlet-01`, `owlet-02` and so on. Two minutes after an ephemeral owl stops,
roost can give its name to the next ephemeral owl that starts. When an
ephemeral owl stops, roost doesn't count the messages it misses as dropped.

The command is the same, with no volume and with `OWL_EPHEMERAL=true`:

```sh
docker run -d --rm --pull always --name ephemeral-owl-1 -e ROOST_URL=http://roost.example.org:30117 -e FLOCK_KEY=the-flock-key -e OWL_OPERATOR=your-name -e OWL_EPHEMERAL=true ghcr.io/jazzz/flock-owl:latest
```

`docker stop ephemeral-owl-1` stops the ephemeral owl and deletes it.
