-- 01-init-permissions.sql

-- === 1. Role Creation ===.
CREATE ROLE app_architect WITH LOGIN PASSWORD :'app_architect_pass';
CREATE ROLE app_user      WITH LOGIN PASSWORD :'app_user_pass';
CREATE ROLE app_readonly  WITH LOGIN PASSWORD :'app_readonly_pass';

-- === 2. Database-Level Privileges ===
GRANT CONNECT ON DATABASE :"DB_NAME" TO app_architect, app_user, app_readonly;

-- === 3. Application Schema Creation ===
CREATE SCHEMA app;
GRANT USAGE, CREATE ON SCHEMA app TO app_architect;
GRANT USAGE ON SCHEMA app TO app_user, app_readonly;

-- === 4. Schema-Level Privileges ===
-- Set default privileges for any NEW tables/sequences the architect creates.
ALTER DEFAULT PRIVILEGES FOR ROLE app_architect IN SCHEMA app
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO app_user;

-- The app_user will need to use sequences (e.g., for primary keys).
ALTER DEFAULT PRIVILEGES FOR ROLE app_architect IN SCHEMA app
GRANT USAGE, SELECT ON SEQUENCES TO app_user;

-- The readonly user should ONLY be able to read from new tables.
ALTER DEFAULT PRIVILEGES FOR ROLE app_architect IN SCHEMA app
GRANT SELECT ON TABLES TO app_readonly;

-- === 5. Set search_path for Convenience ===
ALTER ROLE app_architect SET search_path TO app, public;
ALTER ROLE app_user      SET search_path TO app, public;
ALTER ROLE app_readonly  SET search_path TO app, public;