# Transfer History Cleanup

## Goal

Allow an authenticated Tubemin user to remove stale transfer-history rows whose
state is already `deleted`, including legacy Jellypik handoff records stored as
`submissions.status = 'handed_off'`.

## Behavior

- Add a `Clear archived records` action to the Transfers page.
- Display the number of current `deleted` and legacy `handed_off` records when
  available.
- Require confirmation before deletion.
- The confirmation must state that this removes history rows only; it does not
  delete PeerTube videos, source files, or Navidrome output.
- The backend deletes transfer rows where `state = 'deleted'` and legacy
  submission rows where `status = 'handed_off'`.
- Completed, failed, pending, active, and non-legacy submission rows remain
  untouched.
- The action is protected by Tubemin’s existing authenticated page/API layer.

## API and data flow

1. The Transfers page loads the current transfer list as it does today.
2. The page renders the cleanup action only when deleted or legacy handed-off
   rows exist.
3. After confirmation, the browser sends an authenticated request to a new
   transfer-cleanup endpoint.
4. The endpoint performs constrained deletes for both archive states in one
   database transaction and returns separate counts for deleted transfers and
   legacy handed-off submissions.
5. The page refreshes the transfer list and shows the result.

No PeerTube API, source-cleanup, or downstream-service operation is performed
by this action.

## Error handling

- Empty archive set: return success with zero removed rows.
- Database failure: return an error response and leave the page data unchanged.
- Failed cleanup requests must not partially delete either archive category or
  any other transfer state.

## Verification

- Add a database test proving only `deleted` transfers and `handed_off`
  submissions are removed.
- Add handler/page coverage for the cleanup endpoint and button text.
- Build and run the Tubemin container, then verify the Transfers page cleanup
  action against the local transfer history.
