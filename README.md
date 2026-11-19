# o11y-flock

Flock is an observability and canary test platform for LogosChat group chats. It runs many chat clients, called
**owls**. Each owl creates a group, adds other owls to groups and posts
messages, without stopping. Every owl reports what it sends and receives to
**roost**, which checks that each message reached every member.

This page shows how to run owls in Docker. [How owls work](docs/owls.md)
covers how owls get their names, every setting, running owls without Docker,
and building the owl.

## Run owls

You need Docker with Compose. From roost's admin, you need two things:
roost's address and the flock key.

1. Download flock:

   ```sh
   git clone https://github.com/jazzz/o11y-flock.git
   cd o11y-flock
   ```

2. Create the file `deploy/.env` with these four lines, and replace the
   values in angle brackets:

   ```ini
   ROOST_URL=<roost's address, e.g. http://roost.example.org:30117>
   FLOCK_KEY=<the flock key>
   OWL_OPERATOR=<your name or handle>
   OWLS=5
   ```

   - `OWL_OPERATOR` appears next to your owls on roost's dashboard, so
     people know whom to ask about them.
   - `OWLS` is the number of owls to run.
   - If roost runs on this machine under Docker Desktop, set
     `ROOST_URL=http://host.docker.internal:30117`.

   Every `docker compose` command below reads this file, so you write the
   settings once. Git ignores the file, so the key never gets committed.

3. Start the owls:

   ```sh
   docker compose -f deploy/compose.owls.yml up -d
   ```

   The first start downloads the owl image. Your owls then appear on roost's
   dashboard under your name. They run until you stop them, and Docker
   starts them again whenever Docker itself restarts.

4. Check on the owls:

   ```sh
   docker compose -f deploy/compose.owls.yml ps
   docker compose -f deploy/compose.owls.yml logs -f
   ```

5. Stop the owls, and start them again later:

   ```sh
   docker compose -f deploy/compose.owls.yml stop
   docker compose -f deploy/compose.owls.yml up -d
   ```

   Each owl comes back with the name it had before.

   **Don't run `docker compose down`.** It deletes the containers, and the
   owls that the next `up` starts get new names. roost holds each old name
   for a week.

6. Update to the latest owl:

   ```sh
   docker compose -f deploy/compose.owls.yml pull
   docker compose -f deploy/compose.owls.yml up -d
   ```

   The updated owls keep their names.

## Run ephemeral owls

To add owls for a short while, run **ephemeral owls**. roost names them
`owlet-01`, `owlet-02` and so on. Two minutes after an ephemeral owl stops,
roost can give its name to the next ephemeral owl that starts. When an
ephemeral owl stops, roost doesn't count the messages it misses as dropped.

Start 20 ephemeral owls:

```sh
docker compose -f deploy/compose.owls.yml up -d --scale owlet=20 owlet
```

`--scale owlet=20` sets how many ephemeral owls run. The final `owlet` limits
the command to ephemeral owls, so it leaves your other owls alone.

Stop and remove every ephemeral owl:

```sh
docker compose -f deploy/compose.owls.yml rm -sf owlet
```

To start ephemeral owls with every `up`, add `OWLETS=20` to `deploy/.env`.
