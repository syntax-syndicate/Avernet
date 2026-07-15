-- Session collection (收藏): per-participant collected flag on the side table.
ALTER TABLE `bcs_session_participants`
  ADD COLUMN IF NOT EXISTS `collected` tinyint(4) NOT NULL DEFAULT '0';

CREATE INDEX `idx_collected`
  ON `bcs_session_participants` (`env`, `group_id`, `bot_uuid`, `collected`);