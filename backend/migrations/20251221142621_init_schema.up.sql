create extension if not exists "uuid-ossp";

create table public.assignments (
  id uuid default uuid_generate_v4() primary key,
  created_at timestamp with time zone default timezone('utc'::text, now()) not null,
  
  -- Data Akademik
  course_name text not null,        -- Nama Mata Kuliah
  description text not null,        -- Detail Tugas
  deadline timestamp with time zone,-- Tenggat waktu tugas
  
  -- Metadata Chat (untuk tracking asal tugas)
  source_chat_id text,              -- ID Grup/Chat tempat tugas ditemukan
  source_message_id text,           -- ID Pesan asli (untuk referensi/reply)
  reporter_number text,             -- Nomor WA pelapor/PJ yang mengirim info
  
  -- Status
  is_completed boolean default false,
  completed_at timestamp with time zone
);

-- Indexing untuk mempercepat pencarian tugas berdasarkan deadline
create index assignments_deadline_idx on public.assignments (deadline);
create index assignments_course_idx on public.assignments (course_name);

create table public.wa_logs (
  id uuid default uuid_generate_v4() primary key,
  created_at timestamp with time zone default timezone('utc'::text, now()) not null,
  event_type text,           
  payload jsonb,               -- Menyimpan seluruh JSON dari WAHA
  processed boolean default false
);
