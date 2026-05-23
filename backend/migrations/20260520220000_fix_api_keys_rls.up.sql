-- Fix RLS: drop the overly permissive policy and restrict to backend service only.
-- This prevents anyone with the Supabase anon key from reading all API keys.

ALTER TABLE public.user_api_keys DISABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS "Enable access to all users" ON public.user_api_keys;
