INSERT INTO public.courses (name, aliases)
VALUES 
  ('Analisis Algoritme', ARRAY['analgor', 'aa', 'algoritme', 'algoritma', 'kom1303']),
  ('Komunikasi Data dan Jaringan Komputer', ARRAY['komdat', 'jarkom', 'kj', 'kdj', 'jaringan komputer', 'kom1314']),
  ('Kecerdasan Buatan', ARRAY['ai', 'kb', 'kom1327']),
  ('Sistem Operasi', ARRAY['so', 'os', 'kom1313']),
  ('Sistem Informasi', ARRAY['si', 'is', 'kom133a']),
  ('Keamanan Informasi', ARRAY['kemin', 'ki', 'kaminfo', 'kom1326'])
ON CONFLICT (name) DO NOTHING;
