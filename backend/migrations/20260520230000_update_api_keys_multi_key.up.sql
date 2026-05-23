ALTER TABLE public.user_api_keys
    DROP CONSTRAINT IF EXISTS user_api_keys_pkey;

ALTER TABLE public.user_api_keys
    ADD COLUMN IF NOT EXISTS key_name TEXT;

UPDATE public.user_api_keys
SET key_name = COALESCE(NULLIF(key_name, ''), 'default');

ALTER TABLE public.user_api_keys
    ALTER COLUMN key_name SET NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_user_api_keys_user_id_key_name
    ON public.user_api_keys(user_id, key_name);

CREATE UNIQUE INDEX IF NOT EXISTS idx_user_api_keys_api_key
    ON public.user_api_keys(api_key);

CREATE INDEX IF NOT EXISTS idx_user_api_keys_user_id
    ON public.user_api_keys(user_id);