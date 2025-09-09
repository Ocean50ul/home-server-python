#!/bin/bash
# 01-init.sh
set -e

# The path is now relative to this script's location inside the container
SQL_FILE="/docker-entrypoint-initdb.d/sql/permissions.sql"

psql -v ON_ERROR_STOP=1 \
    --username "$POSTGRES_USER" \
    --dbname "$DB_NAME" \
    -v DB_NAME="$DB_NAME" \
    -v app_architect_pass="$APP_ARCHITECT_PASS" \
    -v app_user_pass="$APP_USER_PASS" \
    -v app_readonly_pass="$APP_READONLY_PASS" \
    -f "$SQL_FILE"