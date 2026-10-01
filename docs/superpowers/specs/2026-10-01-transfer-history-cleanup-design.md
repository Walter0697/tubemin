# Transfer History Cleanup

## Goal

Allow an authenticated Tubemin user to remove stale transfer-history rows whose
state is already `deleted`.

## Behavior

- Add a `Clear deleted records` action to the Transfers page.
- Display the number of currently deleted records when available.
- Require confirmation before deletion.
- The confirmation must state that this removes history rows only; it does not
  delete PeerTube videos, source files, or Navidrome output.
- The backend deletes only rows where `state = 'deleted'`.
- Completed, failed, pending, and active transfer rows remain untouched.
- The action is protected by Tubemin’s existing authenticated page/API layer.

## API and data flow

1. The Transfers page loads the current transfer list as it does today.
2. The page renders the cleanup action only when deleted rows exist.
3. After confirmation, the browser sends an authenticated request to a new
   transfer-cleanup endpoint.
4. The endpoint performs one database delete constrained to `state = 'deleted'`
   and returns the number of removed rows.
5. The page refreshes the transfer list and shows the result.

No PeerTube API, source-cleanup, or downstream-service operation is performed
by this action.

## Error handling

- Empty deleted set: return success with zero removed rows.
- Database failure: return an error response and leave the page data unchanged.
- Failed cleanup requests must not partially delete other transfer states.

## Verification

- Add a database test proving only `deleted` rows are removed.
- Add handler/page coverage for the cleanup endpoint and button text.
- Build and run the Tubemin container, then verify the Transfers page cleanup
  action against the local transfer history.
