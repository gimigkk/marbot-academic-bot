-- 1. Extension (Untuk UUID)
create extension if not exists "uuid-ossp";


-- TABEL 1: COURSES (Mata Kuliah)
create table public.courses (
  id uuid default uuid_generate_v4() primary key,
  name text not null unique,   -- Nama Matkul (Unique Value)
  created_at timestamp with time zone default timezone('utc'::text, now()) not null
);

-- INPUT Mata Kuliah
insert into public.courses (name) values 
  ('Pro Gaming'),
  ('Struktur Data'),
  ('Rekayasa Perangkat Lunak'),
  ('Organisasi dan Arsitektur Komputer'),
  ('Metode Kuantitatif'),
  ('Grafika Komputer dan Visualisasi'),
  ('User Experience Design');



-- TABEL 2: ASSIGNMENTS (Tugas)
create table public.assignments (
  id uuid default uuid_generate_v4() primary key,
  created_at timestamp with time zone default timezone('utc'::text, now()) not null,
  
  -- Relasi ke Tabel Courses
  course_id uuid references public.courses(id) on delete set null,
  
  -- Data Tugas
  title text not null,       -- Judul Tugas
  description text not null, -- Deskripsi
  deadline timestamp with time zone,

  -- Paralel
  parallel_code text check (parallel_code = lower(parallel_code)),

  -- Sumber Chat
  sender_id text,           -- Nomor Pengirim
  message_ids text[] not null 
);

-- TABEL 3: WA LOGS (Debugging Purpose Only)
create table public.wa_logs (
  id uuid default uuid_generate_v4() primary key,
  created_at timestamp with time zone default timezone('utc'::text, now()) not null,
  event_type text,
  payload jsonb,
  processed boolean default false
);

-- Security 
alter table public.courses enable row level security;
alter table public.assignments enable row level security;
alter table public.wa_logs enable row level security;

-- Policy
create policy "Enable access to all users" on public.courses for all using (true) with check (true);
create policy "Enable access to all users" on public.assignments for all using (true) with check (true);
create policy "Enable access to all users" on public.wa_logs for all using (true) with check (true);