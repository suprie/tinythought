ALTER TABLE post_images DROP CONSTRAINT post_images_draft_id_key;
CREATE INDEX post_images_draft_id_idx ON post_images (draft_id);

UPDATE post_drafts AS d
SET body_markdown = d.body_markdown || E'\n\n![Post image](/post-images/' || i.filename || E')\n',
    revision = d.revision + 1
FROM post_images AS i
WHERE i.draft_id = d.id;
