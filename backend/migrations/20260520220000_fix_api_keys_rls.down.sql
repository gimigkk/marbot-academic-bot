-- Rollback: re-enable the original (permissive) policy
ALTER TABLE public.user_api_keys ENABLE ROW LEVEL SECURITY;
CREATE POLICY "Enable access to all users" ON public.user_api_keys
  FOR ALL USING (true) WITH CHECK (true);
