-- Runs once, on first cluster init. The schema itself is owned by the sqlx
-- migrations in engine/migrations; this only enables extensions that need
-- superuser so the migration role does not.
CREATE EXTENSION IF NOT EXISTS postgis;
CREATE EXTENSION IF NOT EXISTS pg_stat_statements;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
