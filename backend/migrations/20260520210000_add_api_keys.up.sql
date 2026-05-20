CREATE TABLE IF NOT EXISTS public.user_api_keys (
    user_id TEXT PRIMARY KEY,
    api_key TEXT UNIQUE NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    last_used_at TIMESTAMP WITH TIME ZONE
);

CREATE INDEX IF NOT EXISTS idx_user_api_keys_api_key ON public.user_api_keys(api_key);

DO $$ 
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_policies WHERE tablename = 'user_api_keys' AND policyname = 'Enable access to all users') THEN
    alter table public.user_api_keys enable row level security;
    create policy "Enable access to all users" on public.user_api_keys for all using (true) with check (true);
  END IF;
END $$;
