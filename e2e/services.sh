#!/usr/bin/env bash
# The databases the E2E tests browse: throwaway containers, labelled as the Compose project of e2e/fixtures/shop so the
# Servers panel lists them under that folder. `up` recreates them (fresh data every run) and waits until each answers;
# `down` removes them.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
shop="$here/fixtures/shop"
project=pretorium-e2e

down() {
  ids=$(docker ps -aq --filter "label=com.docker.compose.project=$project")
  [ -z "$ids" ] || docker rm -f $ids >/dev/null
}

# run <service> <docker run args…>
run() {
  local service=$1
  shift
  docker run -d --name "$project-$service" \
    --label "com.docker.compose.project=$project" \
    --label "com.docker.compose.project.working_dir=$shop" \
    --label "com.docker.compose.service=$service" \
    "$@" >/dev/null
}

# wait_for <what> <command…>: retries for up to 90s.
wait_for() {
  local what=$1
  shift
  for _ in $(seq 1 180); do
    if "$@" >/dev/null 2>&1; then return 0; fi
    sleep 0.5
  done
  echo "e2e services: $what didn't come up" >&2
  return 1
}

up() {
  down
  run pg -p 127.0.0.1:54391:5432 -e POSTGRES_PASSWORD=shop-secret -e POSTGRES_DB=shop \
    -v "$here/seed/pg.sql:/docker-entrypoint-initdb.d/seed.sql:ro" postgres:16-alpine
  run pg-hidden -e POSTGRES_PASSWORD=hidden-secret -e POSTGRES_DB=warehouse \
    -v "$here/seed/pg-hidden.sql:/docker-entrypoint-initdb.d/seed.sql:ro" postgres:16-alpine
  # Passwords in secret files: the containers' settings don't give them away.
  run pg-env -p 127.0.0.1:54392:5432 -e POSTGRES_USER=app -e POSTGRES_DB=app -e POSTGRES_PASSWORD_FILE=/run/pw \
    -v "$here/fixtures/secrets/pg_env_password:/run/pw:ro" -v "$here/seed/pg-env.sql:/docker-entrypoint-initdb.d/seed.sql:ro" postgres:16-alpine
  run pg-locked -p 127.0.0.1:54393:5432 -e POSTGRES_USER=keeper -e POSTGRES_DB=vault -e POSTGRES_PASSWORD_FILE=/run/pw \
    -v "$here/fixtures/secrets/pg_locked_password:/run/pw:ro" -v "$here/seed/pg-locked.sql:/docker-entrypoint-initdb.d/seed.sql:ro" postgres:16-alpine
  run mariadb -p 127.0.0.1:33091:3306 -e MARIADB_ROOT_PASSWORD=root-secret -e MARIADB_DATABASE=shop \
    -e MARIADB_USER=shop -e MARIADB_PASSWORD=shop-secret -v "$here/seed/mariadb.sql:/docker-entrypoint-initdb.d/seed.sql:ro" mariadb:11.4
  run redis -p 127.0.0.1:63791:6379 redis:7-alpine redis-server --requirepass redis-secret
  run mongo -e MONGO_INITDB_ROOT_USERNAME=root -e MONGO_INITDB_ROOT_PASSWORD=mongo-secret -e MONGO_INITDB_DATABASE=shop \
    -v "$here/seed/mongo.js:/docker-entrypoint-initdb.d/seed.js:ro" mongo:7

  mkdir -p "$shop/data"
  rm -f "$shop/data/notes.sqlite"
  sqlite3 "$shop/data/notes.sqlite" "CREATE TABLE notes (id INTEGER PRIMARY KEY, title TEXT NOT NULL, body TEXT, pinned INTEGER DEFAULT 0);
    INSERT INTO notes (title, body, pinned) VALUES ('Shopping', 'milk, coffee', 1), ('Ideas', 'a database browser', 0), ('Empty', NULL, 0);"

  # Over TCP: the init scripts' temporary server only listens on the socket.
  for s in pg:shop:postgres pg-hidden:warehouse:postgres pg-env:app:app pg-locked:vault:keeper; do
    IFS=: read -r name db user <<<"$s"
    wait_for "$name" docker exec "$project-$name" pg_isready -q -h 127.0.0.1 -U "$user" -d "$db"
  done
  wait_for mariadb docker exec "$project-mariadb" mariadb -h127.0.0.1 -uroot -proot-secret -e "SELECT 1 FROM shop.suppliers"
  wait_for redis docker exec "$project-redis" redis-cli -a redis-secret --no-auth-warning ping
  r() { docker exec "$project-redis" redis-cli -a redis-secret --no-auth-warning "$@" >/dev/null; }
  r HSET user:1 name Ada email ada@example.com
  r SET session:abc '{"user":1}' EX 3600
  r RPUSH queue:emails welcome reminder digest
  r SADD tags red green blue
  r ZADD leaderboard 30 ada 20 bob 10 cy
  r SET counter 41
  r -n 2 SET archive:2019 old
  wait_for "mongo init" sh -c "docker logs $project-mongo 2>&1 | grep -q 'init process complete'"
  wait_for mongo docker exec "$project-mongo" mongosh --quiet -u root -p mongo-secret --eval "db.getSiblingDB('shop').reviews.countDocuments()"
  echo "e2e services: up"
}

case "${1:-up}" in
  up) up ;;
  down) down ;;
  *) echo "usage: $0 [up|down]" >&2; exit 2 ;;
esac
