-- Hapus Policy
DROP POLICY IF EXISTS "Enable access to all users" ON public.user_completions;
DROP POLICY IF EXISTS "Enable access to all users" ON public.wa_logs;
DROP POLICY IF EXISTS "Enable access to all users" ON public.assignments;
DROP POLICY IF EXISTS "Enable access to all users" ON public.courses;

-- Hapus Tabel 
DROP TABLE IF EXISTS public.pekan_ilkomers;
DROP TABLE IF EXISTS public.user_completions;
DROP TABLE IF EXISTS public.wa_logs;
DROP TABLE IF EXISTS public.assignments;
DROP TABLE IF EXISTS public.courses;

-- Hapus Extension
DROP EXTENSION IF EXISTS "uuid-ossp";