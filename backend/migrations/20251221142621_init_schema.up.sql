-- 1. Extension (Untuk UUID)
create extension if not exists "uuid-ossp";

-- TABEL 1: COURSES (Mata Kuliah)
create table if not exists public.courses (
  id uuid default uuid_generate_v4() primary key,
  name text not null unique,
  aliases text[],           
  created_at timestamp with time zone default timezone('utc'::text, now()) not null
);

-- INPUT Mata Kuliah (only if table is empty)
insert into public.courses (name, aliases) 
select * from (values 
  ('Pemrograman', ARRAY['pemrog']),
  ('Struktur Data', ARRAY['Ssrukdat', 'sd']),
  ('Rekayasa Perangkat Lunak', ARRAY['rpl']),
  ('Organisasi dan Arsitektur Komputer', ARRAY['orkom', 'oak']),
  ('Metode Kuantitatif', ARRAY['metkun', 'mk', 'metcuan']),
  ('Grafika Komputer dan Visualisasi', ARRAY['grafkom', 'gkv', 'gk']),
  ('User Experience Design', ARRAY['ux', 'uxd'])
) as v(name, aliases)
where not exists (select 1 from public.courses);

-- TABEL 2: ASSIGNMENTS (Tugas)
create table if not exists public.assignments (
  id uuid default uuid_generate_v4() primary key,
  created_at timestamp with time zone default timezone('utc'::text, now()) not null,
  course_id uuid references public.courses(id) on delete set null,
  title text not null,
  description text not null,
  deadline timestamp with time zone,
  parallel_codes text[],
  sender_id text,
  message_ids text[] not null,
  reminder_1h_sent boolean not null default false,
  relating_messages TEXT[] DEFAULT '{}'
);

-- TABEL 3: WA LOGS
create table if not exists public.wa_logs (
  id uuid default uuid_generate_v4() primary key,
  created_at timestamp with time zone default timezone('utc'::text, now()) not null,
  event_type text,
  payload jsonb,
  processed boolean default false
);

-- TABEL 4: USER COMPLETIONS
CREATE TABLE IF NOT EXISTS public.user_completions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id VARCHAR(255) NOT NULL,
    assignment_id UUID NOT NULL REFERENCES public.assignments(id) ON DELETE CASCADE,
    completed_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    UNIQUE(user_id, assignment_id)
);

-- TABEL 5: USER COURSE SETTINGS
CREATE TABLE IF NOT EXISTS public.user_course_settings (
    user_id TEXT NOT NULL,
    course_id UUID NOT NULL REFERENCES public.courses(id) ON DELETE CASCADE,
    parallel_code TEXT NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    PRIMARY KEY (user_id, course_id)
);

-- TABEL 6: USER PREFRENCE
CREATE TABLE IF NOT EXISTS public.user_preferences (
    user_id TEXT PRIMARY KEY, -- Nomor WA
    daily_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_user_settings_user ON public.user_course_settings(user_id);
CREATE INDEX IF NOT EXISTS idx_user_completions_user ON public.user_completions (user_id, completed_at DESC);
CREATE INDEX IF NOT EXISTS idx_user_completions_lookup ON public.user_completions (user_id, assignment_id);

-- Security 
DO $$ 
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_policies WHERE tablename = 'courses' AND policyname = 'Enable access to all users') THEN
    alter table public.courses enable row level security;
    create policy "Enable access to all users" on public.courses for all using (true) with check (true);
  END IF;
  
  IF NOT EXISTS (SELECT 1 FROM pg_policies WHERE tablename = 'assignments' AND policyname = 'Enable access to all users') THEN
    alter table public.assignments enable row level security;
    create policy "Enable access to all users" on public.assignments for all using (true) with check (true);
  END IF;
  
  IF NOT EXISTS (SELECT 1 FROM pg_policies WHERE tablename = 'wa_logs' AND policyname = 'Enable access to all users') THEN
    alter table public.wa_logs enable row level security;
    create policy "Enable access to all users" on public.wa_logs for all using (true) with check (true);
  END IF;
  
  IF NOT EXISTS (SELECT 1 FROM pg_policies WHERE tablename = 'user_completions' AND policyname = 'Enable access to all users') THEN
    alter table public.user_completions enable row level security;
    create policy "Enable access to all users" on public.user_completions for all using (true) with check (true);
  END IF;
END $$;