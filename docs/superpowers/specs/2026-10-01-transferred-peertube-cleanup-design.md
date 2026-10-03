# Transferred PeerTube Cleanup

## Goal

Extend the authenticated Cleanup page so it lists both untracked PeerTube
videos and videos whose Tubemin submissions are already `deleted` or legacy
`handed_off` but still remain on PeerTube.

## Behavior

- Active and in-progress Tubemin submissions are not cleanup candidates.
- `deleted` and `handed_off` videos are shown with a transferred label.
- True PeerTube orphans retain the existing orphan label.
- Cleanup deletes only the selected PeerTube videos.
- Tubemin submissions and transfer-history rows are never deleted or changed.
- The server validates each requested UUID as either an orphan or an archived
  submission before allowing PeerTube deletion.
