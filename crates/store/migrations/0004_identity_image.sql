-- Spec 0012: the identity's profile image URL as the payload carried it
-- (`imageUrl` / `_links.avatar.href`). NULL until a payload with one is seen.
ALTER TABLE identity ADD COLUMN image_url TEXT;
