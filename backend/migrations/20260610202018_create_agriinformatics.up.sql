CREATE TABLE IF NOT EXISTS public.agriinformatics (
    id SERIAL PRIMARY KEY,
    nama_tugas TEXT NOT NULL,
    deadline TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    reminder_1h_sent BOOLEAN NOT NULL DEFAULT FALSE
);
