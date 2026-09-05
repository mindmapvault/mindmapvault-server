//! The database schema.
//!
//! Kept out of `postgres.rs` because it is not a query: this runs on every
//! boot, against databases of every age, which is why the `CREATE`s are
//! `IF NOT EXISTS` and the column changes sit in guarded `DO $$ … $$` blocks.
//! Those are idempotent on purpose — do not fold them into plain `ALTER TABLE`.

use anyhow::Context;
use tokio_postgres::Client;

pub async fn ensure_schema(client: &Client) -> anyhow::Result<()> {
    client
        .batch_execute(
            "CREATE TABLE IF NOT EXISTS users (
                id TEXT PRIMARY KEY,
                username TEXT NOT NULL UNIQUE,
                auth_hash TEXT NOT NULL,
                argon2_salt TEXT NOT NULL,
                argon2_params JSONB NOT NULL,
                classical_public_key TEXT NOT NULL,
                pq_public_key TEXT NOT NULL,
                classical_priv_encrypted TEXT NOT NULL,
                pq_priv_encrypted TEXT NOT NULL,
                key_version INTEGER NOT NULL,
                created_at TIMESTAMPTZ NOT NULL,
                subscription_tier TEXT NOT NULL,
                stripe_customer_id TEXT,
                stripe_subscription_id TEXT,
                stripe_subscription_status TEXT,
                subscription_current_period_end TIMESTAMPTZ,
                first_name TEXT,
                last_name TEXT,
                email TEXT,
                is_locked BOOLEAN NOT NULL DEFAULT FALSE,
                locked_reason TEXT,
                admin_note TEXT,
                manual_subscription_tier TEXT,
                manual_subscription_expires_at TIMESTAMPTZ,
                manual_subscription_reason TEXT,
                manual_subscription_granted_by TEXT,
                access_grants_json JSONB NOT NULL DEFAULT '[]'::jsonb
            );

            ALTER TABLE users
            ADD COLUMN IF NOT EXISTS is_locked BOOLEAN NOT NULL DEFAULT FALSE;

            ALTER TABLE users ADD COLUMN IF NOT EXISTS locked_reason TEXT;
            ALTER TABLE users ADD COLUMN IF NOT EXISTS admin_note TEXT;
            ALTER TABLE users ADD COLUMN IF NOT EXISTS manual_subscription_tier TEXT;
            ALTER TABLE users ADD COLUMN IF NOT EXISTS manual_subscription_expires_at TIMESTAMPTZ;
            ALTER TABLE users ADD COLUMN IF NOT EXISTS manual_subscription_reason TEXT;
            ALTER TABLE users ADD COLUMN IF NOT EXISTS manual_subscription_granted_by TEXT;
            ALTER TABLE users ADD COLUMN IF NOT EXISTS access_grants_json JSONB NOT NULL DEFAULT '[]'::jsonb;

            CREATE TABLE IF NOT EXISTS user_account_settings (
                user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
                locale TEXT NOT NULL DEFAULT 'en',
                timezone TEXT NOT NULL DEFAULT 'UTC',
                date_format TEXT NOT NULL DEFAULT 'iso',
                accessibility_reduce_motion BOOLEAN NOT NULL DEFAULT FALSE,
                sync_appearance_across_devices BOOLEAN NOT NULL DEFAULT FALSE,
                default_map_layout TEXT NOT NULL DEFAULT 'mindmap',
                default_map_theme TEXT NOT NULL DEFAULT 'system',
                default_export_format TEXT NOT NULL DEFAULT 'cryptmind',
                default_node_style_preset TEXT NOT NULL DEFAULT 'default',
                user_labels_json TEXT NOT NULL DEFAULT '[]',
                updated_at TIMESTAMPTZ NOT NULL
            );

            -- One row, id pinned to 1. Instance-wide operator settings;
            -- see models/instance_settings.rs.
            CREATE TABLE IF NOT EXISTS instance_settings (
                id SMALLINT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
                registration_enabled BOOLEAN NOT NULL DEFAULT TRUE,
                user_storage_limit_bytes BIGINT NOT NULL DEFAULT 0,
                max_attachment_size_bytes BIGINT NOT NULL DEFAULT 0,
                auth_rate_limit_per_minute INTEGER NOT NULL DEFAULT 30,
                failed_login_threshold INTEGER NOT NULL DEFAULT 10,
                failed_login_lockout_minutes INTEGER NOT NULL DEFAULT 15,
                trust_proxy_headers BOOLEAN NOT NULL DEFAULT FALSE,
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );

            -- One-time codes that allow a sign-up while registration is
            -- closed. See models/invite.rs for why the code is stored as
            -- written rather than hashed.
            CREATE TABLE IF NOT EXISTS registration_invites (
                id TEXT PRIMARY KEY,
                code TEXT NOT NULL UNIQUE,
                label TEXT,
                created_at TIMESTAMPTZ NOT NULL,
                expires_at TIMESTAMPTZ,
                used_at TIMESTAMPTZ,
                used_by_username TEXT
            );

            CREATE TABLE IF NOT EXISTS mind_maps (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                title_encrypted TEXT NOT NULL,
                object_key TEXT NOT NULL,
                eph_classical_public TEXT NOT NULL,
                eph_pq_ciphertext TEXT NOT NULL,
                wrapped_dek TEXT NOT NULL,
                created_at TIMESTAMPTZ NOT NULL,
                updated_at TIMESTAMPTZ NOT NULL,
                current_version_id TEXT,
                version_history JSONB NOT NULL DEFAULT '[]'::jsonb,
                vault_color TEXT,
                vault_note_encrypted TEXT,
                vault_encryption_mode TEXT NOT NULL DEFAULT 'standard',
                max_versions INTEGER NOT NULL,
                vault_labels JSONB NOT NULL DEFAULT '[]'::jsonb
            );

            -- These two were named after MinIO, which the server no longer
            -- assumes it is talking to. Guarded so the rename runs once and a
            -- database created after it is left alone.
            DO $$ BEGIN
                IF EXISTS (
                    SELECT 1 FROM information_schema.columns
                    WHERE table_name = 'mind_maps' AND column_name = 'minio_object_key'
                ) THEN
                    ALTER TABLE mind_maps RENAME COLUMN minio_object_key TO object_key;
                END IF;
                IF EXISTS (
                    SELECT 1 FROM information_schema.columns
                    WHERE table_name = 'mind_maps' AND column_name = 'minio_version_id'
                ) THEN
                    ALTER TABLE mind_maps RENAME COLUMN minio_version_id TO current_version_id;
                END IF;
            END $$;

            ALTER TABLE mind_maps ADD COLUMN IF NOT EXISTS vault_encryption_mode TEXT NOT NULL DEFAULT 'standard';
            ALTER TABLE mind_maps ADD COLUMN IF NOT EXISTS vault_labels JSONB NOT NULL DEFAULT '[]'::jsonb;
            ALTER TABLE user_account_settings ADD COLUMN IF NOT EXISTS user_labels_json TEXT NOT NULL DEFAULT '[]';

            CREATE TABLE IF NOT EXISTS mind_map_attachments (
                id TEXT PRIMARY KEY,
                map_id TEXT NOT NULL REFERENCES mind_maps(id) ON DELETE CASCADE,
                node_id TEXT,
                name TEXT NOT NULL,
                sanitized_name TEXT NOT NULL,
                content_type TEXT NOT NULL,
                size_bytes BIGINT NOT NULL,
                s3_key TEXT NOT NULL,
                s3_version_id TEXT,
                uploaded_by TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                uploaded_at TIMESTAMPTZ NOT NULL,
                encrypted BOOLEAN NOT NULL,
                encryption_meta JSONB,
                checksum_sha256 TEXT,
                status TEXT NOT NULL DEFAULT 'pending'
            );

            CREATE TABLE IF NOT EXISTS mind_map_shares (
                id TEXT PRIMARY KEY,
                map_id TEXT NOT NULL REFERENCES mind_maps(id) ON DELETE CASCADE,
                share_name TEXT NOT NULL,
                share_scope TEXT NOT NULL,
                s3_key TEXT NOT NULL,
                s3_version_id TEXT,
                created_by TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                created_at TIMESTAMPTZ NOT NULL,
                updated_at TIMESTAMPTZ NOT NULL,
                expires_at TIMESTAMPTZ,
                revoked BOOLEAN NOT NULL DEFAULT FALSE,
                include_attachments BOOLEAN NOT NULL DEFAULT FALSE,
                passphrase_hint TEXT,
                content_type TEXT NOT NULL,
                size_bytes BIGINT NOT NULL,
                encryption_meta JSONB NOT NULL,
                checksum_sha256 TEXT,
                status TEXT NOT NULL DEFAULT 'pending'
            );

            CREATE TABLE IF NOT EXISTS mind_map_share_attachments (
                id TEXT PRIMARY KEY,
                share_id TEXT NOT NULL REFERENCES mind_map_shares(id) ON DELETE CASCADE,
                source_attachment_id TEXT,
                node_id TEXT,
                name TEXT NOT NULL,
                sanitized_name TEXT NOT NULL,
                content_type TEXT NOT NULL,
                size_bytes BIGINT NOT NULL,
                s3_key TEXT NOT NULL,
                s3_version_id TEXT,
                uploaded_at TIMESTAMPTZ NOT NULL,
                encryption_meta JSONB NOT NULL,
                checksum_sha256 TEXT,
                status TEXT NOT NULL DEFAULT 'pending'
            );

            CREATE TABLE IF NOT EXISTS admin_audit_events (
                id TEXT PRIMARY KEY,
                entity_type TEXT NOT NULL,
                entity_id TEXT NOT NULL,
                action_type TEXT NOT NULL,
                summary TEXT NOT NULL,
                detail TEXT,
                actor TEXT,
                created_at TIMESTAMPTZ NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_registration_invites_created_at ON registration_invites (created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_mind_maps_user_id ON mind_maps (user_id);
            CREATE INDEX IF NOT EXISTS idx_mind_map_attachments_map_id ON mind_map_attachments (map_id, uploaded_at DESC);
            CREATE INDEX IF NOT EXISTS idx_mind_map_attachments_uploaded_by ON mind_map_attachments (uploaded_by);
            CREATE INDEX IF NOT EXISTS idx_mind_map_attachments_status ON mind_map_attachments (status);
            CREATE INDEX IF NOT EXISTS idx_admin_audit_events_created_at ON admin_audit_events (created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_mind_map_shares_map_id ON mind_map_shares (map_id, created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_mind_map_shares_status ON mind_map_shares (status, revoked);
            CREATE INDEX IF NOT EXISTS idx_mind_map_shares_expires_at ON mind_map_shares (expires_at);
            CREATE INDEX IF NOT EXISTS idx_mind_map_share_attachments_share_id ON mind_map_share_attachments (share_id, uploaded_at DESC);
            CREATE INDEX IF NOT EXISTS idx_mind_map_share_attachments_status ON mind_map_share_attachments (status);",
        )
        .await
        .context("failed to ensure PostgreSQL schema")?;

    Ok(())
}
