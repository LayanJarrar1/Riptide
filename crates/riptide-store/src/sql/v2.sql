-- v2: media downloads (yt-dlp). `kind` says which engine runs the record; `media_format` is the
-- yt-dlp format selector the user picked in the quality picker (NULL for plain HTTP).
ALTER TABLE downloads ADD COLUMN kind TEXT NOT NULL DEFAULT 'http' CHECK (kind IN ('http', 'media'));
ALTER TABLE downloads ADD COLUMN media_format TEXT;
